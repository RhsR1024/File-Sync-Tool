use crate::device_simulator::alarms::scheduler::{
    AlarmApplicationStatus, AlarmClock, AlarmDeviceTarget,
    AlarmDispatchMode as ScheduledDispatchMode, AlarmFuture, AlarmInvocation, AlarmJobSnapshot,
    AlarmScheduler, AlarmSchedulerLimits, AlarmSendError, AlarmSender, AlarmSenderResponse,
    OneShotAlarmJob, OutboundAlarmRequest, PeriodicAlarmJob, RunningAlarmJob,
    ScheduledAlarmJobState, SystemAlarmClock,
};
use crate::device_simulator::alarms::{
    embedded_image_count, AlarmBuildContext, AlarmHandlerDefinition, AlarmHandlerId,
    AlarmHandlerRegistry, AlarmRequestDefinition, AlarmTypeId, BodyEncoding, CompiledTemplate,
    DynamicField, FixtureProvenance, HandlerEvidence, HttpMethod, ImageAssetRef,
    ImageAttachmentDefinition, ImageCache, ImageExtension, ImagePolicy, PackIdentity,
    PlatformEvidence, PlatformVerification, RecoveryDefinition, RecoveryTrigger,
    ResponseSuccessRule, SharedImageCache, SourceBinding, TransportDefinition,
};
use crate::device_simulator::api::{
    AlarmDispatchMode, AlarmJobRequest, AlarmJobStatsSnapshot, AlarmTriggerResult,
    DeviceIdentityPreviewDto, DevicePreview, TargetPlatformConfig,
};
use crate::device_simulator::assets::catalog::PackManifest;
use crate::device_simulator::errors::SimulatorErrorBody;
use crate::device_simulator::models::AlarmJobState;
use crate::device_simulator::profiles::scope::{FirstReleaseProfileId, TargetPlatform};
use crate::device_simulator::runtime_assets::{PinnedPackDirectory, RuntimeAssetLayout};
use parking_lot::RwLock;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fs;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

/// Alarm receiver endpoints advertised by every platform currently subscribed
/// to this simulator session. The protocol runtime adds or refreshes entries;
/// alarm jobs select one entry explicitly when more than one is active.
pub type SharedLearnedAlarmSubscriptions = Arc<RwLock<LearnedAlarmSubscriptions>>;

/// A subscription endpoint parsed out of a LAPI `Event/Subscription` body.
///
/// Real UMS deployments allocate this port dynamically (observed: `22815`), so
/// no port range may be assumed — only a non-zero port is required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearnedAlarmEndpoint {
    /// Stable for repeated subscriptions from the same source and advertised
    /// host. The dynamically allocated receiver port may change on renewal.
    pub id: String,
    /// TCP peer that delivered the subscription request.
    pub source_ip: IpAddr,
    /// `IPAddress` from the subscription body; `None` falls back to the TCP peer
    /// that delivered this subscription, preserving platform identity even when
    /// several UMS instances subscribe to the same virtual device.
    pub host: Option<Ipv4Addr>,
    /// `Port` from the subscription body. Never `0`.
    pub port: u16,
    /// `Duration` in seconds, when the platform declared one.
    pub duration_secs: Option<u32>,
    /// Wall-clock milliseconds when this subscription was last accepted.
    pub learned_at_ms: u64,
}

impl LearnedAlarmEndpoint {
    pub fn new(
        source_ip: IpAddr,
        host: Option<Ipv4Addr>,
        port: u16,
        duration_secs: Option<u32>,
        learned_at_ms: u64,
    ) -> Self {
        let advertised_host = host
            .map(|address| address.to_string())
            .unwrap_or_else(|| "configured-host".into());
        Self {
            id: format!("{source_ip}|{advertised_host}"),
            source_ip,
            host,
            port,
            duration_secs,
            learned_at_ms,
        }
    }

    /// Wall-clock milliseconds when the platform's subscription lapses. `None`
    /// when the platform declared no `Duration`.
    pub fn expires_at_ms(&self) -> Option<u64> {
        self.duration_secs.map(|seconds| {
            self.learned_at_ms
                .saturating_add(u64::from(seconds).saturating_mul(1_000))
        })
    }

    pub fn is_expired_at(&self, timestamp_ms: u64) -> bool {
        self.expires_at_ms()
            .is_some_and(|expires_at_ms| expires_at_ms <= timestamp_ms)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LearnedAlarmSubscriptions {
    entries: BTreeMap<String, LearnedAlarmEndpoint>,
}

impl LearnedAlarmSubscriptions {
    pub fn upsert(&mut self, endpoint: LearnedAlarmEndpoint) {
        if !self.entries.contains_key(&endpoint.id)
            && self.entries.len() >= MAX_LEARNED_ALARM_SUBSCRIPTIONS
        {
            if let Some(oldest_id) = self
                .entries
                .values()
                .min_by_key(|candidate| candidate.learned_at_ms)
                .map(|candidate| candidate.id.clone())
            {
                self.entries.remove(&oldest_id);
            }
        }
        self.entries.insert(endpoint.id.clone(), endpoint);
    }

    pub fn entries(&self) -> Vec<LearnedAlarmEndpoint> {
        let mut entries = self.entries.values().cloned().collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            right
                .learned_at_ms
                .cmp(&left.learned_at_ms)
                .then_with(|| left.id.cmp(&right.id))
        });
        entries
    }

    pub fn active_entries(&self, timestamp_ms: u64) -> Vec<LearnedAlarmEndpoint> {
        self.entries()
            .into_iter()
            .filter(|endpoint| !endpoint.is_expired_at(timestamp_ms))
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<&LearnedAlarmEndpoint> {
        self.entries.get(id)
    }
}

const ALARM_TYPES_SCHEMA_VERSION: u32 = 1;
const DEFAULT_CHANNEL_ID: &str = "1";
const DEFAULT_ALARM_REQUEST_TIMEOUT_MS: u64 = 10_000;
const MAX_ALARM_RESPONSE_INSPECTION_BYTES: usize = 64 * 1024;
const RECENTLY_STOPPED_ALARM_JOB_CAPACITY: usize = 256;
const MAX_LEARNED_ALARM_SUBSCRIPTIONS: usize = 16;
const LEARNED_DESTINATION_PREFIX: &str = "subscription:";
const NUMERIC_TIMESTAMP_SENTINEL: &str = "__FST_NUMERIC_TIMESTAMP__";
const NUMERIC_EVENT_ID_SENTINEL: &str = "__FST_NUMERIC_EVENT_ID__";
const NUMERIC_RELATED_ID_SENTINEL: &str = "__FST_NUMERIC_RELATED_ID__";
const NUMERIC_PERSON_ID_SENTINEL: &str = "__FST_NUMERIC_PERSON_ID__";
const NUMERIC_CHANNEL_SENTINEL: &str = "__FST_NUMERIC_CHANNEL__";
const NUMERIC_IMAGE_SIZE_SENTINEL: &str = "__FST_NUMERIC_IMAGE_SIZE__";
const NUMERIC_IMAGE_SIZE_2_SENTINEL: &str = "__FST_NUMERIC_IMAGE_SIZE_2__";
const NUMERIC_IMAGE_SIZE_3_SENTINEL: &str = "__FST_NUMERIC_IMAGE_SIZE_3__";
const NUMERIC_IMAGE_SIZE_4_SENTINEL: &str = "__FST_NUMERIC_IMAGE_SIZE_4__";
const NUMERIC_IMAGE_SIZE_5_SENTINEL: &str = "__FST_NUMERIC_IMAGE_SIZE_5__";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmRuntimeError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for AlarmRuntimeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AlarmRuntimeError {}

#[derive(Debug, Clone)]
pub struct AlarmRuntimeConfig {
    pub platform: TargetPlatform,
    pub target: TargetPlatformConfig,
    pub preview: DevicePreview,
    pub device_http_port: u16,
    pub assets: Arc<RuntimeAssetLayout>,
    pub app_data_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AlarmTypesManifestV1 {
    schema_version: u32,
    profile_id: String,
    handler_id: String,
    definitions: Vec<RuntimeAlarmTypeDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeAlarmTypeDefinition {
    id: String,
    display_name: String,
    platforms: Vec<TargetPlatform>,
    protocol: String,
    event_type: String,
    alarm_template: Option<String>,
    structure_template: Option<String>,
    structure_path: Option<String>,
    image_root: Option<String>,
    supports_pictures: bool,
    recovery_event_type: Option<String>,
    source_type: Option<String>,
    evidence: RuntimeAlarmEvidence,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeAlarmEvidence {
    status: String,
    source: String,
    line: usize,
}

#[derive(Debug, Clone)]
struct RuntimeAlarmDevice {
    preview: DeviceIdentityPreviewDto,
    profile_id: FirstReleaseProfileId,
}

struct ActiveAlarmJob {
    running: RunningAlarmJob,
    tracker: crate::device_simulator::alarms::scheduler::AlarmJobTracker,
}

struct AlarmJobRegistry<T> {
    active: BTreeMap<String, T>,
    recently_stopped: VecDeque<String>,
    recently_stopped_capacity: usize,
}

impl<T> AlarmJobRegistry<T> {
    fn new(recently_stopped_capacity: usize) -> Self {
        Self {
            active: BTreeMap::new(),
            recently_stopped: VecDeque::with_capacity(recently_stopped_capacity),
            recently_stopped_capacity: recently_stopped_capacity.max(1),
        }
    }

    fn insert(&mut self, job_id: String, job: T) {
        self.recently_stopped
            .retain(|stopped_id| stopped_id != &job_id);
        self.active.insert(job_id, job);
    }

    fn take_for_stop(&mut self, job_id: &str) -> Result<Option<T>, AlarmRuntimeError> {
        if self.recently_stopped.iter().any(|known| known == job_id) {
            return Ok(None);
        }
        let Some(job) = self.active.remove(job_id) else {
            // Stopping is a desired-state operation. The Worker may have been
            // restarted or the final telemetry event may have raced the UI, so
            // an already-absent job is successfully stopped as well.
            return Ok(None);
        };
        self.remember_stopped(job_id.to_owned());
        Ok(Some(job))
    }

    fn mark_finished(&mut self, job_id: &str) -> Option<T> {
        let job = self.active.remove(job_id)?;
        self.remember_stopped(job_id.to_owned());
        Some(job)
    }

    fn take_all_for_stop(&mut self) -> Vec<T> {
        let active = std::mem::take(&mut self.active);
        for job_id in active.keys() {
            self.remember_stopped(job_id.clone());
        }
        active.into_values().collect()
    }

    fn remember_stopped(&mut self, job_id: String) {
        if self.recently_stopped.iter().any(|known| known == &job_id) {
            return;
        }
        if self.recently_stopped.len() == self.recently_stopped_capacity {
            self.recently_stopped.pop_front();
        }
        self.recently_stopped.push_back(job_id);
    }
}

pub struct AlarmRuntime {
    scheduler: AlarmScheduler,
    registry: AlarmHandlerRegistry,
    image_cache: SharedImageCache,
    assets: Arc<RuntimeAssetLayout>,
    pack_root: PathBuf,
    user_asset_root: PathBuf,
    image_manifests: Arc<BTreeMap<PackIdentity, PackManifest>>,
    devices: BTreeMap<String, RuntimeAlarmDevice>,
    destinations: BTreeMap<String, reqwest::Url>,
    destination_ids: Vec<String>,
    device_http_port: u16,
    platform: TargetPlatform,
    learned_subscriptions: SharedLearnedAlarmSubscriptions,
    allow_learned_endpoint: bool,
    jobs: Mutex<AlarmJobRegistry<ActiveAlarmJob>>,
}

impl AlarmRuntime {
    pub fn new(config: AlarmRuntimeConfig) -> Result<Self, AlarmRuntimeError> {
        if config.preview.devices.is_empty() {
            return Err(runtime_error(
                "device_simulator.alarm.preview_empty",
                "alarm runtime requires at least one simulated device",
            ));
        }
        let (destinations, destination_ids, allow_learned_endpoint) =
            build_destinations(&config.target)?;
        let learned_subscriptions: SharedLearnedAlarmSubscriptions =
            Arc::new(RwLock::new(LearnedAlarmSubscriptions::default()));
        let sender = Arc::new(HttpAlarmSender::new(
            destinations.clone(),
            Arc::clone(&learned_subscriptions),
            allow_learned_endpoint,
        ));
        let scheduler = AlarmScheduler::new(
            sender,
            Arc::new(SystemAlarmClock::default()) as Arc<dyn AlarmClock>,
            AlarmSchedulerLimits {
                request_timeout_ms: DEFAULT_ALARM_REQUEST_TIMEOUT_MS,
                ..AlarmSchedulerLimits::default()
            },
        )
        .map_err(|source| runtime_error(source.code, source.message))?;

        let selected_profiles = config
            .preview
            .devices
            .iter()
            .map(|device| parse_profile_id(&device.profile_id))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let mut registry = AlarmHandlerRegistry::default();
        for profile_id in selected_profiles {
            register_profile_definitions(
                &mut registry,
                &config.assets,
                profile_id,
                config.platform,
            )?;
        }
        if registry.is_empty() {
            return Err(runtime_error(
                "device_simulator.alarm.registry_empty",
                "no approved alarm definitions match the selected profiles and platform",
            ));
        }

        let (pack_root, manifests) = image_manifest_index(&config.assets)?;
        let user_asset_root = config
            .app_data_dir
            .join("device-simulator")
            .join("user-alarm-images");
        let mut image_references = registry.image_references();
        image_references.extend(all_declared_pack_images(&manifests));
        image_references.extend(config.assets.local_alarm_image_references());
        let image_cache = ImageCache::declare_at_start(
            image_references,
            &pack_root,
            &user_asset_root,
            &manifests,
        )
        .map_err(|source| runtime_error(source.code, source.message))?;
        let image_manifests = Arc::new(manifests);

        let mut devices = BTreeMap::new();
        for device in config.preview.devices {
            let profile_id = parse_profile_id(&device.profile_id)?;
            let profile = config.assets.profile(profile_id).ok_or_else(|| {
                runtime_error(
                    "device_simulator.alarm.profile_missing",
                    format!("runtime profile '{}' is not loaded", device.profile_id),
                )
            })?;
            profile.handlers.alarms.first().ok_or_else(|| {
                runtime_error(
                    "device_simulator.alarm.handler_missing",
                    format!("profile '{}' has no alarm handler", device.profile_id),
                )
            })?;
            devices.insert(
                device.device_id.clone(),
                RuntimeAlarmDevice {
                    preview: device,
                    profile_id,
                },
            );
        }

        Ok(Self {
            scheduler,
            registry,
            image_cache: Arc::new(parking_lot::RwLock::new(image_cache)),
            assets: config.assets,
            pack_root,
            user_asset_root,
            image_manifests,
            devices,
            destinations,
            destination_ids,
            device_http_port: config.device_http_port,
            platform: config.platform,
            learned_subscriptions,
            allow_learned_endpoint,
            jobs: Mutex::new(AlarmJobRegistry::new(RECENTLY_STOPPED_ALARM_JOB_CAPACITY)),
        })
    }

    pub fn image_cache(&self) -> SharedImageCache {
        Arc::clone(&self.image_cache)
    }

    /// Shared handle the HTTP runtime updates when the platform advertises its
    /// alarm receiver endpoint during subscription, so dispatch and the rendered
    /// `Reference` follow it. See [`SharedLearnedAlarmSubscriptions`].
    pub fn learned_subscriptions_handle(&self) -> SharedLearnedAlarmSubscriptions {
        Arc::clone(&self.learned_subscriptions)
    }

    /// Where alarms are currently dispatched, resolved exactly like a real
    /// send. Surfaced to the UI so the effective destination is visible without
    /// having to trigger an alarm and read the failure back.
    pub fn effective_destinations(&self) -> Vec<String> {
        let active = self.active_learned_subscriptions();
        let automatic = (active.len() == 1).then(|| &active[0]);
        self.destination_ids
            .iter()
            .filter_map(|id| self.destinations.get(id))
            .map(|base| {
                apply_learned_endpoint(base, automatic, self.allow_learned_endpoint).to_string()
            })
            .collect()
    }

    pub fn learned_subscriptions(&self) -> Vec<LearnedAlarmEndpoint> {
        self.learned_subscriptions.read().entries()
    }

    pub fn active_learned_subscriptions(&self) -> Vec<LearnedAlarmEndpoint> {
        self.learned_subscriptions.read().active_entries(now_ms())
    }

    /// Legacy single-subscription summary. Multiple active subscriptions never
    /// choose a winner implicitly.
    pub fn learned_subscription(&self) -> Option<LearnedAlarmEndpoint> {
        let active = self.active_learned_subscriptions();
        (active.len() == 1).then(|| active[0].clone())
    }

    pub fn subscription_selection_required(&self) -> bool {
        self.allow_learned_endpoint && self.active_learned_subscriptions().len() > 1
    }

    /// `false` when an explicit receiver URL pins the destination, which
    /// deliberately suppresses subscription learning.
    pub fn follows_platform_subscription(&self) -> bool {
        self.allow_learned_endpoint
    }

    fn resolve_subscription_destination_id(
        &self,
        requested_id: Option<&str>,
    ) -> Result<Option<String>, AlarmRuntimeError> {
        if !self.allow_learned_endpoint {
            if requested_id.is_some() {
                return Err(runtime_error(
                    "device_simulator.alarm.subscription_override_active",
                    "an explicit alarm receiver URL is configured; learned subscriptions cannot be selected",
                ));
            }
            return Ok(None);
        }

        let timestamp_ms = now_ms();
        let learned = self.learned_subscriptions.read();
        if let Some(id) = requested_id {
            let endpoint = learned.get(id).ok_or_else(|| {
                runtime_error(
                    "device_simulator.alarm.subscription_unknown",
                    format!("alarm subscription '{id}' is not available in this session"),
                )
            })?;
            if endpoint.is_expired_at(timestamp_ms) {
                return Err(runtime_error(
                    "device_simulator.alarm.subscription_expired",
                    format!("alarm subscription '{id}' has expired"),
                ));
            }
            return Ok(Some(format!("{LEARNED_DESTINATION_PREFIX}{id}")));
        }

        let active = learned.active_entries(timestamp_ms);
        match active.as_slice() {
            [] => Ok(None),
            [endpoint] => Ok(Some(format!(
                "{LEARNED_DESTINATION_PREFIX}{}",
                endpoint.id
            ))),
            _ => Err(runtime_error(
                "device_simulator.alarm.destination_selection_required",
                format!(
                    "{} active platform subscriptions are available; select the intended alarm receiver",
                    active.len()
                ),
            )),
        }
    }

    pub async fn trigger_once(
        &self,
        request: AlarmJobRequest,
    ) -> Result<AlarmTriggerResult, AlarmRuntimeError> {
        let started = Instant::now();
        let job_id = format!("alarm-{}", uuid::Uuid::new_v4().simple());
        let targets = self.build_targets(&request, &job_id).await?;
        let snapshot = self
            .scheduler
            .trigger_once(OneShotAlarmJob {
                job_id,
                targets,
                mode: one_shot_mode(request.mode, request.alarm_type_ids.len())?,
                recovery_delay_ms: request
                    .recovery_delay_secs
                    .map(|seconds| seconds.saturating_mul(1_000)),
                random_seed: random_seed(),
            })
            .await
            .map_err(|source| runtime_error(source.code, source.message))?;
        Ok(trigger_result(
            snapshot,
            started.elapsed().as_millis() as u64,
        ))
    }

    pub async fn start_job(&self, request: AlarmJobRequest) -> Result<String, AlarmRuntimeError> {
        let job_id = format!("alarm-{}", uuid::Uuid::new_v4().simple());
        let targets = self.build_targets(&request, &job_id).await?;
        let running = self
            .scheduler
            .start_periodic(PeriodicAlarmJob {
                job_id: job_id.clone(),
                targets,
                mode: scheduled_mode(request.mode, request.alarm_type_ids.len())?,
                interval_ms: request.interval_ms,
                send_count: request.send_count,
                recovery_delay_ms: request
                    .recovery_delay_secs
                    .map(|seconds| seconds.saturating_mul(1_000)),
                random_seed: random_seed(),
            })
            .map_err(|source| runtime_error(source.code, source.message))?;
        let tracker = running.tracker();
        let mut jobs = self.jobs.lock().await;
        jobs.insert(job_id.clone(), ActiveAlarmJob { running, tracker });
        Ok(job_id)
    }

    pub async fn stop_job(&self, job_id: &str) -> Result<(), AlarmRuntimeError> {
        let Some(active) = self.jobs.lock().await.take_for_stop(job_id)? else {
            return Ok(());
        };
        active
            .running
            .stop_and_wait()
            .await
            .map(|_| ())
            .map_err(|source| runtime_error(source.code, source.message))
    }

    pub async fn stop_all(&self) -> Result<(), AlarmRuntimeError> {
        let jobs = {
            let mut active = self.jobs.lock().await;
            active.take_all_for_stop()
        };
        let mut first_error = None;
        for job in jobs {
            if let Err(source) = job.running.stop_and_wait().await {
                first_error.get_or_insert_with(|| runtime_error(source.code, source.message));
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub async fn stats(&self) -> Vec<AlarmJobStatsSnapshot> {
        let trackers = self
            .jobs
            .lock()
            .await
            .active
            .iter()
            .map(|(job_id, job)| (job_id.clone(), job.tracker.clone()))
            .collect::<Vec<_>>();
        let mut snapshots = Vec::with_capacity(trackers.len());
        let mut finished = Vec::new();
        for (job_id, tracker) in trackers {
            let snapshot = tracker.snapshot().await;
            if matches!(
                snapshot.state,
                ScheduledAlarmJobState::Completed
                    | ScheduledAlarmJobState::Cancelled
                    | ScheduledAlarmJobState::Failed
            ) {
                finished.push(job_id);
            }
            snapshots.push(job_stats_snapshot(snapshot));
        }
        if !finished.is_empty() {
            let mut jobs = self.jobs.lock().await;
            for job_id in finished {
                jobs.mark_finished(&job_id);
            }
        }
        snapshots
    }

    pub async fn active_job_count(&self) -> u32 {
        self.jobs
            .lock()
            .await
            .active
            .len()
            .try_into()
            .unwrap_or(u32::MAX)
    }

    async fn build_targets(
        &self,
        request: &AlarmJobRequest,
        job_id: &str,
    ) -> Result<Vec<AlarmDeviceTarget>, AlarmRuntimeError> {
        let image_variant = validate_image_variant(request.image_variant.as_deref())?;
        let user_image_id = request.user_image_id.as_deref();
        if image_variant.is_some() && user_image_id.is_some() {
            return Err(runtime_error(
                "device_simulator.alarm.image_source_conflict",
                "an official image size variant and a custom user image cannot be selected together",
            ));
        }
        let (user_image, job_image_cache) = self.load_job_image(user_image_id).await?;
        if self.destination_ids.is_empty() {
            return Err(runtime_error(
                "device_simulator.alarm.destination_missing",
                "configure an alarm receiver URL or at least one platform server",
            ));
        }
        let selected_subscription_destination =
            self.resolve_subscription_destination_id(request.target_subscription_id.as_deref())?;
        let selected_ids = if request.target_device_ids.is_empty() {
            self.devices.keys().cloned().collect::<Vec<_>>()
        } else {
            request.target_device_ids.clone()
        };
        let requested_types = request
            .alarm_type_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let mut targets = Vec::with_capacity(selected_ids.len());
        let mut user_image_applied = false;
        for (index, device_id) in selected_ids.into_iter().enumerate() {
            let device = self.devices.get(&device_id).ok_or_else(|| {
                runtime_error(
                    "device_simulator.alarm.device_unknown",
                    format!("alarm target device '{device_id}' does not exist"),
                )
            })?;
            if request.alarm_profile_id != "default"
                && request.alarm_profile_id != device.profile_id.as_str()
            {
                return Err(runtime_error(
                    "device_simulator.alarm.profile_mismatch",
                    format!(
                        "device '{}' uses profile '{}', not '{}'",
                        device_id,
                        device.profile_id.as_str(),
                        request.alarm_profile_id
                    ),
                ));
            }
            let mut definitions = self
                .registry
                .definitions()
                .filter(|definition| {
                    definition.profile_id == device.profile_id
                        && (requested_types.is_empty()
                            || requested_types.contains(definition.alarm_type_id.as_str()))
                })
                .cloned()
                .collect::<Vec<_>>();
            definitions.sort_by(|left, right| {
                left.alarm_type_id
                    .as_str()
                    .cmp(right.alarm_type_id.as_str())
            });
            if definitions.is_empty() {
                return Err(runtime_error(
                    "device_simulator.alarm.type_unknown",
                    format!("no requested alarm type is available for device '{device_id}'"),
                ));
            }
            if request.mode == AlarmDispatchMode::Configured && definitions.len() != 1 {
                return Err(runtime_error(
                    "device_simulator.alarm.configured_type_count_invalid",
                    "configured mode requires exactly one alarm type",
                ));
            }
            let subscription_id = stable_numeric_id(job_id, &device_id);
            let destination_id = selected_subscription_destination
                .clone()
                .unwrap_or_else(|| {
                    self.destination_ids[index % self.destination_ids.len()].clone()
                });
            // Fail here rather than at dispatch time if the job names a
            // destination that was never configured.
            if !destination_id.starts_with(LEARNED_DESTINATION_PREFIX)
                && !self.destinations.contains_key(&destination_id)
            {
                return Err(runtime_error(
                    "device_simulator.alarm.destination_missing",
                    format!("alarm destination '{destination_id}' is not configured"),
                ));
            }
            let mut invocations = Vec::with_capacity(definitions.len());
            for mut definition in definitions {
                if let Some(user_image) = &user_image {
                    if apply_user_image(&mut definition, user_image) {
                        user_image_applied = true;
                    }
                } else {
                    apply_image_variant(&mut definition, image_variant, &self.assets)?;
                }
                let context = build_context(device, &subscription_id, self.device_http_port)?;
                invocations.push(AlarmInvocation {
                    definition: Arc::new(definition),
                    context,
                    image_cache: Arc::clone(&job_image_cache),
                });
            }
            targets.push(AlarmDeviceTarget {
                device_id: device_id.clone(),
                destination_id,
                platform: self.platform,
                invocations,
            });
        }
        if user_image.is_some() && !user_image_applied {
            return Err(runtime_error(
                "device_simulator.alarm.user_image_unsupported",
                "none of the selected alarm definitions supports an image attachment",
            ));
        }
        Ok(targets)
    }

    async fn load_job_image(
        &self,
        image_id: Option<&str>,
    ) -> Result<(Option<ImageAssetRef>, SharedImageCache), AlarmRuntimeError> {
        let Some(image_id) = image_id else {
            return Ok((None, Arc::clone(&self.image_cache)));
        };
        let image_id = image_id.to_owned();
        let pack_root = self.pack_root.clone();
        let user_asset_root = self.user_asset_root.clone();
        let manifests = Arc::clone(&self.image_manifests);
        let (reference, additional) = tokio::task::spawn_blocking(move || {
            load_user_alarm_image(&image_id, &pack_root, &user_asset_root, manifests.as_ref())
        })
        .await
        .map_err(|source| {
            runtime_error(
                "device_simulator.alarm.user_image_task_failed",
                format!("failed to join user image validation task: {source}"),
            )
        })??;
        let mut cache = self.image_cache.write();
        let merged = cache
            .merged(additional)
            .map_err(|source| runtime_error(source.code, source.message))?;
        *cache = merged;
        drop(cache);
        Ok((Some(reference), Arc::clone(&self.image_cache)))
    }
}

#[derive(Debug)]
struct HttpAlarmSender {
    destinations: BTreeMap<String, reqwest::Url>,
    learned_subscriptions: SharedLearnedAlarmSubscriptions,
    allow_learned_endpoint: bool,
    clients: StdMutex<HashMap<Ipv4Addr, reqwest::Client>>,
}

impl HttpAlarmSender {
    fn new(
        destinations: BTreeMap<String, reqwest::Url>,
        learned_subscriptions: SharedLearnedAlarmSubscriptions,
        allow_learned_endpoint: bool,
    ) -> Self {
        Self {
            destinations,
            learned_subscriptions,
            allow_learned_endpoint,
            clients: StdMutex::new(HashMap::new()),
        }
    }

    fn client(&self, source_ip: Ipv4Addr) -> Result<reqwest::Client, AlarmSendError> {
        if let Some(client) = self
            .clients
            .lock()
            .map_err(|_| {
                AlarmSendError::new("device_simulator.alarm.client_cache_poisoned", false)
            })?
            .get(&source_ip)
            .cloned()
        {
            return Ok(client);
        }
        let client = reqwest::Client::builder()
            .local_address(IpAddr::V4(source_ip))
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(10))
            // Legacy HTTPConnection reports the receiver's first response and
            // never follows redirects. Following a 302 can turn the alarm POST
            // into a GET to a login/general page and misreport that final 200 as
            // an alarm acknowledgement.
            .redirect(reqwest::redirect::Policy::none())
            // The legacy sender creates a fresh TCP connection for every alarm.
            // Keep the per-source client cache, but do not reuse idle sockets.
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|source| {
                AlarmSendError::new("device_simulator.alarm.http_client_failed", false)
                    .with_details(format!("source {source_ip}: {source}"))
            })?;
        self.clients
            .lock()
            .map_err(|_| {
                AlarmSendError::new("device_simulator.alarm.client_cache_poisoned", false)
            })?
            .insert(source_ip, client.clone());
        Ok(client)
    }
}

impl AlarmSender for HttpAlarmSender {
    fn send(
        &self,
        outbound: OutboundAlarmRequest,
    ) -> AlarmFuture<'_, Result<AlarmSenderResponse, AlarmSendError>> {
        Box::pin(async move {
            let base = if let Some(subscription_id) = outbound
                .destination_id
                .strip_prefix(LEARNED_DESTINATION_PREFIX)
            {
                let learned = self.learned_subscriptions.read();
                let endpoint = learned.get(subscription_id).ok_or_else(|| {
                    AlarmSendError::new("device_simulator.alarm.subscription_unknown", false)
                        .with_details(format!(
                            "alarm subscription '{subscription_id}' is no longer available"
                        ))
                })?;
                if endpoint.is_expired_at(now_ms()) {
                    return Err(AlarmSendError::new(
                        "device_simulator.alarm.subscription_expired",
                        false,
                    )
                    .with_details(format!(
                        "alarm subscription '{subscription_id}' expired before dispatch"
                    )));
                }
                let configured = self.destinations.values().next().ok_or_else(|| {
                    AlarmSendError::new("device_simulator.alarm.destination_unknown", false)
                        .with_details("no configured destination is available")
                })?;
                apply_learned_endpoint(configured, Some(endpoint), self.allow_learned_endpoint)
            } else {
                self.destinations
                    .get(&outbound.destination_id)
                    .cloned()
                    .ok_or_else(|| {
                        AlarmSendError::new("device_simulator.alarm.destination_unknown", false)
                            .with_details(format!(
                                "no destination configured for '{}'",
                                outbound.destination_id
                            ))
                    })?
            };
            let url = base
                .join(outbound.request.path.trim_start_matches('/'))
                .map_err(|source| {
                    AlarmSendError::new("device_simulator.alarm.destination_url_invalid", false)
                        .with_details(format!("{base} + {}: {source}", outbound.request.path))
                })?;
            let endpoint = url.to_string();
            let source_ip = outbound.request.source_ip;
            let client = self.client(source_ip)?;
            let mut builder = match outbound.request.method {
                HttpMethod::Post => client.post(url),
            };
            for (name, value) in &outbound.request.headers {
                let name = reqwest::header::HeaderName::from_str(name).map_err(|_| {
                    AlarmSendError::new("device_simulator.alarm.header_invalid", false)
                        .with_details(format!("invalid header name '{name}'"))
                })?;
                let value = reqwest::header::HeaderValue::from_str(value).map_err(|_| {
                    AlarmSendError::new("device_simulator.alarm.header_invalid", false)
                        .with_details(format!("invalid value for header '{name}'"))
                })?;
                builder = builder.header(name, value);
            }
            let mut response = builder
                .body(outbound.request.body.to_vec())
                .send()
                .await
                .map_err(|source| {
                    AlarmSendError::new(
                        if source.is_timeout() {
                            "device_simulator.alarm.transport_timeout"
                        } else if source.is_connect() {
                            "device_simulator.alarm.transport_connect_failed"
                        } else {
                            "device_simulator.alarm.transport_failed"
                        },
                        source.is_timeout() || source.is_connect(),
                    )
                    // The endpoint and the OS-level reason are the two facts that
                    // actually identify the fault; without them the UI can only
                    // say "sending failed".
                    .with_details(format!(
                        "{source_ip} -> {endpoint}: {}",
                        transport_reason(&source)
                    ))
                })?;
            let status = response.status().as_u16();
            let application_status = inspect_ums_alarm_response(&mut response).await;
            let mut result = AlarmSenderResponse::new(status).with_endpoint(endpoint);
            if let Some(application_status) = application_status {
                result = result.with_application_status(application_status);
            }
            Ok(result)
        })
    }
}

/// Decode the bounded UMS JSON response envelope. A successful application
/// code is stronger evidence than HTTP 2xx; a non-zero code is a real platform
/// rejection even when the HTTP status itself is 200. Empty and unknown bodies
/// deliberately remain unverified.
async fn inspect_ums_alarm_response(
    response: &mut reqwest::Response,
) -> Option<AlarmApplicationStatus> {
    let mut body = Vec::new();
    loop {
        let Some(chunk) = response.chunk().await.ok()? else {
            break;
        };
        if body.len().saturating_add(chunk.len()) > MAX_ALARM_RESPONSE_INSPECTION_BYTES {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    decode_ums_alarm_response(&body)
}

fn decode_ums_alarm_response(body: &[u8]) -> Option<AlarmApplicationStatus> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return None;
    }
    let document: Value = serde_json::from_slice(body).ok()?;
    let envelope = document.get("Response").unwrap_or(&document);
    let response_code = json_i64(envelope.get("ResponseCode"));
    let status_code = json_i64(envelope.get("StatusCode"));
    if response_code.is_none() && status_code.is_none() {
        return None;
    }
    let sub_response_code = json_i64(envelope.get("SubResponseCode"));
    let accepted = response_code.is_none_or(|code| code == 0)
        && status_code.is_none_or(|code| code == 0)
        && sub_response_code.is_none_or(|code| code == 0);
    if accepted {
        return Some(AlarmApplicationStatus::Accepted);
    }
    let mut parts = Vec::new();
    if let Some(code) = response_code {
        parts.push(format!("ResponseCode={code}"));
    }
    if let Some(code) = sub_response_code {
        parts.push(format!("SubResponseCode={code}"));
    }
    if let Some(code) = status_code {
        parts.push(format!("StatusCode={code}"));
    }
    for key in ["ResponseString", "StatusString"] {
        if let Some(value) = envelope.get(key).and_then(Value::as_str) {
            let value = value.trim();
            if !value.is_empty() {
                parts.push(format!("{key}={}", truncate_public_detail(value, 256)));
            }
        }
    }
    Some(AlarmApplicationStatus::Rejected {
        details: parts.join(", "),
    })
}

fn json_i64(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| number.try_into().ok()))
        .or_else(|| value.as_str()?.trim().parse().ok())
}

fn truncate_public_detail(value: &str, maximum_chars: usize) -> String {
    let mut chars = value.chars();
    let mut truncated = chars.by_ref().take(maximum_chars).collect::<String>();
    if chars.next().is_some() {
        truncated.push('…');
    }
    truncated
}

/// Innermost cause of a transport failure. `reqwest`'s own `Display` stops at
/// "error sending request", which hides the very thing an operator needs (for
/// example "No connection could be made because the target machine actively
/// refused it").
fn transport_reason(error: &reqwest::Error) -> String {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    let mut deepest = error.to_string();
    while let Some(current) = source {
        deepest = current.to_string();
        source = current.source();
    }
    deepest
}

fn register_profile_definitions(
    registry: &mut AlarmHandlerRegistry,
    assets: &RuntimeAssetLayout,
    profile_id: FirstReleaseProfileId,
    platform: TargetPlatform,
) -> Result<(), AlarmRuntimeError> {
    let bytes = assets
        .read_from_pack(profile_id.as_str(), "runtime/alarm-types.json")
        .map_err(|source| runtime_error(source.code, source.message))?;
    let manifest: AlarmTypesManifestV1 = serde_json::from_slice(&bytes).map_err(|source| {
        runtime_error(
            "device_simulator.alarm.manifest_invalid",
            format!(
                "alarm manifest for '{}' is invalid: {source}",
                profile_id.as_str()
            ),
        )
    })?;
    if manifest.schema_version != ALARM_TYPES_SCHEMA_VERSION
        || manifest.profile_id != profile_id.as_str()
    {
        return Err(runtime_error(
            "device_simulator.alarm.manifest_identity_mismatch",
            format!(
                "alarm manifest for '{}' has the wrong identity",
                profile_id.as_str()
            ),
        ));
    }
    let handler_id = AlarmHandlerId::from_str(&manifest.handler_id)
        .map_err(|source| runtime_error(source.code, source.message))?;
    if handler_id.profile_id() != profile_id {
        return Err(runtime_error(
            "device_simulator.alarm.handler_profile_mismatch",
            "alarm manifest handler does not match its profile",
        ));
    }
    let mut registered = 0;
    for definition in manifest
        .definitions
        .into_iter()
        .filter(|definition| definition.platforms.contains(&platform))
    {
        let compiled =
            compile_runtime_definition(assets, profile_id, handler_id, platform, definition)?;
        registry
            .register(compiled)
            .map_err(|source| runtime_error(source.code, source.message))?;
        registered += 1;
    }
    if registered == 0 {
        return Err(runtime_error(
            "device_simulator.alarm.platform_definition_missing",
            format!(
                "profile '{}' has no alarm definitions for {platform:?}",
                profile_id.as_str()
            ),
        ));
    }
    Ok(())
}

fn compile_runtime_definition(
    assets: &RuntimeAssetLayout,
    profile_id: FirstReleaseProfileId,
    handler_id: AlarmHandlerId,
    _platform: TargetPlatform,
    definition: RuntimeAlarmTypeDefinition,
) -> Result<AlarmHandlerDefinition, AlarmRuntimeError> {
    if definition.evidence.status != "reviewed_static"
        || definition.evidence.source != "data/alarms_info.yml"
        || definition.evidence.line == 0
        || definition.display_name.trim().is_empty()
        || definition.event_type.trim().is_empty()
    {
        return Err(runtime_error(
            "device_simulator.alarm.evidence_invalid",
            format!(
                "alarm definition '{}' has invalid static evidence",
                definition.id
            ),
        ));
    }
    let multipart = definition.protocol == "v1_1";
    if !multipart && definition.protocol != "v1_0" {
        return Err(runtime_error(
            "device_simulator.alarm.protocol_unsupported",
            format!(
                "alarm definition '{}' uses an unsupported protocol",
                definition.id
            ),
        ));
    }
    let selected_template = definition
        .structure_template
        .as_ref()
        .or(definition.alarm_template.as_ref())
        .ok_or_else(|| {
            runtime_error(
                "device_simulator.alarm.template_missing",
                format!(
                    "alarm definition '{}' has no approved template",
                    definition.id
                ),
            )
        })?;
    let selected_is_structure = definition
        .structure_template
        .as_ref()
        .is_some_and(|path| path == selected_template);
    let embedded_image_limit = if definition.supports_pictures && !multipart {
        usize::MAX
    } else {
        0
    };
    let template_bytes = assets
        .read_from_pack(
            profile_id.as_str(),
            &normalize_runtime_path(selected_template),
        )
        .map_err(|source| runtime_error(source.code, source.message))?;
    let event_type_override =
        (definition.event_type != definition.id).then_some(definition.event_type.as_str());
    let template = compile_json_template_with_options(
        &template_bytes,
        event_type_override,
        embedded_image_limit,
        false,
        false,
    )?;
    let image_count = if !definition.supports_pictures {
        0
    } else if multipart {
        1
    } else {
        embedded_image_count(&template)
    };
    let mut image_reference_groups = select_pack_image_groups(
        assets,
        profile_id,
        definition.image_root.as_deref(),
        image_count,
        &definition.id,
    )?;
    let images = build_image_attachments(
        image_reference_groups.drain(..1).next().unwrap_or_default(),
        multipart,
        image_count,
    );
    let alternate_images = image_reference_groups
        .into_iter()
        .map(|group| build_image_attachments(group, multipart, image_count))
        .collect::<Vec<_>>();
    let image_policy = if images.is_empty() {
        ImagePolicy::Forbidden
    } else {
        ImagePolicy::Required
    };
    let path = if multipart {
        "/LAPI/V1.1/System/Event/Notification".to_owned()
    } else if selected_is_structure {
        definition
            .structure_path
            .clone()
            .unwrap_or_else(|| "/LAPI/V1.0/System/Event/Notification/Structure".to_owned())
    } else {
        "/LAPI/V1.0/System/Event/Notification/Alarm".to_owned()
    };
    let recovery = if let (Some(recovery_event), Some(alarm_template)) = (
        definition.recovery_event_type.as_deref(),
        definition.alarm_template.as_deref(),
    ) {
        let bytes = assets
            .read_from_pack(profile_id.as_str(), &normalize_runtime_path(alarm_template))
            .map_err(|source| runtime_error(source.code, source.message))?;
        RecoveryDefinition::RenderWith {
            template: compile_json_template(&bytes, Some(recovery_event), 0)?,
            transport: TransportDefinition {
                method: HttpMethod::Post,
                path: "/LAPI/V1.0/System/Event/Notification/Alarm".into(),
                source_binding: SourceBinding::DeviceIp,
                body_encoding: BodyEncoding::Raw {
                    content_type: "application/json; charset=utf-8".into(),
                },
                success_rule: ResponseSuccessRule::Unverified,
            },
            trigger: RecoveryTrigger::RequestedDelay,
            include_images: false,
        }
    } else {
        RecoveryDefinition::None
    };
    let follow_up_requests = if selected_is_structure && !multipart {
        if let Some(alarm_template) = definition.alarm_template.as_deref() {
            let bytes = assets
                .read_from_pack(profile_id.as_str(), &normalize_runtime_path(alarm_template))
                .map_err(|source| runtime_error(source.code, source.message))?;
            vec![AlarmRequestDefinition {
                template: compile_json_template(&bytes, event_type_override, 0)?,
                image_policy: ImagePolicy::Forbidden,
                images: vec![],
                transport: TransportDefinition {
                    method: HttpMethod::Post,
                    path: "/LAPI/V1.0/System/Event/Notification/Alarm".into(),
                    source_binding: SourceBinding::DeviceIp,
                    body_encoding: BodyEncoding::Raw {
                        content_type: "application/json; charset=utf-8".into(),
                    },
                    success_rule: ResponseSuccessRule::Unverified,
                },
            }]
        } else {
            vec![]
        }
    } else {
        vec![]
    };
    let mut intentional_changes =
        vec!["HTTP response success remains unverified until real-platform acceptance".into()];
    if let Some(source_type) = &definition.source_type {
        intentional_changes.push(format!("legacy source type: {source_type}"));
    }
    let mut legacy_sources = vec![
        definition.evidence.source.clone(),
        selected_template.clone(),
    ];
    if let Some(alarm_template) = definition.alarm_template.as_ref() {
        if !legacy_sources.contains(alarm_template) {
            legacy_sources.push(alarm_template.clone());
        }
    }
    Ok(AlarmHandlerDefinition {
        handler_id,
        alarm_type_id: AlarmTypeId::new(definition.id)
            .map_err(|source| runtime_error(source.code, source.message))?,
        profile_id,
        template,
        image_policy,
        images,
        alternate_images,
        transport: TransportDefinition {
            method: HttpMethod::Post,
            path,
            source_binding: SourceBinding::DeviceIp,
            body_encoding: if multipart {
                BodyEncoding::Multipart {
                    metadata_name: sanitize_multipart_name(&definition.event_type),
                    metadata_content_type: "text/plain".into(),
                }
            } else {
                BodyEncoding::Raw {
                    content_type: "application/json; charset=utf-8".into(),
                }
            },
            success_rule: ResponseSuccessRule::Unverified,
        },
        follow_up_requests,
        recovery,
        evidence: HandlerEvidence {
            legacy_sources,
            template_source: format!(
                "{}:{} reviewed static fixture",
                profile_id.as_str(),
                selected_template
            ),
            fixture_provenance: FixtureProvenance::LegacyOrCaptureDerived,
            platforms: definition
                .platforms
                .into_iter()
                .map(|platform| PlatformEvidence {
                    platform,
                    verification: PlatformVerification::SourceConfirmedPlatformUnverified,
                })
                .collect(),
            intentional_changes,
        },
    })
}

fn compile_json_template(
    bytes: &[u8],
    event_type: Option<&str>,
    embedded_image_limit: usize,
) -> Result<CompiledTemplate, AlarmRuntimeError> {
    compile_json_template_with_options(bytes, event_type, embedded_image_limit, false, false)
}

fn compile_json_template_with_options(
    bytes: &[u8],
    event_type: Option<&str>,
    embedded_image_limit: usize,
    include_empty_image_slots: bool,
    allow_missing_event_type: bool,
) -> Result<CompiledTemplate, AlarmRuntimeError> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|source| {
        runtime_error(
            "device_simulator.alarm.template_json_invalid",
            format!("approved alarm template is not valid JSON: {source}"),
        )
    })?;
    let mut image_fields = ImageFieldCounters::default();
    rewrite_event_type(&mut value, event_type, allow_missing_event_type)?;
    rewrite_json_value(
        None,
        &mut value,
        embedded_image_limit,
        include_empty_image_slots,
        &mut image_fields,
    )?;
    let mut rendered = serde_json::to_string(&value).map_err(|source| {
        runtime_error(
            "device_simulator.alarm.template_serialize_failed",
            format!("approved alarm template could not be normalized: {source}"),
        )
    })?;
    for (sentinel, marker) in [
        (NUMERIC_TIMESTAMP_SENTINEL, "{{timestamp}}"),
        (NUMERIC_EVENT_ID_SENTINEL, "{{event_id}}"),
        (NUMERIC_RELATED_ID_SENTINEL, "{{related_id}}"),
        (NUMERIC_PERSON_ID_SENTINEL, "{{person_id}}"),
        (NUMERIC_CHANNEL_SENTINEL, "{{channel_id}}"),
        (NUMERIC_IMAGE_SIZE_SENTINEL, "{{image_size}}"),
        (NUMERIC_IMAGE_SIZE_2_SENTINEL, "{{image_size_2}}"),
        (NUMERIC_IMAGE_SIZE_3_SENTINEL, "{{image_size_3}}"),
        (NUMERIC_IMAGE_SIZE_4_SENTINEL, "{{image_size_4}}"),
        (NUMERIC_IMAGE_SIZE_5_SENTINEL, "{{image_size_5}}"),
    ] {
        rendered = rendered.replace(&format!("\"{sentinel}\""), marker);
    }
    CompiledTemplate::compile(rendered.as_bytes())
        .map_err(|source| runtime_error(source.code, source.message))
}

#[derive(Debug, Default)]
struct ImageFieldCounters {
    data: usize,
    size: usize,
    url: usize,
}

fn rewrite_event_type(
    value: &mut Value,
    event_type: Option<&str>,
    allow_missing: bool,
) -> Result<(), AlarmRuntimeError> {
    let Some(event_type) = event_type else {
        return Ok(());
    };
    let mut rewritten = false;
    for pointer in ["/EventInfo/Type", "/AlarmInfo/AlarmType", "/AlarmType"] {
        let Some(slot) = value.pointer_mut(pointer) else {
            continue;
        };
        if !slot.is_string() {
            return Err(runtime_error(
                "device_simulator.alarm.event_type_field_invalid",
                format!("approved alarm template field '{pointer}' must be a string"),
            ));
        }
        *slot = Value::String(event_type.to_owned());
        rewritten = true;
    }
    if !rewritten && allow_missing {
        return Ok(());
    }
    if !rewritten {
        return Err(runtime_error(
            "device_simulator.alarm.event_type_field_missing",
            "approved alarm template has no supported event type field",
        ));
    }
    Ok(())
}

fn rewrite_json_value(
    key: Option<&str>,
    value: &mut Value,
    embedded_image_limit: usize,
    include_empty_image_slots: bool,
    image_fields: &mut ImageFieldCounters,
) -> Result<(), AlarmRuntimeError> {
    match value {
        Value::Object(map) => {
            if image_fields.data < embedded_image_limit
                && key != Some("VehicleImage")
                && map.get("Data").is_some_and(Value::is_string)
                && map.get("Size").is_some_and(Value::is_number)
                && (include_empty_image_slots || image_slot_is_present(map))
            {
                let slot = image_fields.data;
                if let Some(data) = map.get_mut("Data") {
                    *data = Value::String(image_base64_marker(slot)?.into());
                    image_fields.data += 1;
                }
                if let Some(size) = map.get_mut("Size") {
                    *size = Value::String(image_size_sentinel(slot)?.into());
                    image_fields.size += 1;
                }
                if let Some(url) = map.get_mut("URL") {
                    if let Some(picture_type) = url
                        .as_str()
                        .and_then(picture_type_from_url)
                        .map(str::to_owned)
                    {
                        *url = Value::String(format!(
                            "/LAPI/V1.0/System/Picture?Type={picture_type}&Index={}&Size={}",
                            image_index_marker(slot)?,
                            image_size_marker(slot)?
                        ));
                        image_fields.url += 1;
                    }
                }
                if let Some(capture_time) = map.get_mut("CaptureTime") {
                    *capture_time = if capture_time.is_number() {
                        Value::String(NUMERIC_TIMESTAMP_SENTINEL.into())
                    } else {
                        Value::String("{{capture_time}}".into())
                    };
                }
            }
            for (child_key, child) in map.iter_mut() {
                rewrite_json_value(
                    Some(child_key),
                    child,
                    embedded_image_limit,
                    include_empty_image_slots,
                    image_fields,
                )?;
            }
        }
        Value::Array(items) => {
            for item in items {
                rewrite_json_value(
                    key,
                    item,
                    embedded_image_limit,
                    include_empty_image_slots,
                    image_fields,
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn image_slot_is_present(map: &serde_json::Map<String, Value>) -> bool {
    map.get("Data")
        .and_then(Value::as_str)
        .is_some_and(|data| !data.is_empty())
        || map
            .get("Size")
            .and_then(Value::as_u64)
            .is_some_and(|size| size > 0)
        || map
            .get("URL")
            .and_then(Value::as_str)
            .is_some_and(|url| !url.is_empty())
}

fn picture_type_from_url(url: &str) -> Option<&str> {
    if !url.contains("/System/Picture") {
        return None;
    }
    url.split_once('?')?
        .1
        .split('&')
        .find_map(|part| part.strip_prefix("Type="))
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
}

fn image_base64_marker(index: usize) -> Result<&'static str, AlarmRuntimeError> {
    [
        "{{image_base64}}",
        "{{image_base64_2}}",
        "{{image_base64_3}}",
        "{{image_base64_4}}",
        "{{image_base64_5}}",
    ]
    .get(index)
    .copied()
    .ok_or_else(|| image_slot_error(index))
}

fn image_size_marker(index: usize) -> Result<&'static str, AlarmRuntimeError> {
    [
        "{{image_size}}",
        "{{image_size_2}}",
        "{{image_size_3}}",
        "{{image_size_4}}",
        "{{image_size_5}}",
    ]
    .get(index)
    .copied()
    .ok_or_else(|| image_slot_error(index))
}

fn image_size_sentinel(index: usize) -> Result<&'static str, AlarmRuntimeError> {
    [
        NUMERIC_IMAGE_SIZE_SENTINEL,
        NUMERIC_IMAGE_SIZE_2_SENTINEL,
        NUMERIC_IMAGE_SIZE_3_SENTINEL,
        NUMERIC_IMAGE_SIZE_4_SENTINEL,
        NUMERIC_IMAGE_SIZE_5_SENTINEL,
    ]
    .get(index)
    .copied()
    .ok_or_else(|| image_slot_error(index))
}

fn image_index_marker(index: usize) -> Result<&'static str, AlarmRuntimeError> {
    [
        "{{image_index}}",
        "{{image_index_2}}",
        "{{image_index_3}}",
        "{{image_index_4}}",
        "{{image_index_5}}",
    ]
    .get(index)
    .copied()
    .ok_or_else(|| image_slot_error(index))
}

fn image_slot_error(index: usize) -> AlarmRuntimeError {
    runtime_error(
        "device_simulator.alarm.image_slot_exceeded",
        format!("approved alarm template declares more than five image slots ({index})"),
    )
}

struct SelectedPackImage {
    reference: ImageAssetRef,
    url_reference: Option<ImageAssetRef>,
}

fn build_image_attachments(
    selected: Vec<SelectedPackImage>,
    multipart: bool,
    image_count: usize,
) -> Vec<ImageAttachmentDefinition> {
    selected
        .into_iter()
        .enumerate()
        .map(|(index, selected)| ImageAttachmentDefinition {
            file_name: image_reference_file_name(&selected.reference),
            reference: selected.reference,
            url_reference: selected.url_reference,
            field_name: if multipart || image_count == 1 {
                "image".into()
            } else {
                format!("image{}", index + 1)
            },
            image_index: multipart.then_some((index + 1) as u16),
        })
        .collect()
}

fn select_pack_image_groups(
    assets: &RuntimeAssetLayout,
    profile_id: FirstReleaseProfileId,
    image_root: Option<&str>,
    count: usize,
    alarm_type_id: &str,
) -> Result<Vec<Vec<SelectedPackImage>>, AlarmRuntimeError> {
    if count == 0 {
        return Ok(vec![vec![]]);
    }
    if let Some(groups) = assets.local_alarm_image_groups(alarm_type_id, "normal") {
        let mut selected_groups = Vec::with_capacity(groups.len());
        for images in groups {
            if images.len() != count {
                return Err(runtime_error(
                    "device_simulator.alarm.image_missing",
                    format!("local alarm material '{alarm_type_id}' contains an incomplete image group: {} images found but {count} are required", images.len()),
                ));
            }
            selected_groups.push(
                images
                    .iter()
                    .cloned()
                    .map(|reference| SelectedPackImage {
                        reference,
                        url_reference: None,
                    })
                    .collect(),
            );
        }
        return Ok(selected_groups);
    }
    let Some(image_root) = image_root else {
        return Err(runtime_error(
            "device_simulator.alarm.image_root_missing",
            "approved pictured alarm has no image root",
        ));
    };
    let image_root = normalize_runtime_path(image_root);
    let mut paths = assets
        .declared_files_under(profile_id.as_str(), &image_root)
        .map_err(|source| runtime_error(source.code, source.message))?
        .into_iter()
        .filter(|path| {
            let lower = path.to_ascii_lowercase();
            lower.ends_with(".jpg") || lower.ends_with(".jpeg") || lower.ends_with(".png")
        })
        .collect::<Vec<_>>();
    paths.sort();
    // StructureAlarm.py embeds and links its pictures in directory order.
    let indexes = (0..count).collect::<Vec<_>>();
    let (data_indexes, url_indexes) = (indexes.clone(), indexes);
    if data_indexes
        .iter()
        .chain(&url_indexes)
        .copied()
        .max()
        .is_none_or(|maximum| maximum >= paths.len())
    {
        return Err(runtime_error(
            "device_simulator.alarm.image_missing",
            format!(
                "approved image root '{image_root}' does not contain the {count} legacy image slots required by '{alarm_type_id}'"
            ),
        ));
    }
    let pack = assets.pack(profile_id.as_str()).ok_or_else(|| {
        runtime_error(
            "device_simulator.alarm.profile_pack_missing",
            format!("profile pack '{}' is not pinned", profile_id.as_str()),
        )
    })?;
    Ok(vec![data_indexes
        .into_iter()
        .zip(url_indexes)
        .map(|(data_index, url_index)| {
            let reference = ImageAssetRef::Pack {
                pack_id: pack.id.clone(),
                version: pack.version.clone(),
                path: paths[data_index].clone(),
            };
            let url_reference = (url_index != data_index).then(|| ImageAssetRef::Pack {
                pack_id: pack.id.clone(),
                version: pack.version.clone(),
                path: paths[url_index].clone(),
            });
            SelectedPackImage {
                reference,
                url_reference,
            }
        })
        .collect()])
}

fn image_reference_file_name(reference: &ImageAssetRef) -> String {
    match reference {
        ImageAssetRef::Pack { path, .. } => path
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("approved-alarm.jpg")
            .to_owned(),
        ImageAssetRef::UserAsset { extension, .. } => {
            format!("user-alarm.{}", extension.as_str())
        }
    }
}

fn all_declared_pack_images(
    manifests: &BTreeMap<PackIdentity, PackManifest>,
) -> Vec<ImageAssetRef> {
    manifests
        .iter()
        .flat_map(|(identity, manifest)| {
            manifest.files.iter().filter_map(move |file| {
                let lower = file.path.to_ascii_lowercase();
                (lower.ends_with(".jpg") || lower.ends_with(".jpeg") || lower.ends_with(".png"))
                    .then(|| ImageAssetRef::Pack {
                        pack_id: identity.id.clone(),
                        version: identity.version.clone(),
                        path: file.path.clone(),
                    })
            })
        })
        .collect()
}

fn load_user_alarm_image(
    image_id: &str,
    pack_root: &Path,
    user_asset_root: &Path,
    manifests: &BTreeMap<PackIdentity, PackManifest>,
) -> Result<(ImageAssetRef, ImageCache), AlarmRuntimeError> {
    if image_id.len() != 64
        || !image_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(runtime_error(
            "device_simulator.alarm.user_image_id_invalid",
            "custom alarm image IDs must be 64-character lowercase SHA-256 values",
        ));
    }
    let mut candidates = Vec::new();
    for extension in [
        ImageExtension::Jpg,
        ImageExtension::Jpeg,
        ImageExtension::Png,
    ] {
        let path = user_asset_root.join(format!("{image_id}.{}", extension.as_str()));
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(runtime_error(
                        "device_simulator.alarm.user_image_file_invalid",
                        format!(
                            "custom alarm image '{}' is not a regular file",
                            path.display()
                        ),
                    ));
                }
                candidates.push((extension, metadata.len()));
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(runtime_error(
                    "device_simulator.alarm.user_image_read_failed",
                    format!("failed to inspect custom alarm image: {source}"),
                ));
            }
        }
    }
    if candidates.is_empty() {
        return Err(runtime_error(
            "device_simulator.alarm.user_image_not_found",
            format!("custom alarm image '{image_id}' is not available in user storage"),
        ));
    }
    if candidates.len() != 1 {
        return Err(runtime_error(
            "device_simulator.alarm.user_image_ambiguous",
            format!("custom alarm image '{image_id}' exists with multiple extensions"),
        ));
    }
    let (extension, size) = candidates[0];
    let reference = ImageAssetRef::UserAsset {
        image_id: image_id.to_owned(),
        extension,
        sha256: image_id.to_owned(),
        size,
    };
    let cache =
        ImageCache::load_at_start([reference.clone()], pack_root, user_asset_root, manifests)
            .map_err(|source| runtime_error(source.code, source.message))?;
    Ok((reference, cache))
}

fn apply_user_image(definition: &mut AlarmHandlerDefinition, user_image: &ImageAssetRef) -> bool {
    if definition.images.is_empty() {
        return false;
    }
    let extension = match user_image {
        ImageAssetRef::UserAsset { extension, .. } => extension.as_str(),
        ImageAssetRef::Pack { .. } => return false,
    };
    for image in &mut definition.images {
        image.reference = user_image.clone();
        image.url_reference = None;
        image.file_name = format!("user-alarm.{extension}");
    }
    definition.alternate_images.clear();
    true
}

fn validate_image_variant(value: Option<&str>) -> Result<Option<&str>, AlarmRuntimeError> {
    match value.map(str::trim) {
        None => Ok(None),
        Some("small") => Ok(Some("small")),
        Some("normal") => Ok(Some("normal")),
        Some("big") => Ok(Some("big")),
        Some(value) => Err(runtime_error(
            "device_simulator.alarm.image_variant_invalid",
            format!("unsupported alarm image variant '{value}'"),
        )),
    }
}

fn apply_image_variant(
    definition: &mut AlarmHandlerDefinition,
    variant: Option<&str>,
    assets: &RuntimeAssetLayout,
) -> Result<(), AlarmRuntimeError> {
    let Some(variant) = variant else {
        return Ok(());
    };
    if let Some(groups) =
        assets.local_alarm_image_groups(definition.alarm_type_id.as_str(), variant)
    {
        let expected_group_count = 1 + definition.alternate_images.len();
        if groups.len() != expected_group_count {
            return Err(runtime_error(
                "device_simulator.alarm.image_variant_unavailable",
                format!(
                    "local alarm material '{}' has a mismatched '{variant}' group count",
                    definition.alarm_type_id.as_str()
                ),
            ));
        }
        let mut attachment_groups =
            std::iter::once(&mut definition.images).chain(definition.alternate_images.iter_mut());
        for (attachments, images) in attachment_groups.by_ref().zip(groups) {
            if images.len() != attachments.len() {
                return Err(runtime_error(
                    "device_simulator.alarm.image_variant_unavailable",
                    format!(
                        "local alarm material '{}' has no complete '{variant}' variant",
                        definition.alarm_type_id.as_str()
                    ),
                ));
            }
            for (attachment, reference) in attachments.iter_mut().zip(images) {
                attachment.reference = reference.clone();
                attachment.url_reference = None;
                attachment.file_name = image_reference_file_name(reference);
            }
        }
        return Ok(());
    }
    for image in definition
        .images
        .iter_mut()
        .chain(definition.alternate_images.iter_mut().flatten())
    {
        apply_image_variant_to_reference(&mut image.reference, variant, assets)?;
        if let Some(reference) = &mut image.url_reference {
            apply_image_variant_to_reference(reference, variant, assets)?;
        }
    }
    Ok(())
}

fn apply_image_variant_to_reference(
    reference: &mut ImageAssetRef,
    variant: &str,
    assets: &RuntimeAssetLayout,
) -> Result<(), AlarmRuntimeError> {
    let ImageAssetRef::Pack { pack_id, path, .. } = reference else {
        return Ok(());
    };
    let mut segments = path.split('/').map(str::to_owned).collect::<Vec<_>>();
    let variant_index = segments
        .iter()
        .position(|segment| matches!(segment.as_str(), "small" | "normal" | "big"))
        .ok_or_else(|| {
            runtime_error(
                "device_simulator.alarm.image_variant_unavailable",
                format!("alarm image '{path}' has no declared size variant"),
            )
        })?;
    segments[variant_index] = variant.to_owned();
    let preferred = segments.join("/");
    let manifest = assets.manifest(pack_id).ok_or_else(|| {
        runtime_error(
            "device_simulator.alarm.image_manifest_missing",
            format!("image pack '{pack_id}' is not active"),
        )
    })?;
    if manifest.files.iter().any(|file| file.path == preferred) {
        *path = preferred;
        return Ok(());
    }
    let root = preferred
        .rsplit_once('/')
        .map(|(root, _)| root)
        .ok_or_else(|| {
            runtime_error(
                "device_simulator.alarm.image_variant_unavailable",
                format!("alarm image '{preferred}' has no variant directory"),
            )
        })?;
    *path = assets
        .declared_files_under(pack_id, root)
        .map_err(|source| runtime_error(source.code, source.message))?
        .into_iter()
        .find(|candidate| {
            let lower = candidate.to_ascii_lowercase();
            lower.ends_with(".jpg") || lower.ends_with(".jpeg") || lower.ends_with(".png")
        })
        .ok_or_else(|| {
            runtime_error(
                "device_simulator.alarm.image_variant_unavailable",
                format!("alarm image variant '{variant}' is not available below '{root}'"),
            )
        })?;
    Ok(())
}

fn image_manifest_index(
    assets: &RuntimeAssetLayout,
) -> Result<(PathBuf, BTreeMap<PackIdentity, PackManifest>), AlarmRuntimeError> {
    let pins = assets.pinned_pack_directories();
    let first = pins.first().ok_or_else(|| {
        runtime_error(
            "device_simulator.alarm.pack_root_missing",
            "runtime assets contain no pinned packs",
        )
    })?;
    let pack_root = pack_root_for_pin(first)?;
    let mut manifests = BTreeMap::new();
    for pin in pins {
        let expected = if pin.version.trim().is_empty() {
            pack_root.join(&pin.id)
        } else {
            pack_root.join(&pin.id).join(&pin.version)
        };
        if expected != pin.directory {
            return Err(runtime_error(
                "device_simulator.alarm.pack_root_mismatch",
                "runtime asset directories do not share one loose or versioned asset root",
            ));
        }
        let manifest = assets.manifest(&pin.id).cloned().ok_or_else(|| {
            runtime_error(
                "device_simulator.alarm.image_manifest_missing",
                format!("manifest for '{}' is missing", pin.id),
            )
        })?;
        manifests.insert(
            PackIdentity {
                id: pin.id,
                version: pin.version,
            },
            manifest,
        );
    }
    Ok((pack_root, manifests))
}

fn pack_root_for_pin(pin: &PinnedPackDirectory) -> Result<PathBuf, AlarmRuntimeError> {
    let root = if pin.version.trim().is_empty() {
        // Built-in protocol/profile files are loose directories at <root>/<id>.
        pin.directory.parent()
    } else {
        // Legacy release packs remain readable at <root>/<id>/<version>.
        pin.directory.parent().and_then(Path::parent)
    };
    root.map(Path::to_path_buf).ok_or_else(|| {
        runtime_error(
            "device_simulator.alarm.pack_root_invalid",
            "runtime asset directory is not under <root>/<id> or <root>/<id>/<version>",
        )
    })
}

fn build_destinations(
    target: &TargetPlatformConfig,
) -> Result<(BTreeMap<String, reqwest::Url>, Vec<String>, bool), AlarmRuntimeError> {
    let mut destinations = BTreeMap::new();
    // An explicit receiver URL is a deliberate override and is never rewritten
    // by a learned subscription. Server-derived destinations start with the
    // dedicated receiver port and may then follow the platform subscription.
    let mut allow_learned_endpoint = false;
    if let Some(url) = target
        .alarm_receiver_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        destinations.insert("configured".into(), normalize_destination_url(url)?);
    } else {
        allow_learned_endpoint = true;
        for server in &target.servers {
            let host = server.host.trim();
            if host.is_empty() {
                return Err(runtime_error(
                    "device_simulator.alarm.destination_invalid",
                    "platform server host is empty",
                ));
            }
            let rendered_host = if host.contains(':') && !host.starts_with('[') {
                format!("[{host}]")
            } else {
                host.to_owned()
            };
            let receiver_port = target.alarm_receiver_port.unwrap_or(server.port);
            destinations.insert(
                server.id.clone(),
                normalize_destination_url(&format!("http://{rendered_host}:{receiver_port}/"))?,
            );
        }
    }
    let ids = destinations.keys().cloned().collect();
    Ok((destinations, ids, allow_learned_endpoint))
}

/// Apply a learned alarm receiver endpoint to a base destination URL when
/// override is permitted and a subscription has actually been seen. Used for
/// both the rendered `Reference` and the outbound request so they stay aligned.
///
/// The subscription carries `IPAddress` as well as `Port`; a platform whose
/// alarm receiver is not co-located with its web service advertises a different
/// host there, so both are honoured.
fn apply_learned_endpoint(
    base: &reqwest::Url,
    learned: Option<&LearnedAlarmEndpoint>,
    allow: bool,
) -> reqwest::Url {
    let mut url = base.clone();
    if !allow {
        return url;
    }
    let Some(endpoint) = learned else {
        return url;
    };
    let host = endpoint
        .host
        .map(|host| host.to_string())
        .unwrap_or_else(|| endpoint.source_ip.to_string());
    if url.host_str() != Some(host.as_str()) {
        let _ = url.set_host(Some(&host));
    }
    if url.port_or_known_default() != Some(endpoint.port) {
        let _ = url.set_port(Some(endpoint.port));
    }
    url
}

fn normalize_destination_url(value: &str) -> Result<reqwest::Url, AlarmRuntimeError> {
    let mut url = reqwest::Url::parse(value).map_err(|source| {
        runtime_error(
            "device_simulator.alarm.destination_invalid",
            format!("alarm receiver URL is invalid: {source}"),
        )
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(runtime_error(
            "device_simulator.alarm.destination_invalid",
            "alarm receiver URL must be absolute HTTP(S)",
        ));
    }
    url.set_query(None);
    url.set_fragment(None);
    url.set_path("/");
    Ok(url)
}

fn build_context(
    device: &RuntimeAlarmDevice,
    subscription_id: &str,
    device_http_port: u16,
) -> Result<AlarmBuildContext, AlarmRuntimeError> {
    // The structured alarm Reference must advertise the port where this runtime
    // actually serves /System/Picture; otherwise a non-80 listener produces
    // accepted alarms whose pictures cannot be fetched.
    let reference = format!(
        "{}:{device_http_port}/Subscription/Subscribers/1",
        device.preview.ip
    );
    let mut fields = BTreeMap::new();
    fields.insert(DynamicField::DeviceId, device.preview.hardware_id.clone());
    fields.insert(DynamicField::DeviceIp, device.preview.ip.to_string());
    fields.insert(DynamicField::ChannelId, DEFAULT_CHANNEL_ID.into());
    fields.insert(DynamicField::Timestamp, "0".into());
    fields.insert(DynamicField::CaptureTime, "19700101000000000".into());
    fields.insert(
        DynamicField::CaptureTimeText,
        "1970-01-01T00:00:00.000Z".into(),
    );
    fields.insert(DynamicField::Reference, reference);
    fields.insert(DynamicField::SubscriptionId, subscription_id.into());
    fields.insert(DynamicField::EventId, subscription_id.into());
    fields.insert(DynamicField::RelatedId, subscription_id.into());
    fields.insert(DynamicField::PersonId, "1".into());
    fields.insert(DynamicField::AlarmState, "alarm".into());
    Ok(AlarmBuildContext {
        source_ip: Some(device.preview.ip),
        fields,
        multipart_boundary: Some(format!("fst-simulator-{}", uuid::Uuid::new_v4().simple())),
        legacy_values: None,
    })
}

fn scheduled_mode(
    mode: AlarmDispatchMode,
    requested_type_count: usize,
) -> Result<ScheduledDispatchMode, AlarmRuntimeError> {
    Ok(match mode {
        AlarmDispatchMode::Configured => {
            if requested_type_count != 1 {
                return Err(runtime_error(
                    "device_simulator.alarm.configured_type_count_invalid",
                    "configured mode requires exactly one explicit alarm type",
                ));
            }
            ScheduledDispatchMode::Specified
        }
        AlarmDispatchMode::Random => ScheduledDispatchMode::Random,
        AlarmDispatchMode::Sequential => ScheduledDispatchMode::Sequential,
    })
}

fn one_shot_mode(
    mode: AlarmDispatchMode,
    requested_type_count: usize,
) -> Result<ScheduledDispatchMode, AlarmRuntimeError> {
    if requested_type_count == 0 {
        return Ok(ScheduledDispatchMode::Sequential);
    }
    scheduled_mode(mode, requested_type_count)
}

/// Build the operator-facing error body for a failed alarm attempt.
///
/// The scheduler's raw `code` is the only thing that identifies the fault, so it
/// is always carried through alongside the sanitized `details` the sender
/// collected. Dropping either leaves the UI with nothing but a generic message.
fn alarm_error_body(code: String, details: Option<String>) -> SimulatorErrorBody {
    let body = SimulatorErrorBody::new(code, "deviceSimulator.errors.alarmDispatchFailed")
        .retryable(false);
    match details {
        Some(details) => body.with_public_details(details),
        None => body,
    }
}

fn trigger_result(snapshot: AlarmJobSnapshot, duration_ms: u64) -> AlarmTriggerResult {
    let mut errors = Vec::new();
    if let Some(code) = snapshot.last_error_code {
        errors.push(alarm_error_body(code, snapshot.last_error_details));
    }
    AlarmTriggerResult {
        attempted: snapshot.attempted,
        succeeded: snapshot.succeeded,
        failed: snapshot.failed,
        unverified: snapshot.unverified,
        duration_ms,
        errors,
    }
}

fn job_stats_snapshot(snapshot: AlarmJobSnapshot) -> AlarmJobStatsSnapshot {
    AlarmJobStatsSnapshot {
        job_id: snapshot.job_id,
        state: match snapshot.state {
            ScheduledAlarmJobState::Starting => AlarmJobState::Starting,
            ScheduledAlarmJobState::Running => AlarmJobState::Running,
            ScheduledAlarmJobState::Stopping => AlarmJobState::Stopping,
            ScheduledAlarmJobState::Completed | ScheduledAlarmJobState::Cancelled => {
                AlarmJobState::Completed
            }
            ScheduledAlarmJobState::Failed => AlarmJobState::Failed,
        },
        attempted: snapshot.attempted,
        succeeded: snapshot.succeeded,
        failed: snapshot.failed,
        unverified: snapshot.unverified,
        in_flight: snapshot.in_flight,
        average_duration_ms: snapshot.average_duration_ms as f64,
        last_http_status: snapshot.last_http_status,
        last_error: snapshot
            .last_error_code
            .map(|code| alarm_error_body(code, snapshot.last_error_details)),
    }
}

fn normalize_runtime_path(value: &str) -> String {
    let mut normalized = value.replace('\\', "/");
    while normalized.contains("//") {
        normalized = normalized.replace("//", "/");
    }
    normalized.trim_start_matches('/').to_owned()
}

fn sanitize_multipart_name(value: &str) -> String {
    let normalized = value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
                byte as char
            } else {
                '-'
            }
        })
        .collect::<String>();
    let normalized = normalized.trim_matches('-');
    if normalized.is_empty() {
        "event".into()
    } else {
        normalized.chars().take(96).collect()
    }
}

fn stable_numeric_id(job_id: &str, device_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("{job_id}:{device_id}").as_bytes());
    let value = u64::from_be_bytes(digest[..8].try_into().unwrap()) % 900_000_000 + 100_000_000;
    value.to_string()
}

fn random_seed() -> u64 {
    let bytes = *uuid::Uuid::new_v4().as_bytes();
    u64::from_be_bytes(bytes[..8].try_into().unwrap())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn parse_profile_id(value: &str) -> Result<FirstReleaseProfileId, AlarmRuntimeError> {
    match value {
        "ipc-structured" => Ok(FirstReleaseProfileId::IpcStructured),
        _ => Err(runtime_error(
            "device_simulator.alarm.profile_unknown",
            format!("unknown first-release profile '{value}'"),
        )),
    }
}

fn runtime_error(code: &'static str, message: impl Into<String>) -> AlarmRuntimeError {
    AlarmRuntimeError {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::api::{
        preview_devices, DeviceGroupDraft, PlatformAccessMode, RtspPorts, SimulatorStartRequest,
        StreamRuntimeConfig, StreamTransport, TargetPlatformServer,
    };
    use crate::device_simulator::runtime_assets::PinnedPackDirectory;
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn loose_runtime_pins_share_their_direct_parent() {
        let root = TempDir::new().unwrap();
        let pins = ["protocol-core", "media-h264-live", "ipc-structured"]
            .into_iter()
            .map(|id| PinnedPackDirectory {
                id: id.into(),
                version: String::new(),
                directory: root.path().join(id),
            })
            .collect::<Vec<_>>();

        let shared = pack_root_for_pin(&pins[0]).unwrap();
        assert_eq!(shared, root.path());
        for pin in &pins {
            assert_eq!(shared.join(&pin.id), pin.directory);
        }
    }

    #[test]
    fn versioned_runtime_pin_keeps_the_legacy_root_shape() {
        let root = TempDir::new().unwrap();
        let pin = PinnedPackDirectory {
            id: "ipc-structured".into(),
            version: "1.0.3".into(),
            directory: root.path().join("ipc-structured").join("1.0.3"),
        };

        assert_eq!(pack_root_for_pin(&pin).unwrap(), root.path());
    }

    #[tokio::test]
    async fn alarm_http_client_reports_the_first_redirect_instead_of_following_it() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/alarm"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/login", server.uri())),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/login"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let sender = HttpAlarmSender::new(
            BTreeMap::new(),
            Arc::new(RwLock::new(LearnedAlarmSubscriptions::default())),
            false,
        );
        let response = sender
            .client(Ipv4Addr::LOCALHOST)
            .unwrap()
            .post(format!("{}/alarm", server.uri()))
            .body("alarm")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::FOUND);
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url.path(), "/alarm");
    }

    #[test]
    fn ums_response_envelope_confirms_or_rejects_the_alarm_application_result() {
        assert_eq!(decode_ums_alarm_response(b""), None);
        assert_eq!(
            decode_ums_alarm_response(
                br#"{"Response":{"ResponseCode":0,"SubResponseCode":0,"StatusCode":0,"ResponseString":"Succeed"}}"#,
            ),
            Some(AlarmApplicationStatus::Accepted)
        );
        assert_eq!(
            decode_ums_alarm_response(
                br#"{"Response":{"ResponseCode":17,"SubResponseCode":2,"StatusCode":0,"ResponseString":"Invalid image"}}"#,
            ),
            Some(AlarmApplicationStatus::Rejected {
                details: "ResponseCode=17, SubResponseCode=2, StatusCode=0, ResponseString=Invalid image".into(),
            })
        );
        assert_eq!(
            decode_ums_alarm_response(br#"{"unrelated":"login page payload"}"#),
            None
        );
    }

    #[test]
    fn alarm_job_registry_makes_stopping_an_inactive_job_idempotent() {
        let mut jobs = AlarmJobRegistry::new(4);
        jobs.insert("alarm-explicit".into(), 7_u8);

        assert_eq!(jobs.take_for_stop("alarm-explicit").unwrap(), Some(7));
        assert_eq!(jobs.take_for_stop("alarm-explicit").unwrap(), None);
        assert_eq!(jobs.take_for_stop("alarm-never-started").unwrap(), None);
    }

    #[test]
    fn alarm_job_registry_remembers_a_naturally_finished_job() {
        let mut jobs = AlarmJobRegistry::new(4);
        jobs.insert("alarm-finite".into(), 11_u8);

        assert_eq!(jobs.mark_finished("alarm-finite"), Some(11));
        assert_eq!(jobs.take_for_stop("alarm-finite").unwrap(), None);
    }

    #[test]
    fn alarm_job_registry_bounds_recently_stopped_ids() {
        let mut jobs = AlarmJobRegistry::new(2);
        for (job_id, value) in [("alarm-1", 1_u8), ("alarm-2", 2), ("alarm-3", 3)] {
            jobs.insert(job_id.into(), value);
            assert_eq!(jobs.take_for_stop(job_id).unwrap(), Some(value));
        }

        assert_eq!(jobs.take_for_stop("alarm-1").unwrap(), None);
        assert_eq!(jobs.take_for_stop("alarm-2").unwrap(), None);
        assert_eq!(jobs.take_for_stop("alarm-3").unwrap(), None);
    }

    #[test]
    fn learned_alarm_endpoint_follows_servers_but_never_an_explicit_receiver() {
        use crate::device_simulator::api::TargetPlatformConfig;

        // Server-derived destinations allow the learned endpoint to override.
        let servers = TargetPlatformConfig {
            kind: TargetPlatform::Ums,
            servers: vec![TargetPlatformServer {
                id: "primary".into(),
                host: "198.51.100.9".into(),
                port: 6000,
            }],
            access_mode: PlatformAccessMode::Open,
            alarm_receiver_url: None,
            alarm_receiver_port: Some(55_025),
        };
        let (destinations, _ids, allow_learned) = build_destinations(&servers).unwrap();
        assert!(allow_learned);
        let base = &destinations["primary"];
        assert_eq!(base.port(), Some(55_025));

        // A port far outside the legacy 55000..55999 range must be honoured.
        let learned = LearnedAlarmEndpoint::new(
            "198.51.100.9".parse().unwrap(),
            None,
            22_815,
            Some(600),
            1_000,
        );
        assert_eq!(
            apply_learned_endpoint(base, Some(&learned), true).port(),
            Some(22_815)
        );
        assert_eq!(
            apply_learned_endpoint(base, Some(&learned), true).host_str(),
            Some("198.51.100.9")
        );

        // If IPAddress is absent, the TCP peer identifies the correct UMS
        // instead of borrowing the first configured server's host.
        let peer_fallback = LearnedAlarmEndpoint::new(
            "198.51.100.44".parse().unwrap(),
            None,
            22_816,
            Some(600),
            1_000,
        );
        let resolved_peer = apply_learned_endpoint(base, Some(&peer_fallback), true);
        assert_eq!(resolved_peer.host_str(), Some("198.51.100.44"));
        assert_eq!(resolved_peer.port(), Some(22_816));
        // Override disabled or nothing learned yet leaves the configured port.
        assert_eq!(
            apply_learned_endpoint(base, Some(&learned), false).port(),
            Some(55_025)
        );
        assert_eq!(
            apply_learned_endpoint(base, None, true).port(),
            Some(55_025)
        );

        // The advertised receiver host wins over the configured server host.
        let relocated = LearnedAlarmEndpoint::new(
            "198.51.100.9".parse().unwrap(),
            Some(Ipv4Addr::new(192, 115, 1, 55)),
            22_815,
            None,
            1_000,
        );
        let resolved = apply_learned_endpoint(base, Some(&relocated), true);
        assert_eq!(resolved.host_str(), Some("192.115.1.55"));
        assert_eq!(resolved.port(), Some(22_815));

        let mut fallback = servers.clone();
        fallback.alarm_receiver_port = None;
        let (fallback_destinations, _, _) = build_destinations(&fallback).unwrap();
        assert_eq!(fallback_destinations["primary"].port(), Some(6000));

        // An explicit receiver URL is a deliberate choice and is never rewritten.
        let explicit = TargetPlatformConfig {
            kind: TargetPlatform::Ums,
            servers: Vec::new(),
            access_mode: PlatformAccessMode::Open,
            alarm_receiver_url: Some("http://198.51.100.9:7000/alarm".into()),
            alarm_receiver_port: Some(55_025),
        };
        let (explicit_destinations, _i, allow_explicit) = build_destinations(&explicit).unwrap();
        assert!(!allow_explicit);
        assert_eq!(explicit_destinations["configured"].port(), Some(7000));
    }

    #[test]
    fn learned_alarm_subscriptions_are_bounded_deduplicated_and_expiry_aware() {
        let mut subscriptions = LearnedAlarmSubscriptions::default();
        for index in 1..=17_u8 {
            subscriptions.upsert(LearnedAlarmEndpoint::new(
                format!("198.51.100.{index}").parse().unwrap(),
                None,
                22_815,
                None,
                u64::from(index),
            ));
        }

        let entries = subscriptions.entries();
        assert_eq!(entries.len(), MAX_LEARNED_ALARM_SUBSCRIPTIONS);
        assert!(entries
            .iter()
            .all(|entry| entry.source_ip.to_string() != "198.51.100.1"));

        let replacement = LearnedAlarmEndpoint::new(
            "198.51.100.17".parse().unwrap(),
            None,
            23_000,
            Some(1),
            1_000,
        );
        let replacement_id = replacement.id.clone();
        subscriptions.upsert(replacement);
        assert_eq!(
            subscriptions.entries().len(),
            MAX_LEARNED_ALARM_SUBSCRIPTIONS
        );
        assert_eq!(
            subscriptions.get(&replacement_id).unwrap().learned_at_ms,
            1_000
        );
        assert_eq!(subscriptions.get(&replacement_id).unwrap().port, 23_000);
        assert_eq!(
            subscriptions.active_entries(1_999).len(),
            MAX_LEARNED_ALARM_SUBSCRIPTIONS
        );
        assert_eq!(
            subscriptions.active_entries(2_000).len(),
            MAX_LEARNED_ALARM_SUBSCRIPTIONS - 1
        );
    }

    #[test]
    fn compiles_legacy_json_into_bounded_runtime_fields_without_executable_content() {
        let template = compile_json_template(
            br#"{
                "Reference":"legacy",
                "AlarmInfo":{"TimeStamp":1,"AlarmType":"Old","AlarmSeq":2,"Nested":{"Type":"Camera"}},
                "Image":{"Size":0,"Data":"legacy","URL":"/LAPI/V1.0/System/Picture?Index=C:\\old.jpg"}
            }"#,
            Some("CrossLine"),
            usize::MAX,
        )
        .unwrap();
        // Identity fields are injected by the profile-specific mapper after
        // rendering; they are not global template variables.
        assert!(!template.fields().contains(&DynamicField::Reference));
        assert!(!template.fields().contains(&DynamicField::Timestamp));
        assert!(!template.fields().contains(&DynamicField::EventId));
        assert!(template.fields().contains(&DynamicField::ImageBase64));
        assert!(template.fields().contains(&DynamicField::ImageSize));
        let rendered = String::from_utf8(
            template
                .render(&BTreeMap::from([
                    (DynamicField::ImageBase64, "YWJj".into()),
                    (DynamicField::ImageSize, "3".into()),
                ]))
                .unwrap(),
        )
        .unwrap();
        assert!(rendered.contains("\"TimeStamp\":1"));
        assert!(rendered.contains("\"Reference\":\"legacy\""));
        assert!(rendered.contains("\"AlarmType\":\"CrossLine\""));
        assert!(rendered.contains("\"Type\":\"Camera\""));
        assert!(rendered.contains("\"Size\":3"));
        assert!(!rendered.contains("C:\\old.jpg"));
    }

    #[test]
    fn event_type_override_requires_an_exact_supported_json_path() {
        let error = compile_json_template(br#"{"Nested":{"Type":"Camera"}}"#, Some("CrossLine"), 0)
            .unwrap_err();
        assert_eq!(
            error.code,
            "device_simulator.alarm.event_type_field_missing"
        );
    }

    #[test]
    fn compiles_distinct_image_slots_in_legacy_order() {
        let template = compile_json_template(
            br#"{"ImageList":[{"Size":1,"Data":"first"},{"Size":2,"Data":"second"}]}"#,
            None,
            usize::MAX,
        )
        .unwrap();
        assert!(template.fields().contains(&DynamicField::ImageBase64));
        assert!(template.fields().contains(&DynamicField::ImageBase642));
        assert!(template.fields().contains(&DynamicField::ImageSize));
        assert!(template.fields().contains(&DynamicField::ImageSize2));
        let rendered = String::from_utf8(
            template
                .render(&BTreeMap::from([
                    (DynamicField::ImageBase64, "Zmlyc3Q=".into()),
                    (DynamicField::ImageBase642, "c2Vjb25k".into()),
                    (DynamicField::ImageSize, "5".into()),
                    (DynamicField::ImageSize2, "6".into()),
                ]))
                .unwrap(),
        )
        .unwrap();
        assert!(rendered.contains("\"Data\":\"Zmlyc3Q=\""));
        assert!(rendered.contains("\"Data\":\"c2Vjb25k\""));
        assert!(rendered.contains("\"Size\":5"));
        assert!(rendered.contains("\"Size\":6"));
    }

    #[test]
    fn success_semantics_remain_unverified_and_destination_urls_are_origin_scoped() {
        let url =
            normalize_destination_url("http://127.0.0.1:18080/custom/path?token=secret").unwrap();
        assert_eq!(url.as_str(), "http://127.0.0.1:18080/");
        assert_eq!(ResponseSuccessRule::Unverified.evaluate(200), None);
    }

    #[test]
    fn job_stats_api_preserves_the_last_http_status() {
        let stats = job_stats_snapshot(AlarmJobSnapshot {
            job_id: "alarm-status".into(),
            state: ScheduledAlarmJobState::Running,
            attempted: 1,
            succeeded: 0,
            failed: 0,
            unverified: 1,
            timed_out: 0,
            cancelled: 0,
            rejected: 0,
            in_flight: 0,
            average_duration_ms: 4,
            last_http_status: Some(202),
            last_error_code: None,
            last_error_details: None,
            devices: BTreeMap::new(),
        });

        assert_eq!(stats.last_http_status, Some(202));
    }

    #[test]
    fn configured_mode_requires_one_explicit_alarm_type() {
        assert!(scheduled_mode(AlarmDispatchMode::Configured, 0).is_err());
        assert_eq!(
            scheduled_mode(AlarmDispatchMode::Configured, 1).unwrap(),
            ScheduledDispatchMode::Specified
        );
    }

    #[test]
    fn one_shot_all_types_always_uses_sequential_dispatch() {
        assert_eq!(
            one_shot_mode(AlarmDispatchMode::Random, 0).unwrap(),
            ScheduledDispatchMode::Sequential
        );
        assert_eq!(
            one_shot_mode(AlarmDispatchMode::Sequential, 1).unwrap(),
            ScheduledDispatchMode::Sequential
        );
    }

    #[test]
    fn built_in_structured_templates_keep_alarm_categories_independent() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/device-simulator-base/ipc-structured/object/StructStruct");
        for (file, active_count, expected_types) in [
            ("StructurePerson.json", "PersonNum", &[23u64, 12][..]),
            ("StructureFace.json", "FaceNum", &[15u64, 11][..]),
            ("StructureCar.json", "VehicleNum", &[23u64, 13, 2][..]),
            (
                "StructureNonMotor.json",
                "NonMotorVehicleNum",
                &[23u64, 14][..],
            ),
        ] {
            let document: serde_json::Value =
                serde_json::from_slice(&fs::read(root.join(file)).unwrap()).unwrap();
            let objects = document.pointer("/StructureInfo/ObjInfo").unwrap();
            for count in ["PersonNum", "FaceNum", "VehicleNum", "NonMotorVehicleNum"] {
                assert_eq!(
                    objects[count].as_u64().unwrap(),
                    (count == active_count) as u64,
                    "{file} must contain only its own object category",
                );
            }
            let types = document["StructureInfo"]["ImageInfoList"]
                .as_array()
                .unwrap()
                .iter()
                .map(|image| image["Type"].as_u64().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(types, expected_types, "{file} image roles are incorrect");
        }
    }

    #[tokio::test]
    async fn approved_release_alarm_registry_loads_when_explicitly_configured() {
        let Ok(root) = std::env::var("FST_APPROVED_PACK_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        assert!(
            root.is_dir(),
            "FST_APPROVED_PACK_ROOT does not exist or is not a directory: {}",
            root.display()
        );
        let version = std::env::var("FST_APPROVED_PACK_VERSION").unwrap_or_else(|_| "1.0.3".into());
        let pins = ["protocol-core", "media-h264-live", "ipc-structured"]
            .into_iter()
            .map(|id| PinnedPackDirectory {
                id: id.into(),
                version: version.clone(),
                directory: root.join(id).join(&version),
            })
            .collect::<Vec<_>>();
        let profile_ids = ["ipc-structured"].map(str::to_owned);
        let assets = Arc::new(RuntimeAssetLayout::load(&pins, &profile_ids).unwrap());
        let request = SimulatorStartRequest {
            platform: TargetPlatformConfig {
                kind: TargetPlatform::Ums,
                servers: vec![TargetPlatformServer {
                    id: "receiver".into(),
                    host: "127.0.0.1".into(),
                    port: 18080,
                }],
                access_mode: PlatformAccessMode::Open,
                alarm_receiver_url: None,
                alarm_receiver_port: Some(55_025),
            },
            interface_id: "fixture-interface".into(),
            start_ip: "127.30.0.10".parse().unwrap(),
            device_ips: vec![],
            subnet_prefix: 24,
            device_http_port: 18081,
            rtsp_ports: RtspPorts {
                main: 18554,
                sub: 18555,
                third: 18556,
            },
            _legacy_allow_local_player_access: true,
            media_theme_id: crate::device_simulator::api::DEFAULT_MEDIA_THEME_ID.into(),
            groups: vec![DeviceGroupDraft {
                id: "structured".into(),
                profile_id: "ipc-structured".into(),
                count: 1,
            }],
            stream: StreamRuntimeConfig {
                transport: StreamTransport::TcpInterleaved,
                enabled_streams: vec![
                    crate::device_simulator::api::DeviceSimulatorStreamKind::Main,
                    crate::device_simulator::api::DeviceSimulatorStreamKind::Sub,
                    crate::device_simulator::api::DeviceSimulatorStreamKind::Third,
                ],
                audio_enabled: false,
                time_watermark_enabled: true,
            },
        };
        let preview = preview_devices(&request).unwrap();
        let app_data = TempDir::new().unwrap();
        let runtime = AlarmRuntime::new(AlarmRuntimeConfig {
            platform: TargetPlatform::Ums,
            target: request.platform,
            preview,
            device_http_port: request.device_http_port,
            assets,
            app_data_dir: app_data.path().to_path_buf(),
        })
        .unwrap();
        assert_eq!(runtime.registry.len(), 4);
        assert!(runtime
            .registry
            .definitions()
            .all(|definition| definition.profile_id == FirstReleaseProfileId::IpcStructured));
        assert!(runtime.image_cache.read().declared_len() > 0);
        assert!(runtime.image_cache.read().is_empty());
        let legacy_values = crate::device_simulator::alarms::LegacyAlarmValues::new(0x0f0f_0f0f);
        let structured_device = runtime
            .devices
            .values()
            .find(|device| device.profile_id == FirstReleaseProfileId::IpcStructured)
            .unwrap();
        for (alarm_type_id, expected_types) in [
            ("person", vec![23, 12]),
            ("face", vec![15, 11]),
            ("car", vec![23, 13, 2]),
            ("nonmotor", vec![23, 14]),
        ] {
            let definition = runtime
                .registry
                .definitions()
                .find(|definition| {
                    definition.profile_id == FirstReleaseProfileId::IpcStructured
                        && definition.alarm_type_id.as_str() == alarm_type_id
                })
                .unwrap();
            assert_eq!(definition.images.len(), expected_types.len());
            assert_eq!(
                definition.transport.path,
                "/LAPI/V1.0/System/Event/Notification/Structure"
            );
            let mut context =
                build_context(structured_device, "321", runtime.device_http_port).unwrap();
            context
                .fields
                .insert(DynamicField::Timestamp, "1710000000".into());
            context
                .fields
                .insert(DynamicField::CaptureTime, "20240309160000000".into());
            context.fields.insert(DynamicField::EventId, "321".into());
            context.legacy_values = Some(crate::device_simulator::alarms::LegacyAlarmValues::new(
                0x3210_3210,
            ));
            let request = crate::device_simulator::alarms::build_alarm_request(
                definition,
                &context,
                &runtime.image_cache.read(),
            )
            .unwrap();
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            let expected_reference = format!(
                "{}:{}/Subscription/Subscribers/1",
                structured_device.preview.ip, runtime.device_http_port
            );
            assert_eq!(
                body["Reference"].as_str(),
                Some(expected_reference.as_str())
            );
            let images = body["StructureInfo"]["ImageInfoList"].as_array().unwrap();
            assert_eq!(
                images
                    .iter()
                    .map(|image| image["Type"].as_i64().unwrap())
                    .collect::<Vec<_>>(),
                expected_types
            );
            for image in images {
                assert_eq!(image["Format"].as_u64(), Some(1));
                assert!(image["Size"].as_u64().unwrap() > 0);
                assert!(!image["Data"].as_str().unwrap().is_empty());
                let url = image["URL"].as_str().unwrap();
                assert!(url.contains(&format!("Type={}", image["Type"].as_i64().unwrap())));
                let index = url
                    .split("Index=")
                    .nth(1)
                    .unwrap()
                    .split('&')
                    .next()
                    .unwrap();
                assert_eq!(index.len(), 64);
                assert!(index.bytes().all(|byte| byte.is_ascii_hexdigit()));
            }
        }
        let structured_car = runtime
            .registry
            .definitions()
            .find(|definition| {
                definition.profile_id == FirstReleaseProfileId::IpcStructured
                    && definition.alarm_type_id.as_str() == "car"
            })
            .unwrap();
        let mut structured_context =
            build_context(structured_device, "457", runtime.device_http_port).unwrap();
        structured_context.legacy_values = Some(legacy_values.clone());
        let structured_request = crate::device_simulator::alarms::build_alarm_request(
            structured_car,
            &structured_context,
            &runtime.image_cache.read(),
        )
        .unwrap();
        let structured_body: Value = serde_json::from_slice(&structured_request.body).unwrap();
        let vehicle_attributes = &structured_body["StructureInfo"]["ObjInfo"]["VehicleInfoList"][0];
        assert!(vehicle_attributes["VehicleAttributeInfo"]["SpeedType"]
            .as_u64()
            .is_some_and(|value| value <= 5));
        assert!(vehicle_attributes["PlateAttributeInfo"]["PlateNo"]
            .as_str()
            .is_some_and(|value| value.starts_with("UV") && value.len() == 5));

        use sha2::{Digest, Sha256};
        let user_root = app_data
            .path()
            .join("device-simulator")
            .join("user-alarm-images");
        fs::create_dir_all(&user_root).unwrap();
        let user_bytes = b"runtime-shared-user-picture";
        let user_id = format!("{:x}", Sha256::digest(user_bytes));
        fs::write(user_root.join(format!("{user_id}.jpg")), user_bytes).unwrap();
        let (reference, shared) = runtime.load_job_image(Some(&user_id)).await.unwrap();
        let reference = reference.unwrap();
        assert!(Arc::ptr_eq(&shared, &runtime.image_cache));
        assert!(runtime
            .image_cache
            .read()
            .get_by_token(&crate::device_simulator::alarms::image_reference_token(
                &reference
            ))
            .is_some());
    }

    #[test]
    fn user_alarm_images_are_resolved_by_exact_sha256_and_verified_before_use() {
        use sha2::{Digest, Sha256};

        let root = TempDir::new().unwrap();
        let user_root = root.path().join("user-alarm-images");
        fs::create_dir_all(&user_root).unwrap();
        let bytes = b"verified custom alarm image";
        let image_id = format!("{:x}", Sha256::digest(bytes));
        fs::write(user_root.join(format!("{image_id}.png")), bytes).unwrap();

        let (reference, cache) = load_user_alarm_image(
            &image_id,
            &root.path().join("packs"),
            &user_root,
            &BTreeMap::new(),
        )
        .unwrap();
        assert!(matches!(
            reference,
            ImageAssetRef::UserAsset {
                extension: ImageExtension::Png,
                ..
            }
        ));
        assert_eq!(&*cache.get(&reference).unwrap().bytes, bytes);

        let error = load_user_alarm_image(
            &image_id.to_ascii_uppercase(),
            &root.path().join("packs"),
            &user_root,
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert_eq!(error.code, "device_simulator.alarm.user_image_id_invalid");
    }
}
