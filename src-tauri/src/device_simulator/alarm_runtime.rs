use crate::device_simulator::alarms::scheduler::{
    AlarmClock, AlarmDeviceTarget, AlarmDispatchMode as ScheduledDispatchMode, AlarmFuture,
    AlarmInvocation, AlarmJobSnapshot, AlarmScheduler, AlarmSchedulerLimits, AlarmSendError,
    AlarmSender, AlarmSenderResponse, OneShotAlarmJob, OutboundAlarmRequest, PeriodicAlarmJob,
    RunningAlarmJob, ScheduledAlarmJobState, SystemAlarmClock,
};
use crate::device_simulator::alarms::{
    AlarmBuildContext, AlarmHandlerDefinition, AlarmHandlerId, AlarmHandlerRegistry, AlarmTypeId,
    BodyEncoding, CompiledTemplate, DynamicField, FixtureProvenance, HandlerEvidence, HttpMethod,
    ImageAssetRef, ImageAttachmentDefinition, ImageCache, ImageExtension, ImagePolicy,
    PackIdentity, PlatformEvidence, PlatformVerification, RecoveryDefinition, RecoveryTrigger,
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
    destination_ids: Vec<String>,
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
        let sender = Arc::new(HttpAlarmSender::new(destinations));
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
            destination_ids,
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
            let mut invocations = Vec::with_capacity(definitions.len());
            for mut definition in definitions {
                if let Some(user_image) = &user_image {
                    if apply_user_image(&mut definition, user_image) {
                        user_image_applied = true;
                    }
                } else {
                    apply_image_variant(&mut definition, image_variant, &self.assets)?;
                }
                invocations.push(AlarmInvocation {
                    definition: Arc::new(definition),
                    context: build_context(device, &subscription_id),
                    image_cache: Arc::clone(&job_image_cache),
                });
            }
            targets.push(AlarmDeviceTarget {
                device_id: device_id.clone(),
                destination_id: self.destination_ids[index % self.destination_ids.len()].clone(),
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
    let image_reference = if definition.supports_pictures {
        select_pack_image(assets, profile_id, definition.image_root.as_deref())?
    } else {
        None
    };
    let raw_embedded_image = image_reference.is_some() && !multipart;
    let template_bytes = assets
        .read_from_pack(
            profile_id.as_str(),
            &normalize_runtime_path(selected_template),
        )
        .map_err(|source| runtime_error(source.code, source.message))?;
    let template =
        compile_json_template(&template_bytes, &definition.event_type, raw_embedded_image)?;
    let images = image_reference
        .clone()
        .map(|reference| {
            vec![ImageAttachmentDefinition {
                reference,
                field_name: if profile_id == FirstReleaseProfileId::IpcCustom {
                    "imageindex1"
                } else {
                    "image"
                }
                .into(),
                file_name: "approved-alarm.jpg".into(),
            }]
        })
        .unwrap_or_default();
    let image_policy = match (profile_id, images.is_empty()) {
        (FirstReleaseProfileId::NvrCommon, _) => ImagePolicy::Forbidden,
        (_, false) => ImagePolicy::Required,
        (_, true) => ImagePolicy::Forbidden,
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
            template: compile_json_template(&bytes, recovery_event, false)?,
            trigger: RecoveryTrigger::RequestedDelay,
            include_images: false,
        }
    } else {
        RecoveryDefinition::None
    };
    let mut intentional_changes =
        vec!["HTTP response success remains unverified until real-platform acceptance".into()];
    if selected_is_structure && definition.alarm_template.is_some() {
        intentional_changes.push(
            "The first local runtime sends the approved structure fixture as the primary logical alarm; compound multi-request platform semantics remain under real-platform verification".into(),
        );
    }
    if let Some(source_type) = &definition.source_type {
        intentional_changes.push(format!("legacy source type: {source_type}"));
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
        recovery,
        evidence: HandlerEvidence {
            legacy_sources: vec![definition.evidence.source, selected_template.clone()],
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
    event_type: &str,
    embed_image: bool,
) -> Result<CompiledTemplate, AlarmRuntimeError> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|source| {
        runtime_error(
            "device_simulator.alarm.template_json_invalid",
            format!("approved alarm template is not valid JSON: {source}"),
        )
    })?;
    rewrite_json_value(None, &mut value, event_type, embed_image);
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
    ] {
        rendered = rendered.replace(&format!("\"{sentinel}\""), marker);
    }
    CompiledTemplate::compile(rendered.as_bytes())
        .map_err(|source| runtime_error(source.code, source.message))
}

fn rewrite_json_value(key: Option<&str>, value: &mut Value, event_type: &str, embed_image: bool) {
    match value {
        Value::Object(map) => {
            for (child_key, child) in map.iter_mut() {
                rewrite_json_value(Some(child_key), child, event_type, embed_image);
            }
        }
        Value::Array(items) => {
            for item in items {
                rewrite_json_value(key, item, event_type, embed_image);
            }
        }
        _ => {
            let Some(key) = key else { return };
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
                "AlarmType" => *value = Value::String(event_type.into()),
                "Type" if value.is_string() => *value = Value::String(event_type.into()),
                "Data" if embed_image && value.is_string() => {
                    *value = Value::String("{{image_base64}}".into())
                }
                "Size" if embed_image && value.is_number() => {
                    *value = Value::String(NUMERIC_IMAGE_SIZE_SENTINEL.into())
                }
                "URL"
                    if embed_image
                        && value
                            .as_str()
                            .is_some_and(|url| url.contains("/System/Picture")) =>
                {
                    *value = Value::String(
                        "/LAPI/V1.0/System/Picture?Type=1&Index=approved&Size={{image_size}}"
                            .into(),
                    )
                }
                _ => {}
            }
        }
    }
}

fn dynamic_value(current: &Value, string_marker: &str, numeric_sentinel: &str) -> Value {
    if current.is_number() {
        Value::String(numeric_sentinel.into())
    } else {
        Value::String(string_marker.into())
    }
}

fn select_pack_image(
    assets: &RuntimeAssetLayout,
    profile_id: FirstReleaseProfileId,
    image_root: Option<&str>,
) -> Result<Option<ImageAssetRef>, AlarmRuntimeError> {
    let Some(image_root) = image_root else {
        return Ok(None);
    };
    let image_root = normalize_runtime_path(image_root);
    let path = assets
        .declared_files_under(profile_id.as_str(), &image_root)
        .map_err(|source| runtime_error(source.code, source.message))?
        .into_iter()
        .find(|path| {
            let lower = path.to_ascii_lowercase();
            lower.ends_with(".jpg") || lower.ends_with(".jpeg") || lower.ends_with(".png")
        })
        .ok_or_else(|| {
            runtime_error(
                "device_simulator.alarm.image_missing",
                format!("approved image root '{image_root}' contains no image"),
            )
        })?;
    let pack = assets.pack(profile_id.as_str()).ok_or_else(|| {
        runtime_error(
            "device_simulator.alarm.profile_pack_missing",
            format!("profile pack '{}' is not pinned", profile_id.as_str()),
        )
    })?;
    Ok(Some(ImageAssetRef::Pack {
        pack_id: pack.id.clone(),
        version: pack.version.clone(),
        path,
    }))
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

fn build_context(device: &RuntimeAlarmDevice, subscription_id: &str) -> AlarmBuildContext {
    let channel = device
        .preview
        .channel_count
        .map(|_| DEFAULT_CHANNEL_ID)
        .unwrap_or(DEFAULT_CHANNEL_ID);
    let mut fields = BTreeMap::new();
    fields.insert(DynamicField::DeviceId, device.preview.hardware_id.clone());
    fields.insert(DynamicField::DeviceIp, device.preview.ip.to_string());
    fields.insert(DynamicField::ChannelId, channel.into());
    fields.insert(DynamicField::Timestamp, "0".into());
    fields.insert(
        DynamicField::Reference,
        format!(
            "{}:{}/Subscription/Subscribers/{subscription_id}",
            device.preview.ip, 81
        ),
    );
    fields.insert(DynamicField::SubscriptionId, subscription_id.into());
    fields.insert(DynamicField::AlarmState, "alarm".into());
    AlarmBuildContext {
        source_ip: Some(device.preview.ip),
        fields,
        multipart_boundary: Some(format!("fst-simulator-{}", uuid::Uuid::new_v4().simple())),
    }
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
            "CrossLine",
            true,
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
            assets,
            app_data_dir: app_data.path().to_path_buf(),
        })
        .unwrap();
        assert!(runtime.registry.len() > 10);
        assert!(!runtime.image_cache.is_empty());
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
