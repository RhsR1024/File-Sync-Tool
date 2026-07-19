use crate::device_simulator::alarms::scheduler::{
    AlarmClock, AlarmDeviceTarget, AlarmDispatchMode as ScheduledDispatchMode, AlarmFuture,
    AlarmInvocation, AlarmJobSnapshot, AlarmScheduler, AlarmSchedulerLimits, AlarmSendError,
    AlarmSender, AlarmSenderResponse, OneShotAlarmJob, OutboundAlarmRequest, PeriodicAlarmJob,
    RunningAlarmJob, ScheduledAlarmJobState, SystemAlarmClock,
};
use crate::device_simulator::alarms::{
    embedded_image_count, AlarmBuildContext, AlarmHandlerDefinition, AlarmHandlerId,
    AlarmHandlerRegistry, AlarmRequestDefinition, AlarmTypeId, BodyEncoding, CompiledTemplate,
    DynamicField, FixtureProvenance, HandlerEvidence, HttpMethod, ImageAssetRef,
    ImageAttachmentDefinition, ImageCache, ImageExtension, ImagePolicy, PackIdentity,
    PlatformEvidence, PlatformVerification, RecoveryDefinition, RecoveryTrigger,
    ResponseSuccessRule, SourceBinding, TransportDefinition,
};
use crate::device_simulator::api::{
    AlarmDispatchMode, AlarmJobRequest, AlarmJobStatsSnapshot, AlarmTriggerResult,
    DeviceIdentityPreviewDto, DevicePreview, TargetPlatformConfig,
};
use crate::device_simulator::assets::catalog::PackManifest;
use crate::device_simulator::errors::SimulatorErrorBody;
use crate::device_simulator::models::AlarmJobState;
use crate::device_simulator::profiles::scope::{FirstReleaseProfileId, TargetPlatform};
use crate::device_simulator::runtime_assets::RuntimeAssetLayout;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;
use tokio::sync::Mutex;

const ALARM_TYPES_SCHEMA_VERSION: u32 = 1;
const DEFAULT_CHANNEL_ID: &str = "1";
const DEFAULT_ALARM_REQUEST_TIMEOUT_MS: u64 = 10_000;
const NUMERIC_TIMESTAMP_SENTINEL: &str = "__FST_NUMERIC_TIMESTAMP__";
const NUMERIC_ID_SENTINEL: &str = "__FST_NUMERIC_ID__";
const NUMERIC_CHANNEL_SENTINEL: &str = "__FST_NUMERIC_CHANNEL__";
const NUMERIC_IMAGE_SIZE_SENTINEL: &str = "__FST_NUMERIC_IMAGE_SIZE__";
const NUMERIC_IMAGE_SIZE_2_SENTINEL: &str = "__FST_NUMERIC_IMAGE_SIZE_2__";
const NUMERIC_IMAGE_SIZE_3_SENTINEL: &str = "__FST_NUMERIC_IMAGE_SIZE_3__";
const NUMERIC_IMAGE_SIZE_4_SENTINEL: &str = "__FST_NUMERIC_IMAGE_SIZE_4__";

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
    structure_template_vms: Option<String>,
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

pub struct AlarmRuntime {
    scheduler: AlarmScheduler,
    registry: AlarmHandlerRegistry,
    image_cache: Arc<ImageCache>,
    assets: Arc<RuntimeAssetLayout>,
    pack_root: PathBuf,
    user_asset_root: PathBuf,
    image_manifests: Arc<BTreeMap<PackIdentity, PackManifest>>,
    devices: BTreeMap<String, RuntimeAlarmDevice>,
    destinations: BTreeMap<String, reqwest::Url>,
    destination_ids: Vec<String>,
    device_http_port: u16,
    platform: TargetPlatform,
    jobs: Mutex<BTreeMap<String, ActiveAlarmJob>>,
}

impl AlarmRuntime {
    pub fn new(config: AlarmRuntimeConfig) -> Result<Self, AlarmRuntimeError> {
        if config.preview.devices.is_empty() {
            return Err(runtime_error(
                "device_simulator.alarm.preview_empty",
                "alarm runtime requires at least one simulated device",
            ));
        }
        let (destinations, destination_ids) = build_destinations(&config.target)?;
        let sender = Arc::new(HttpAlarmSender::new(destinations.clone()));
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
        let image_cache =
            ImageCache::load_at_start(image_references, &pack_root, &user_asset_root, &manifests)
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
            image_cache: Arc::new(image_cache),
            assets: config.assets,
            pack_root,
            user_asset_root,
            image_manifests,
            devices,
            destinations,
            destination_ids,
            device_http_port: config.device_http_port,
            platform: config.platform,
            jobs: Mutex::new(BTreeMap::new()),
        })
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
                mode: scheduled_mode(request.mode, request.alarm_type_ids.len())?,
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
        let active = self.jobs.lock().await.remove(job_id).ok_or_else(|| {
            runtime_error(
                "device_simulator.alarm.job_not_found",
                format!("alarm job '{job_id}' is not active"),
            )
        })?;
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
            std::mem::take(&mut *active)
                .into_values()
                .collect::<Vec<_>>()
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
            .values()
            .map(|job| job.tracker.clone())
            .collect::<Vec<_>>();
        let mut snapshots = Vec::with_capacity(trackers.len());
        for tracker in trackers {
            snapshots.push(job_stats_snapshot(tracker.snapshot().await));
        }
        snapshots
    }

    pub async fn active_job_count(&self) -> u32 {
        self.jobs.lock().await.len().try_into().unwrap_or(u32::MAX)
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
            let destination_id = self.destination_ids[index % self.destination_ids.len()].clone();
            let destination = self.destinations.get(&destination_id).ok_or_else(|| {
                runtime_error(
                    "device_simulator.alarm.destination_missing",
                    format!("alarm destination '{destination_id}' is not configured"),
                )
            })?;
            let mut invocations = Vec::with_capacity(definitions.len());
            for mut definition in definitions {
                if let Some(user_image) = &user_image {
                    if apply_user_image(&mut definition, user_image) {
                        user_image_applied = true;
                    }
                } else {
                    apply_image_variant(&mut definition, image_variant, &self.assets)?;
                }
                let context = build_context(
                    device,
                    &definition,
                    &subscription_id,
                    destination,
                    self.device_http_port,
                )?;
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
    ) -> Result<(Option<ImageAssetRef>, Arc<ImageCache>), AlarmRuntimeError> {
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
        let merged = self
            .image_cache
            .merged(additional)
            .map_err(|source| runtime_error(source.code, source.message))?;
        Ok((Some(reference), Arc::new(merged)))
    }
}

#[derive(Debug)]
struct HttpAlarmSender {
    destinations: BTreeMap<String, reqwest::Url>,
    clients: StdMutex<HashMap<Ipv4Addr, reqwest::Client>>,
}

impl HttpAlarmSender {
    fn new(destinations: BTreeMap<String, reqwest::Url>) -> Self {
        Self {
            destinations,
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
            .build()
            .map_err(|_| AlarmSendError::new("device_simulator.alarm.http_client_failed", false))?;
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
            let base = self
                .destinations
                .get(&outbound.destination_id)
                .cloned()
                .ok_or_else(|| {
                    AlarmSendError::new("device_simulator.alarm.destination_unknown", false)
                })?;
            let url = base
                .join(outbound.request.path.trim_start_matches('/'))
                .map_err(|_| {
                    AlarmSendError::new("device_simulator.alarm.destination_url_invalid", false)
                })?;
            let client = self.client(outbound.request.source_ip)?;
            let mut builder = match outbound.request.method {
                HttpMethod::Post => client.post(url),
            };
            for (name, value) in &outbound.request.headers {
                let name = reqwest::header::HeaderName::from_str(name).map_err(|_| {
                    AlarmSendError::new("device_simulator.alarm.header_invalid", false)
                })?;
                let value = reqwest::header::HeaderValue::from_str(value).map_err(|_| {
                    AlarmSendError::new("device_simulator.alarm.header_invalid", false)
                })?;
                builder = builder.header(name, value);
            }
            let response = builder
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
                })?;
            Ok(AlarmSenderResponse {
                status: response.status().as_u16(),
            })
        })
    }
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
    platform: TargetPlatform,
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
    let selected_template = if platform == TargetPlatform::Vms {
        definition
            .structure_template_vms
            .as_ref()
            .or(definition.structure_template.as_ref())
            .or(definition.alarm_template.as_ref())
    } else {
        definition
            .structure_template
            .as_ref()
            .or(definition.alarm_template.as_ref())
    }
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
        .structure_template_vms
        .as_ref()
        .is_some_and(|path| path == selected_template)
        || definition
            .structure_template
            .as_ref()
            .is_some_and(|path| path == selected_template);
    let embedded_image_limit = if definition.supports_pictures && !multipart {
        match (profile_id, definition.id.as_str()) {
            (FirstReleaseProfileId::NvrVehicle, "snap") => 2,
            _ => usize::MAX,
        }
    } else {
        0
    };
    let template_bytes = assets
        .read_from_pack(
            profile_id.as_str(),
            &normalize_runtime_path(selected_template),
        )
        .map_err(|source| runtime_error(source.code, source.message))?;
    let template_bytes = normalize_alarm_source_type(
        &template_bytes,
        profile_id,
        definition.source_type.as_deref(),
        &definition.event_type,
    )?;
    let event_type_override =
        (definition.event_type != definition.id).then_some(definition.event_type.as_str());
    let template =
        compile_json_template(&template_bytes, event_type_override, embedded_image_limit)?;
    let image_count = if !definition.supports_pictures {
        0
    } else if multipart {
        match (profile_id, platform) {
            (FirstReleaseProfileId::IpcCustom, TargetPlatform::Vms) => 4,
            _ => 1,
        }
    } else {
        embedded_image_count(&template)
    };
    let image_references = select_pack_images(
        assets,
        profile_id,
        definition.image_root.as_deref(),
        image_count,
        &definition.id,
    )?;
    let images = image_references
        .into_iter()
        .enumerate()
        .map(|(index, reference)| ImageAttachmentDefinition {
            file_name: image_reference_file_name(&reference),
            reference,
            field_name: if profile_id == FirstReleaseProfileId::IpcCustom {
                format!("imageindex{}", index + 1)
            } else if image_count == 1 {
                "image".into()
            } else {
                format!("image{}", index + 1)
            },
        })
        .collect::<Vec<_>>();
    let image_policy = match (profile_id, images.is_empty()) {
        (FirstReleaseProfileId::NvrCommon, _) => ImagePolicy::Forbidden,
        (_, false) => ImagePolicy::Required,
        (_, true) => ImagePolicy::Forbidden,
    };
    let path = if multipart {
        if profile_id == FirstReleaseProfileId::IpcCustom && platform == TargetPlatform::Vms {
            "/LAPI/V1.1/System/Event/Notification/".to_owned()
        } else {
            "/LAPI/V1.1/System/Event/Notification".to_owned()
        }
    } else if selected_is_structure {
        if profile_id == FirstReleaseProfileId::NvrVehicle && definition.alarm_template.is_some() {
            "/LAPI/V1.0/System/Event/Notification/VehicleEventInfo".to_owned()
        } else {
            definition
                .structure_path
                .clone()
                .unwrap_or_else(|| "/LAPI/V1.0/System/Event/Notification/Structure".to_owned())
        }
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
        let bytes = normalize_alarm_source_type(
            &bytes,
            profile_id,
            definition.source_type.as_deref(),
            &definition.event_type,
        )?;
        RecoveryDefinition::RenderWith {
            template: compile_recovery_json_template(&bytes, recovery_event, profile_id)?,
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
        transport: TransportDefinition {
            method: HttpMethod::Post,
            path,
            source_binding: SourceBinding::DeviceIp,
            body_encoding: if multipart {
                BodyEncoding::Multipart {
                    metadata_name: sanitize_multipart_name(&definition.event_type),
                    metadata_content_type: "text/plain; charset=utf-8".into(),
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
    let mut value: Value = serde_json::from_slice(bytes).map_err(|source| {
        runtime_error(
            "device_simulator.alarm.template_json_invalid",
            format!("approved alarm template is not valid JSON: {source}"),
        )
    })?;
    let mut image_fields = ImageFieldCounters::default();
    rewrite_json_value(
        None,
        &mut value,
        event_type,
        embedded_image_limit,
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
        (NUMERIC_ID_SENTINEL, "{{subscription_id}}"),
        (NUMERIC_CHANNEL_SENTINEL, "{{channel_id}}"),
        (NUMERIC_IMAGE_SIZE_SENTINEL, "{{image_size}}"),
        (NUMERIC_IMAGE_SIZE_2_SENTINEL, "{{image_size_2}}"),
        (NUMERIC_IMAGE_SIZE_3_SENTINEL, "{{image_size_3}}"),
        (NUMERIC_IMAGE_SIZE_4_SENTINEL, "{{image_size_4}}"),
    ] {
        rendered = rendered.replace(&format!("\"{sentinel}\""), marker);
    }
    CompiledTemplate::compile(rendered.as_bytes())
        .map_err(|source| runtime_error(source.code, source.message))
}

fn compile_recovery_json_template(
    bytes: &[u8],
    recovery_event: &str,
    profile_id: FirstReleaseProfileId,
) -> Result<CompiledTemplate, AlarmRuntimeError> {
    if profile_id != FirstReleaseProfileId::IpcSmart {
        return compile_json_template(bytes, Some(recovery_event), 0);
    }
    let mut value: Value = serde_json::from_slice(bytes).map_err(|source| {
        runtime_error(
            "device_simulator.alarm.template_json_invalid",
            format!("approved recovery template is not valid JSON: {source}"),
        )
    })?;
    if let Some(root) = value.as_object_mut() {
        root.remove("RelatedObjects");
        if let Some(alarm_info) = root.get_mut("AlarmInfo").and_then(Value::as_object_mut) {
            alarm_info.remove("RelatedID");
        }
    }
    let normalized = serde_json::to_vec(&value).map_err(|source| {
        runtime_error(
            "device_simulator.alarm.template_serialize_failed",
            format!("approved recovery template could not be normalized: {source}"),
        )
    })?;
    compile_json_template(&normalized, Some(recovery_event), 0)
}

fn normalize_alarm_source_type(
    bytes: &[u8],
    profile_id: FirstReleaseProfileId,
    source_type: Option<&str>,
    event_type: &str,
) -> Result<Vec<u8>, AlarmRuntimeError> {
    if profile_id != FirstReleaseProfileId::NvrCommon {
        return Ok(bytes.to_vec());
    }
    let mut value: Value = serde_json::from_slice(bytes).map_err(|source| {
        runtime_error(
            "device_simulator.alarm.template_json_invalid",
            format!("approved NVR alarm template is not valid JSON: {source}"),
        )
    })?;
    let alarm_source_type = if source_type == Some("device") {
        0
    } else if event_type == "InputAlarmOn" {
        9
    } else {
        8
    };
    if let Some(alarm_info) = value
        .as_object_mut()
        .and_then(|root| root.get_mut("AlarmInfo"))
        .and_then(Value::as_object_mut)
    {
        alarm_info.insert("AlarmSrcType".into(), Value::from(alarm_source_type));
    }
    serde_json::to_vec(&value).map_err(|source| {
        runtime_error(
            "device_simulator.alarm.template_serialize_failed",
            format!("approved NVR alarm template could not be normalized: {source}"),
        )
    })
}

#[derive(Debug, Default)]
struct ImageFieldCounters {
    data: usize,
    size: usize,
    url: usize,
}

fn rewrite_json_value(
    key: Option<&str>,
    value: &mut Value,
    event_type: Option<&str>,
    embedded_image_limit: usize,
    image_fields: &mut ImageFieldCounters,
) -> Result<(), AlarmRuntimeError> {
    match value {
        Value::Object(map) => {
            if image_fields.data < embedded_image_limit
                && key != Some("VehicleImage")
                && map.get("Data").is_some_and(Value::is_string)
                && map.get("Size").is_some_and(Value::is_number)
            {
                if let Some(data) = map.get_mut("Data") {
                    *data = Value::String(image_base64_marker(image_fields.data)?.into());
                    image_fields.data += 1;
                }
                if let Some(size) = map.get_mut("Size") {
                    *size = Value::String(image_size_sentinel(image_fields.size)?.into());
                    image_fields.size += 1;
                }
                if let Some(url) = map.get_mut("URL").filter(|url| {
                    url.as_str()
                        .is_some_and(|url| url.contains("/System/Picture"))
                }) {
                    *url = Value::String(format!(
                        "/LAPI/V1.0/System/Picture?Type=1&Index=approved&Size={}",
                        image_size_marker(image_fields.url)?
                    ));
                    image_fields.url += 1;
                }
            }
            for (child_key, child) in map.iter_mut() {
                rewrite_json_value(
                    Some(child_key),
                    child,
                    event_type,
                    embedded_image_limit,
                    image_fields,
                )?;
            }
        }
        Value::Array(items) => {
            for item in items {
                rewrite_json_value(key, item, event_type, embedded_image_limit, image_fields)?;
            }
        }
        _ => {
            let Some(key) = key else { return Ok(()) };
            match key {
                "Reference" => *value = Value::String("{{reference}}".into()),
                "DeviceCode" | "DeviceID" | "DeviceId" | "DevID" | "SerialNumber" => {
                    *value = Value::String("{{device_id}}".into())
                }
                "DeviceIP" | "DevIP" | "IPAddr" | "IPAddress" => {
                    *value = Value::String("{{device_ip}}".into())
                }
                "TimeStamp" | "Timestamp" | "PassingTime" | "CaptureTime" => {
                    *value = dynamic_value(value, "{{timestamp}}", NUMERIC_TIMESTAMP_SENTINEL)
                }
                "Seq" | "AlarmSeq" | "ID" | "RecordID" | "RelatedID" => {
                    *value = dynamic_value(value, "{{subscription_id}}", NUMERIC_ID_SENTINEL)
                }
                "ChannelID" | "ChannelId" | "SrcID" => {
                    *value = dynamic_value(value, "{{channel_id}}", NUMERIC_CHANNEL_SENTINEL)
                }
                "AlarmType" if event_type.is_some() => {
                    *value = Value::String(event_type.expect("checked").into())
                }
                "Type" if value.is_string() && event_type.is_some() => {
                    *value = Value::String(event_type.expect("checked").into())
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn image_base64_marker(index: usize) -> Result<&'static str, AlarmRuntimeError> {
    [
        "{{image_base64}}",
        "{{image_base64_2}}",
        "{{image_base64_3}}",
        "{{image_base64_4}}",
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
    ]
    .get(index)
    .copied()
    .ok_or_else(|| image_slot_error(index))
}

fn image_slot_error(index: usize) -> AlarmRuntimeError {
    runtime_error(
        "device_simulator.alarm.image_slot_exceeded",
        format!("approved alarm template declares more than four image slots ({index})"),
    )
}

fn dynamic_value(current: &Value, string_marker: &str, numeric_sentinel: &str) -> Value {
    if current.is_number() {
        Value::String(numeric_sentinel.into())
    } else {
        Value::String(string_marker.into())
    }
}

fn select_pack_images(
    assets: &RuntimeAssetLayout,
    profile_id: FirstReleaseProfileId,
    image_root: Option<&str>,
    count: usize,
    alarm_type_id: &str,
) -> Result<Vec<ImageAssetRef>, AlarmRuntimeError> {
    if count == 0 {
        return Ok(vec![]);
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
    let indexes = match (profile_id, alarm_type_id, count) {
        (FirstReleaseProfileId::NvrVehicle, "snap", 2) => vec![0, 2],
        (FirstReleaseProfileId::IpcSmart, "falling", 1) => vec![1],
        _ => (0..count).collect(),
    };
    if indexes
        .iter()
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
    Ok(indexes
        .into_iter()
        .map(|index| ImageAssetRef::Pack {
            pack_id: pack.id.clone(),
            version: pack.version.clone(),
            path: paths[index].clone(),
        })
        .collect())
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
        image.file_name = format!("user-alarm.{extension}");
    }
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
    for image in &mut definition.images {
        let ImageAssetRef::Pack { pack_id, path, .. } = &mut image.reference else {
            continue;
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
            continue;
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
    }
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
    let pack_root = first
        .directory
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            runtime_error(
                "device_simulator.alarm.pack_root_invalid",
                "pinned pack directory is not under <root>/<id>/<version>",
            )
        })?;
    let mut manifests = BTreeMap::new();
    for pin in pins {
        let expected = pack_root.join(&pin.id).join(&pin.version);
        if expected != pin.directory {
            return Err(runtime_error(
                "device_simulator.alarm.pack_root_mismatch",
                "pinned packs do not share one immutable pack root",
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

fn build_destinations(
    target: &TargetPlatformConfig,
) -> Result<(BTreeMap<String, reqwest::Url>, Vec<String>), AlarmRuntimeError> {
    let mut destinations = BTreeMap::new();
    if let Some(url) = target
        .alarm_receiver_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        destinations.insert("configured".into(), normalize_destination_url(url)?);
    } else {
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
            destinations.insert(
                server.id.clone(),
                normalize_destination_url(&format!("http://{rendered_host}:{}/", server.port))?,
            );
        }
    }
    let ids = destinations.keys().cloned().collect();
    Ok((destinations, ids))
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
    definition: &AlarmHandlerDefinition,
    subscription_id: &str,
    destination: &reqwest::Url,
    device_http_port: u16,
) -> Result<AlarmBuildContext, AlarmRuntimeError> {
    let channel = device
        .preview
        .channel_count
        .map(|_| DEFAULT_CHANNEL_ID)
        .unwrap_or(DEFAULT_CHANNEL_ID);
    let device_authority = format!("{}:{device_http_port}", device.preview.ip);
    let destination_authority = destination_reference_authority(destination)?;
    let reference = match definition.profile_id {
        FirstReleaseProfileId::IpcCustom => {
            format!("{device_authority}/Subscription/Subscribers/{subscription_id}")
        }
        FirstReleaseProfileId::IpcSmart
            if matches!(
                &definition.transport.body_encoding,
                BodyEncoding::Multipart { .. }
            ) =>
        {
            format!("{device_authority}/Subscription/Subscribers/{subscription_id}")
        }
        FirstReleaseProfileId::IpcSmart => {
            format!("{destination_authority}/Subscription/Subscribers/{subscription_id}")
        }
        FirstReleaseProfileId::NvrCommon => format!(
            "{destination_authority}/{}/Subscription/Subscribers/{subscription_id}",
            device.preview.hardware_id
        ),
        FirstReleaseProfileId::NvrVehicle if definition.alarm_type_id.as_str() == "snap" => {
            format!(
                "{device_authority}/{}/Subscription/Subscribers/{subscription_id}",
                device.preview.hardware_id
            )
        }
        FirstReleaseProfileId::NvrVehicle => format!(
            "{destination_authority}/{}/Subscription/Subscribers/{subscription_id}",
            device.preview.hardware_id
        ),
    };
    let mut fields = BTreeMap::new();
    fields.insert(DynamicField::DeviceId, device.preview.hardware_id.clone());
    fields.insert(DynamicField::DeviceIp, device.preview.ip.to_string());
    fields.insert(DynamicField::ChannelId, channel.into());
    fields.insert(DynamicField::Timestamp, "0".into());
    fields.insert(DynamicField::Reference, reference);
    fields.insert(DynamicField::SubscriptionId, subscription_id.into());
    fields.insert(DynamicField::AlarmState, "alarm".into());
    Ok(AlarmBuildContext {
        source_ip: Some(device.preview.ip),
        fields,
        multipart_boundary: Some(format!("fst-simulator-{}", uuid::Uuid::new_v4().simple())),
    })
}

fn destination_reference_authority(
    destination: &reqwest::Url,
) -> Result<String, AlarmRuntimeError> {
    let host = destination.host_str().ok_or_else(|| {
        runtime_error(
            "device_simulator.alarm.destination_invalid",
            "alarm destination has no host for legacy Reference rendering",
        )
    })?;
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let port = destination.port_or_known_default().ok_or_else(|| {
        runtime_error(
            "device_simulator.alarm.destination_invalid",
            "alarm destination has no port for legacy Reference rendering",
        )
    })?;
    Ok(format!("{host}:{port}"))
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

fn trigger_result(snapshot: AlarmJobSnapshot, duration_ms: u64) -> AlarmTriggerResult {
    let mut errors = Vec::new();
    if let Some(code) = snapshot.last_error_code {
        errors.push(
            SimulatorErrorBody::new(code, "deviceSimulator.errors.alarmDispatchFailed")
                .retryable(false),
        );
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
        last_error: snapshot.last_error_code.map(|code| {
            SimulatorErrorBody::new(code, "deviceSimulator.errors.alarmDispatchFailed")
                .retryable(false)
        }),
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

fn parse_profile_id(value: &str) -> Result<FirstReleaseProfileId, AlarmRuntimeError> {
    match value {
        "ipc-custom" => Ok(FirstReleaseProfileId::IpcCustom),
        "ipc-smart" => Ok(FirstReleaseProfileId::IpcSmart),
        "nvr-common" => Ok(FirstReleaseProfileId::NvrCommon),
        "nvr-vehicle" => Ok(FirstReleaseProfileId::NvrVehicle),
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
        preview_devices, DeviceGroupDraft, RtspPorts, SimulatorStartRequest, StreamRuntimeConfig,
        StreamTransport, TargetPlatformServer,
    };
    use crate::device_simulator::runtime_assets::PinnedPackDirectory;
    use tempfile::TempDir;

    #[test]
    fn compiles_legacy_json_into_bounded_runtime_fields_without_executable_content() {
        let template = compile_json_template(
            br#"{
                "Reference":"legacy",
                "AlarmInfo":{"TimeStamp":1,"AlarmType":"Old","AlarmSeq":2},
                "Image":{"Size":0,"Data":"legacy","URL":"/LAPI/V1.0/System/Picture?Index=C:\\old.jpg"}
            }"#,
            Some("CrossLine"),
            usize::MAX,
        )
        .unwrap();
        assert!(template.fields().contains(&DynamicField::Reference));
        assert!(template.fields().contains(&DynamicField::Timestamp));
        assert!(template.fields().contains(&DynamicField::SubscriptionId));
        assert!(template.fields().contains(&DynamicField::ImageBase64));
        assert!(template.fields().contains(&DynamicField::ImageSize));
        let rendered = String::from_utf8(
            template
                .render(&BTreeMap::from([
                    (DynamicField::Reference, "ref".into()),
                    (DynamicField::Timestamp, "123".into()),
                    (DynamicField::SubscriptionId, "456".into()),
                    (DynamicField::ImageBase64, "YWJj".into()),
                    (DynamicField::ImageSize, "3".into()),
                ]))
                .unwrap(),
        )
        .unwrap();
        assert!(rendered.contains("\"TimeStamp\":123"));
        assert!(rendered.contains("\"AlarmType\":\"CrossLine\""));
        assert!(rendered.contains("\"Size\":3"));
        assert!(!rendered.contains("C:\\old.jpg"));
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
    fn configured_mode_requires_one_explicit_alarm_type() {
        assert!(scheduled_mode(AlarmDispatchMode::Configured, 0).is_err());
        assert_eq!(
            scheduled_mode(AlarmDispatchMode::Configured, 1).unwrap(),
            ScheduledDispatchMode::Specified
        );
    }

    #[test]
    fn approved_release_alarm_registry_loads_when_explicitly_configured() {
        let Ok(root) = std::env::var("FST_APPROVED_PACK_ROOT") else {
            return;
        };
        let root = PathBuf::from(root);
        let version = std::env::var("FST_APPROVED_PACK_VERSION").unwrap_or_else(|_| "1.0.2".into());
        let pins = [
            "protocol-core",
            "media-h264-live",
            "ipc-custom",
            "ipc-smart",
            "nvr-common",
            "nvr-vehicle",
        ]
        .into_iter()
        .map(|id| PinnedPackDirectory {
            id: id.into(),
            version: version.clone(),
            directory: root.join(id).join(&version),
        })
        .collect::<Vec<_>>();
        let profile_ids =
            ["ipc-custom", "ipc-smart", "nvr-common", "nvr-vehicle"].map(str::to_owned);
        let assets = Arc::new(RuntimeAssetLayout::load(&pins, &profile_ids).unwrap());
        let request = SimulatorStartRequest {
            platform: TargetPlatformConfig {
                kind: TargetPlatform::Vms,
                servers: vec![TargetPlatformServer {
                    id: "receiver".into(),
                    host: "127.0.0.1".into(),
                    port: 18080,
                }],
                alarm_receiver_url: None,
            },
            interface_id: "fixture-interface".into(),
            start_ip: "127.30.0.10".parse().unwrap(),
            subnet_prefix: 24,
            device_http_port: 18081,
            rtsp_ports: RtspPorts {
                main: 18554,
                sub: 18555,
                third: 18556,
            },
            groups: vec![
                DeviceGroupDraft {
                    id: "custom".into(),
                    profile_id: "ipc-custom".into(),
                    count: 1,
                    nvr_channel_count: None,
                },
                DeviceGroupDraft {
                    id: "smart".into(),
                    profile_id: "ipc-smart".into(),
                    count: 1,
                    nvr_channel_count: None,
                },
                DeviceGroupDraft {
                    id: "common".into(),
                    profile_id: "nvr-common".into(),
                    count: 1,
                    nvr_channel_count: Some(8),
                },
                DeviceGroupDraft {
                    id: "vehicle".into(),
                    profile_id: "nvr-vehicle".into(),
                    count: 1,
                    nvr_channel_count: Some(8),
                },
            ],
            stream: StreamRuntimeConfig {
                transport: StreamTransport::TcpInterleaved,
                enabled_streams: vec![
                    crate::device_simulator::api::DeviceSimulatorStreamKind::Main,
                    crate::device_simulator::api::DeviceSimulatorStreamKind::Sub,
                    crate::device_simulator::api::DeviceSimulatorStreamKind::Third,
                ],
                audio_enabled: false,
            },
        };
        let preview = preview_devices(&request).unwrap();
        let app_data = TempDir::new().unwrap();
        let runtime = AlarmRuntime::new(AlarmRuntimeConfig {
            platform: TargetPlatform::Vms,
            target: request.platform,
            preview,
            device_http_port: request.device_http_port,
            assets,
            app_data_dir: app_data.path().to_path_buf(),
        })
        .unwrap();
        assert!(runtime.registry.len() > 10);
        assert!(!runtime.image_cache.is_empty());
        let smart_motion = runtime
            .registry
            .definitions()
            .find(|definition| {
                definition.profile_id == FirstReleaseProfileId::IpcSmart
                    && definition.alarm_type_id.as_str() == "motion"
            })
            .unwrap();
        assert_eq!(
            smart_motion.transport.path,
            "/LAPI/V1.0/System/Event/Notification/Structure"
        );
        assert_eq!(smart_motion.follow_up_requests.len(), 1);
        assert_eq!(smart_motion.images.len(), 2);
        assert_ne!(
            smart_motion.images[0].reference,
            smart_motion.images[1].reference
        );
        assert_eq!(
            smart_motion.follow_up_requests[0].transport.path,
            "/LAPI/V1.0/System/Event/Notification/Alarm"
        );
        let smart_device = runtime
            .devices
            .values()
            .find(|device| device.profile_id == FirstReleaseProfileId::IpcSmart)
            .unwrap();
        let smart_requests = crate::device_simulator::alarms::build_alarm_requests(
            smart_motion,
            &build_context(
                smart_device,
                smart_motion,
                "123",
                runtime.destinations.get("receiver").unwrap(),
                runtime.device_http_port,
            )
            .unwrap(),
            &runtime.image_cache,
        )
        .unwrap();
        assert_eq!(smart_requests.len(), 2);
        assert_eq!(smart_requests[0].path, smart_motion.transport.path);
        assert_eq!(
            smart_requests[1].path,
            smart_motion.follow_up_requests[0].transport.path
        );
        let smart_alarm_body = String::from_utf8_lossy(&smart_requests[1].body);
        let smart_structure_body = String::from_utf8_lossy(&smart_requests[0].body);
        assert!(smart_structure_body.contains("127.0.0.1:18080/Subscription/Subscribers/123"));
        assert!(smart_alarm_body.contains("SmartMotionDetectOn"));
        assert!(!smart_alarm_body.contains("\"AlarmType\":\"motion\""));
        let smart_recovery = runtime
            .registry
            .definitions()
            .find(|definition| {
                definition.profile_id == FirstReleaseProfileId::IpcSmart
                    && definition.alarm_type_id.as_str() == "accesselevator"
            })
            .unwrap();
        let smart_recovery_request = crate::device_simulator::alarms::build_recovery_request(
            smart_recovery,
            &build_context(
                smart_device,
                smart_recovery,
                "124",
                runtime.destinations.get("receiver").unwrap(),
                runtime.device_http_port,
            )
            .unwrap(),
            &runtime.image_cache,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            smart_recovery_request.path,
            "/LAPI/V1.0/System/Event/Notification/Alarm"
        );
        let smart_recovery_body = String::from_utf8_lossy(&smart_recovery_request.body);
        assert!(smart_recovery_body.contains("AccessElevatorAlarmCleared"));
        assert!(!smart_recovery_body.contains("RelatedID"));
        assert!(!smart_recovery_body.contains("RelatedObjects"));
        let smart_falling = runtime
            .registry
            .definitions()
            .find(|definition| {
                definition.profile_id == FirstReleaseProfileId::IpcSmart
                    && definition.alarm_type_id.as_str() == "falling"
            })
            .unwrap();
        assert!(matches!(
            &smart_falling.images[0].reference,
            ImageAssetRef::Pack { path, .. } if path.ends_with("/1-2.jpg")
        ));
        let vehicle_match = runtime
            .registry
            .definitions()
            .find(|definition| {
                definition.profile_id == FirstReleaseProfileId::NvrVehicle
                    && definition.alarm_type_id.as_str() == "match"
            })
            .unwrap();
        assert_eq!(
            vehicle_match.transport.path,
            "/LAPI/V1.0/System/Event/Notification/VehicleEventInfo"
        );
        assert_eq!(vehicle_match.follow_up_requests.len(), 1);
        assert_eq!(vehicle_match.images.len(), 2);
        assert_ne!(
            vehicle_match.images[0].reference,
            vehicle_match.images[1].reference
        );
        assert!(vehicle_match
            .evidence
            .legacy_sources
            .iter()
            .any(|source| source.ends_with("MatchAlarm.json")));
        let vehicle_device = runtime
            .devices
            .values()
            .find(|device| device.profile_id == FirstReleaseProfileId::NvrVehicle)
            .unwrap();
        let vehicle_requests = crate::device_simulator::alarms::build_alarm_requests(
            vehicle_match,
            &build_context(
                vehicle_device,
                vehicle_match,
                "456",
                runtime.destinations.get("receiver").unwrap(),
                runtime.device_http_port,
            )
            .unwrap(),
            &runtime.image_cache,
        )
        .unwrap();
        assert_eq!(vehicle_requests.len(), 2);
        let vehicle_structure_body = String::from_utf8_lossy(&vehicle_requests[0].body);
        assert!(vehicle_structure_body.contains(&format!(
            "127.0.0.1:18080/{}/Subscription/Subscribers/456",
            vehicle_device.preview.hardware_id
        )));
        assert_eq!(
            vehicle_requests
                .iter()
                .map(|request| request.path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "/LAPI/V1.0/System/Event/Notification/VehicleEventInfo",
                "/LAPI/V1.0/System/Event/Notification/Alarm",
            ]
        );
        let vehicle_snap = runtime
            .registry
            .definitions()
            .find(|definition| {
                definition.profile_id == FirstReleaseProfileId::NvrVehicle
                    && definition.alarm_type_id.as_str() == "snap"
            })
            .unwrap();
        let snap_paths = vehicle_snap
            .images
            .iter()
            .filter_map(|image| match &image.reference {
                ImageAssetRef::Pack { path, .. } => Some(path.as_str()),
                ImageAssetRef::UserAsset { .. } => None,
            })
            .collect::<Vec<_>>();
        assert!(snap_paths[0].ends_with("/1-1.jpg"));
        assert!(snap_paths[1].ends_with("/1-3.jpg"));
        let custom_picture = runtime
            .registry
            .definitions()
            .find(|definition| {
                definition.profile_id == FirstReleaseProfileId::IpcCustom
                    && !definition.images.is_empty()
            })
            .unwrap();
        assert_eq!(custom_picture.images.len(), 4);
        assert_eq!(
            custom_picture.transport.path,
            "/LAPI/V1.1/System/Event/Notification/"
        );
        let common_device = runtime
            .devices
            .values()
            .find(|device| device.profile_id == FirstReleaseProfileId::NvrCommon)
            .unwrap();
        for (alarm_type_id, expected_source_type) in [("input-alarm-on", 9), ("disk-abnormal", 0)] {
            let definition = runtime
                .registry
                .definitions()
                .find(|definition| {
                    definition.profile_id == FirstReleaseProfileId::NvrCommon
                        && definition.alarm_type_id.as_str() == alarm_type_id
                })
                .unwrap();
            let request = crate::device_simulator::alarms::build_alarm_request(
                definition,
                &build_context(
                    common_device,
                    definition,
                    "789",
                    runtime.destinations.get("receiver").unwrap(),
                    runtime.device_http_port,
                )
                .unwrap(),
                &runtime.image_cache,
            )
            .unwrap();
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(
                body["AlarmInfo"]["AlarmSrcType"].as_i64(),
                Some(expected_source_type)
            );
        }
        let mut ums_custom_registry = AlarmHandlerRegistry::default();
        register_profile_definitions(
            &mut ums_custom_registry,
            &runtime.assets,
            FirstReleaseProfileId::IpcCustom,
            TargetPlatform::Ums,
        )
        .unwrap();
        let ums_custom_picture = ums_custom_registry
            .definitions()
            .find(|definition| !definition.images.is_empty())
            .unwrap();
        assert_eq!(ums_custom_picture.images.len(), 1);
        assert_eq!(
            ums_custom_picture.transport.path,
            "/LAPI/V1.1/System/Event/Notification"
        );
        let mut pictured = runtime
            .registry
            .definitions()
            .find(|definition| !definition.images.is_empty())
            .cloned()
            .unwrap();
        apply_image_variant(&mut pictured, Some("small"), &runtime.assets).unwrap();
        let selected = &pictured.images[0].reference;
        assert!(matches!(
            selected,
            ImageAssetRef::Pack { path, .. } if path.contains("/small/")
        ));
        runtime.image_cache.get(selected).unwrap();
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
