#[cfg(test)]
use super::ImageCache;
use super::{
    build_alarm_requests, build_recovery_request, event_image_references, AlarmBuildContext,
    AlarmError, AlarmHandlerDefinition, AlarmResult, HttpAlarmRequest, LegacyAlarmValues,
    RecoveryDefinition, ResponseSuccessRule, SharedImageCache,
};
use crate::device_simulator::profiles::scope::TargetPlatform;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Notify, OwnedSemaphorePermit, Semaphore};
use tokio::task::{JoinHandle, JoinSet};

pub const MAX_ALARM_TARGETS: usize = 2_048;
pub const MAX_ALARM_RETRY_ATTEMPTS: u8 = 5;
pub const MAX_ALARM_REQUEST_TIMEOUT_MS: u64 = 60_000;
pub const MAX_ALARM_INTERVAL_MS: u64 = 24 * 60 * 60 * 1_000;

// A Worker is recreated for each simulator session. Starting every Worker at
// event 1 makes a platform treat a later session's structured objects as
// updates to the previous session. Keep IDs in the legacy numeric range while
// choosing a new high-entropy starting point for every Worker process.
static NEXT_ALARM_EVENT_ID: OnceLock<AtomicU64> = OnceLock::new();

pub type AlarmFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmSenderResponse {
    pub status: u16,
    /// Absolute URL the request actually reached, reported back so a rejected
    /// status names the endpoint that rejected it.
    pub endpoint: Option<String>,
    /// Application-level acknowledgement decoded from the platform response
    /// body. HTTP 2xx alone only proves that an HTTP handler answered; UMS may
    /// still reject the alarm in its JSON response envelope.
    pub application_status: Option<AlarmApplicationStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlarmApplicationStatus {
    Accepted,
    Rejected { details: String },
}

impl AlarmSenderResponse {
    pub fn new(status: u16) -> Self {
        Self {
            status,
            endpoint: None,
            application_status: None,
        }
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    pub fn with_application_status(mut self, status: AlarmApplicationStatus) -> Self {
        self.application_status = Some(status);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmSendError {
    pub code: String,
    pub retryable: bool,
    /// Sanitized, operator-facing cause (destination, transport reason). Carried
    /// all the way to the UI so a failure names what actually went wrong instead
    /// of only a generic "the alarm could not be sent".
    pub details: Option<String>,
}

impl AlarmSendError {
    pub fn new(code: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.into(),
            retryable,
            details: None,
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct OutboundAlarmRequest {
    pub destination_id: String,
    pub device_id: String,
    pub phase: AlarmDeliveryPhase,
    /// Includes the required per-device source IP. Implementations must bind
    /// the outbound socket to this address before connecting.
    pub request: HttpAlarmRequest,
}

pub trait AlarmSender: Send + Sync + 'static {
    fn send(
        &self,
        request: OutboundAlarmRequest,
    ) -> AlarmFuture<'_, Result<AlarmSenderResponse, AlarmSendError>>;
}

pub trait AlarmClock: Send + Sync + 'static {
    fn now_ms(&self) -> u64;
    fn sleep(&self, duration: Duration) -> AlarmFuture<'_, ()>;
}

#[derive(Debug)]
pub struct SystemAlarmClock {
    origin: Instant,
}

impl Default for SystemAlarmClock {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl AlarmClock for SystemAlarmClock {
    fn now_ms(&self) -> u64 {
        self.origin.elapsed().as_millis().min(u64::MAX as u128) as u64
    }

    fn sleep(&self, duration: Duration) -> AlarmFuture<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

#[derive(Debug, Clone, Default)]
pub struct AlarmCancellation {
    inner: Arc<CancellationInner>,
}

#[derive(Debug, Default)]
struct CancellationInner {
    cancelled: AtomicBool,
    notify: Notify,
}

impl AlarmCancellation {
    pub fn cancel(&self) {
        if !self.inner.cancelled.swap(true, Ordering::AcqRel) {
            self.inner.notify.notify_waiters();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let notified = self.inner.notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlarmDispatchMode {
    /// Exactly one invocation must be provided for each device.
    Specified,
    Random,
    Sequential,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlarmDeliveryPhase {
    Alarm,
    Recovery,
}

#[derive(Debug, Clone)]
pub struct AlarmInvocation {
    pub definition: Arc<AlarmHandlerDefinition>,
    pub context: AlarmBuildContext,
    pub image_cache: SharedImageCache,
}

#[derive(Debug, Clone)]
pub struct AlarmDeviceTarget {
    pub device_id: String,
    pub destination_id: String,
    pub platform: TargetPlatform,
    pub invocations: Vec<AlarmInvocation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmRetryPolicy {
    /// Includes the first attempt. Zero and values above five are rejected.
    pub max_attempts: u8,
    pub backoff_ms: u64,
    pub retryable_statuses: BTreeSet<u16>,
}

impl Default for AlarmRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 2,
            backoff_ms: 250,
            retryable_statuses: BTreeSet::from([408, 425, 429, 500, 502, 503, 504]),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmSchedulerLimits {
    pub global_max_in_flight: usize,
    pub per_destination_max_in_flight: usize,
    pub max_queued_devices: usize,
    pub global_rate_per_second: u32,
    pub per_destination_rate_per_second: u32,
    pub request_timeout_ms: u64,
    pub shutdown_grace_ms: u64,
    pub retry: AlarmRetryPolicy,
}

impl Default for AlarmSchedulerLimits {
    fn default() -> Self {
        Self {
            global_max_in_flight: 32,
            per_destination_max_in_flight: 8,
            max_queued_devices: 1_024,
            global_rate_per_second: 100,
            per_destination_rate_per_second: 50,
            request_timeout_ms: 10_000,
            shutdown_grace_ms: 5_000,
            retry: AlarmRetryPolicy::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PeriodicAlarmJob {
    pub job_id: String,
    pub targets: Vec<AlarmDeviceTarget>,
    pub mode: AlarmDispatchMode,
    pub interval_ms: u64,
    /// `None` alone means continuous operation. `Some(0)` is invalid.
    pub send_count: Option<u64>,
    pub recovery_delay_ms: Option<u64>,
    pub random_seed: u64,
}

#[derive(Debug, Clone)]
pub struct OneShotAlarmJob {
    pub job_id: String,
    pub targets: Vec<AlarmDeviceTarget>,
    pub mode: AlarmDispatchMode,
    pub recovery_delay_ms: Option<u64>,
    pub random_seed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduledAlarmJobState {
    Starting,
    Running,
    Stopping,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceAlarmStats {
    pub attempted: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub unverified: u64,
    pub timed_out: u64,
    pub cancelled: u64,
    pub rejected: u64,
    pub in_flight: u64,
    pub total_duration_ms: u64,
    pub last_http_status: Option<u16>,
    pub last_error_code: Option<String>,
    pub last_error_details: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmJobSnapshot {
    pub job_id: String,
    pub state: ScheduledAlarmJobState,
    pub attempted: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub unverified: u64,
    pub timed_out: u64,
    pub cancelled: u64,
    pub rejected: u64,
    pub in_flight: u64,
    pub average_duration_ms: u64,
    pub last_http_status: Option<u16>,
    pub last_error_code: Option<String>,
    pub last_error_details: Option<String>,
    pub devices: BTreeMap<String, DeviceAlarmStats>,
}

#[derive(Debug)]
struct JobStatsInner {
    state: ScheduledAlarmJobState,
    last_http_status: Option<u16>,
    devices: BTreeMap<String, DeviceAlarmStats>,
}

#[derive(Debug, Clone)]
pub struct AlarmJobTracker {
    job_id: Arc<str>,
    inner: Arc<Mutex<JobStatsInner>>,
}

impl AlarmJobTracker {
    fn new(job_id: String, targets: &[AlarmDeviceTarget]) -> Self {
        Self {
            job_id: Arc::from(job_id),
            inner: Arc::new(Mutex::new(JobStatsInner {
                state: ScheduledAlarmJobState::Starting,
                last_http_status: None,
                devices: targets
                    .iter()
                    .map(|target| (target.device_id.clone(), DeviceAlarmStats::default()))
                    .collect(),
            })),
        }
    }

    pub async fn snapshot(&self) -> AlarmJobSnapshot {
        let inner = self.inner.lock().await;
        snapshot_from_inner(&self.job_id, &inner)
    }

    async fn set_state(&self, state: ScheduledAlarmJobState) {
        self.inner.lock().await.state = state;
    }

    async fn begin_attempt(&self, device_id: &str) {
        let mut inner = self.inner.lock().await;
        let stats = inner.devices.entry(device_id.to_owned()).or_default();
        stats.attempted = stats.attempted.saturating_add(1);
        stats.in_flight = stats.in_flight.saturating_add(1);
    }

    async fn finish_attempt(&self, device_id: &str, outcome: AttemptResult, duration_ms: u64) {
        let last_http_status = outcome.http_status();
        let mut inner = self.inner.lock().await;
        inner.last_http_status = last_http_status;
        let stats = inner.devices.entry(device_id.to_owned()).or_default();
        stats.in_flight = stats.in_flight.saturating_sub(1);
        stats.total_duration_ms = stats.total_duration_ms.saturating_add(duration_ms);
        stats.last_http_status = last_http_status;
        match outcome {
            AttemptResult::Succeeded { .. } => {
                stats.succeeded = stats.succeeded.saturating_add(1);
                stats.last_error_code = None;
                stats.last_error_details = None;
            }
            AttemptResult::Unverified { .. } => {
                stats.unverified = stats.unverified.saturating_add(1);
                stats.last_error_code = None;
                stats.last_error_details = None;
            }
            AttemptResult::Failed {
                code,
                details,
                timed_out,
                ..
            } => {
                stats.failed = stats.failed.saturating_add(1);
                stats.timed_out = stats.timed_out.saturating_add(u64::from(timed_out));
                stats.last_error_code = Some(code);
                stats.last_error_details = details;
            }
            AttemptResult::Cancelled => {
                stats.cancelled = stats.cancelled.saturating_add(1);
                stats.last_error_code = Some("device_simulator.alarm.cancelled".into());
                stats.last_error_details = None;
            }
        }
    }

    async fn reject(&self, device_id: &str, code: impl Into<String>) {
        let mut inner = self.inner.lock().await;
        let stats = inner.devices.entry(device_id.to_owned()).or_default();
        stats.rejected = stats.rejected.saturating_add(1);
        stats.last_error_code = Some(code.into());
        stats.last_error_details = None;
    }
}

fn snapshot_from_inner(job_id: &str, inner: &JobStatsInner) -> AlarmJobSnapshot {
    let mut attempted = 0_u64;
    let mut succeeded = 0_u64;
    let mut failed = 0_u64;
    let mut unverified = 0_u64;
    let mut timed_out = 0_u64;
    let mut cancelled = 0_u64;
    let mut rejected = 0_u64;
    let mut in_flight = 0_u64;
    let mut total_duration = 0_u64;
    let mut last_error_code = None;
    let mut last_error_details = None;
    for stats in inner.devices.values() {
        attempted = attempted.saturating_add(stats.attempted);
        succeeded = succeeded.saturating_add(stats.succeeded);
        failed = failed.saturating_add(stats.failed);
        unverified = unverified.saturating_add(stats.unverified);
        timed_out = timed_out.saturating_add(stats.timed_out);
        cancelled = cancelled.saturating_add(stats.cancelled);
        rejected = rejected.saturating_add(stats.rejected);
        in_flight = in_flight.saturating_add(stats.in_flight);
        total_duration = total_duration.saturating_add(stats.total_duration_ms);
        if stats.last_error_code.is_some() {
            last_error_code.clone_from(&stats.last_error_code);
            last_error_details.clone_from(&stats.last_error_details);
        }
    }
    AlarmJobSnapshot {
        job_id: job_id.to_owned(),
        state: inner.state,
        attempted,
        succeeded,
        failed,
        unverified,
        timed_out,
        cancelled,
        rejected,
        in_flight,
        average_duration_ms: if attempted == 0 {
            0
        } else {
            total_duration / attempted
        },
        last_http_status: inner.last_http_status,
        last_error_code,
        last_error_details,
        devices: inner.devices.clone(),
    }
}

pub struct RunningAlarmJob {
    tracker: AlarmJobTracker,
    cancellation: AlarmCancellation,
    task: JoinHandle<AlarmResult<AlarmJobSnapshot>>,
    clock: Arc<dyn AlarmClock>,
    shutdown_grace_ms: u64,
}

impl RunningAlarmJob {
    pub async fn stop(&self) {
        self.tracker
            .set_state(ScheduledAlarmJobState::Stopping)
            .await;
        self.cancellation.cancel();
    }

    pub fn tracker(&self) -> AlarmJobTracker {
        self.tracker.clone()
    }

    pub async fn wait(self) -> AlarmResult<AlarmJobSnapshot> {
        join_job_task(self.task.await)
    }

    /// Stops admission immediately, cancels pending/in-flight sends, and adds
    /// a final hard bound in case a future scheduler extension fails to honor
    /// cancellation cooperatively.
    pub async fn stop_and_wait(mut self) -> AlarmResult<AlarmJobSnapshot> {
        self.stop().await;
        tokio::select! {
            biased;
            result = &mut self.task => join_job_task(result),
            _ = self.clock.sleep(Duration::from_millis(self.shutdown_grace_ms)) => {
                self.task.abort();
                let _ = self.task.await;
                self.tracker.set_state(ScheduledAlarmJobState::Cancelled).await;
                Ok(self.tracker.snapshot().await)
            }
        }
    }
}

fn join_job_task(
    result: Result<AlarmResult<AlarmJobSnapshot>, tokio::task::JoinError>,
) -> AlarmResult<AlarmJobSnapshot> {
    result.map_err(|error| {
        AlarmError::new(
            "device_simulator.alarm.job_task_failed",
            format!("alarm job task failed: {error}"),
        )
    })?
}

#[derive(Clone)]
pub struct AlarmScheduler {
    sender: Arc<dyn AlarmSender>,
    clock: Arc<dyn AlarmClock>,
    limits: AlarmSchedulerLimits,
    global_semaphore: Arc<Semaphore>,
    global_rate: Arc<RateGate>,
    destinations: Arc<Mutex<BTreeMap<String, Arc<DestinationControl>>>>,
}

impl AlarmScheduler {
    pub fn new(
        sender: Arc<dyn AlarmSender>,
        clock: Arc<dyn AlarmClock>,
        limits: AlarmSchedulerLimits,
    ) -> AlarmResult<Self> {
        validate_limits(&limits)?;
        Ok(Self {
            global_semaphore: Arc::new(Semaphore::new(limits.global_max_in_flight)),
            global_rate: Arc::new(RateGate::new(limits.global_rate_per_second)),
            sender,
            clock,
            limits,
            destinations: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    pub async fn trigger_once(&self, job: OneShotAlarmJob) -> AlarmResult<AlarmJobSnapshot> {
        validate_job(
            &job.job_id,
            &job.targets,
            job.mode,
            job.recovery_delay_ms,
            &self.limits,
        )?;
        let tracker = AlarmJobTracker::new(job.job_id, &job.targets);
        let cancellation = AlarmCancellation::default();
        self.run_targets(
            job.targets,
            job.mode,
            Some(1),
            job.mode == AlarmDispatchMode::Sequential,
            None,
            job.recovery_delay_ms,
            job.random_seed,
            tracker,
            cancellation,
        )
        .await
    }

    pub fn start_periodic(&self, job: PeriodicAlarmJob) -> AlarmResult<RunningAlarmJob> {
        if job.interval_ms == 0 || job.interval_ms > MAX_ALARM_INTERVAL_MS {
            return Err(AlarmError::new(
                "device_simulator.alarm.interval_invalid",
                "periodic alarm interval must be between 1 ms and 24 hours",
            ));
        }
        if job.send_count == Some(0) {
            return Err(AlarmError::new(
                "device_simulator.alarm.send_count_invalid",
                "send_count zero is invalid; None alone means continuous operation",
            ));
        }
        validate_job(
            &job.job_id,
            &job.targets,
            job.mode,
            job.recovery_delay_ms,
            &self.limits,
        )?;
        let tracker = AlarmJobTracker::new(job.job_id, &job.targets);
        let cancellation = AlarmCancellation::default();
        let scheduler = self.clone();
        let task_tracker = tracker.clone();
        let task_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            scheduler
                .run_targets(
                    job.targets,
                    job.mode,
                    job.send_count,
                    false,
                    Some(job.interval_ms),
                    job.recovery_delay_ms,
                    job.random_seed,
                    task_tracker,
                    task_cancellation,
                )
                .await
        });
        Ok(RunningAlarmJob {
            tracker,
            cancellation,
            task,
            clock: self.clock.clone(),
            shutdown_grace_ms: self.limits.shutdown_grace_ms,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_targets(
        &self,
        targets: Vec<AlarmDeviceTarget>,
        mode: AlarmDispatchMode,
        send_count: Option<u64>,
        send_each_invocation_once: bool,
        interval_ms: Option<u64>,
        recovery_delay_ms: Option<u64>,
        random_seed: u64,
        tracker: AlarmJobTracker,
        cancellation: AlarmCancellation,
    ) -> AlarmResult<AlarmJobSnapshot> {
        tracker.set_state(ScheduledAlarmJobState::Running).await;
        let mut tasks = JoinSet::new();
        for (index, target) in targets.into_iter().enumerate() {
            let scheduler = self.clone();
            let tracker = tracker.clone();
            let cancellation = cancellation.clone();
            let target_send_count = if send_each_invocation_once {
                Some(target.invocations.len() as u64)
            } else {
                send_count
            };
            tasks.spawn(async move {
                scheduler
                    .run_device(
                        target,
                        mode,
                        target_send_count,
                        interval_ms,
                        recovery_delay_ms,
                        random_seed ^ index as u64,
                        tracker,
                        cancellation,
                    )
                    .await
            });
        }

        while let Some(result) = tasks.join_next().await {
            if let Err(error) = result {
                cancellation.cancel();
                tracker.set_state(ScheduledAlarmJobState::Failed).await;
                return Err(AlarmError::new(
                    "device_simulator.alarm.device_task_failed",
                    format!("alarm device task failed: {error}"),
                ));
            }
        }
        tracker
            .set_state(if cancellation.is_cancelled() {
                ScheduledAlarmJobState::Cancelled
            } else {
                ScheduledAlarmJobState::Completed
            })
            .await;
        Ok(tracker.snapshot().await)
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_device(
        &self,
        target: AlarmDeviceTarget,
        mode: AlarmDispatchMode,
        send_count: Option<u64>,
        interval_ms: Option<u64>,
        recovery_delay_ms: Option<u64>,
        random_seed: u64,
        tracker: AlarmJobTracker,
        cancellation: AlarmCancellation,
    ) {
        let mut round = 0_u64;
        let mut random = DeterministicRandom::new(random_seed);
        let mut invocation_cycle = ShuffledCycle::default();
        // Image materials rotate independently for each alarm type. Using the
        // global event ID here skips groups whenever the number of alarm types
        // and the number of material groups share a divisor (for example four
        // types with six car groups).
        let mut image_group_sequences = vec![0_u64; target.invocations.len()];
        let mut image_group_cycles = vec![ShuffledCycle::default(); target.invocations.len()];
        while send_count.map_or(true, |limit| round < limit) && !cancellation.is_cancelled() {
            let invocation_index = match mode {
                AlarmDispatchMode::Specified => 0,
                AlarmDispatchMode::Sequential => round as usize % target.invocations.len(),
                AlarmDispatchMode::Random => {
                    invocation_cycle.next(target.invocations.len(), &mut random)
                }
            };
            let invocation = &target.invocations[invocation_index];
            let image_group_count = 1 + invocation.definition.alternate_images.len();
            let image_group_sequence = match mode {
                AlarmDispatchMode::Random => {
                    image_group_cycles[invocation_index].next(image_group_count, &mut random) as u64
                }
                AlarmDispatchMode::Specified | AlarmDispatchMode::Sequential => {
                    let sequence = image_group_sequences[invocation_index];
                    image_group_sequences[invocation_index] = sequence.saturating_add(1);
                    sequence
                }
            };
            let legacy_values = LegacyAlarmValues::new(random.next_u64());
            let event_id = self.next_event_id();
            let alarm_succeeded = self
                .deliver_invocation(
                    &target,
                    invocation,
                    AlarmDeliveryPhase::Alarm,
                    &legacy_values,
                    event_id,
                    image_group_sequence,
                    &tracker,
                    &cancellation,
                )
                .await;

            if alarm_succeeded
                && recovery_delay_ms.is_some()
                && !matches!(invocation.definition.recovery, RecoveryDefinition::None)
            {
                if !sleep_or_cancel(
                    self.clock.as_ref(),
                    Duration::from_millis(recovery_delay_ms.unwrap_or_default()),
                    &cancellation,
                )
                .await
                {
                    break;
                }
                self.deliver_invocation(
                    &target,
                    invocation,
                    AlarmDeliveryPhase::Recovery,
                    &legacy_values,
                    event_id,
                    image_group_sequence,
                    &tracker,
                    &cancellation,
                )
                .await;
            }

            round = round.saturating_add(1);
            if send_count.is_some_and(|limit| round >= limit) {
                break;
            }
            if let Some(interval_ms) = interval_ms {
                if !sleep_or_cancel(
                    self.clock.as_ref(),
                    Duration::from_millis(interval_ms),
                    &cancellation,
                )
                .await
                {
                    break;
                }
            }
        }
    }

    async fn deliver_invocation(
        &self,
        target: &AlarmDeviceTarget,
        invocation: &AlarmInvocation,
        phase: AlarmDeliveryPhase,
        legacy_values: &LegacyAlarmValues,
        event_id: u64,
        image_group_sequence: u64,
        tracker: &AlarmJobTracker,
        cancellation: &AlarmCancellation,
    ) -> bool {
        let mut context = invocation.context.clone();
        let now = chrono::Local::now();
        let timestamp = now.timestamp().to_string();
        context.fields.insert(
            crate::device_simulator::alarms::DynamicField::Timestamp,
            timestamp,
        );
        context.fields.insert(
            crate::device_simulator::alarms::DynamicField::CaptureTime,
            now.format("%Y%m%d%H%M%S%3f").to_string(),
        );
        context.fields.insert(
            crate::device_simulator::alarms::DynamicField::CaptureTimeText,
            now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        );
        context.fields.insert(
            crate::device_simulator::alarms::DynamicField::EventId,
            event_id.to_string(),
        );
        context.fields.insert(
            crate::device_simulator::alarms::DynamicField::ImageGroupSequence,
            image_group_sequence.to_string(),
        );
        context.fields.insert(
            crate::device_simulator::alarms::DynamicField::RelatedId,
            legacy_values.related_id(),
        );
        context.legacy_values = Some(legacy_values.clone());
        let definition = Arc::clone(&invocation.definition);
        let image_cache = Arc::clone(&invocation.image_cache);
        let requests = tokio::task::spawn_blocking(move || {
            let references = event_image_references(&definition, &context);
            let mut image_cache = image_cache.write();
            image_cache.ensure_cached(references)?;
            match phase {
                AlarmDeliveryPhase::Alarm => {
                    build_alarm_requests(&definition, &context, &image_cache)
                }
                AlarmDeliveryPhase::Recovery => {
                    build_recovery_request(&definition, &context, &image_cache)
                        .map(|request| request.into_iter().collect())
                }
            }
        })
        .await
        .map_err(|error| {
            AlarmError::new(
                "device_simulator.alarm.image_cache_task_failed",
                format!("alarm image cache task failed: {error}"),
            )
        })
        .and_then(|result| result);
        let requests = match requests {
            Ok(requests) => requests,
            Err(error) => {
                tracker.reject(&target.device_id, error.code).await;
                return false;
            }
        };

        for request in requests {
            for attempt in 1..=self.limits.retry.max_attempts {
                let outcome = self
                    .attempt(
                        target,
                        invocation,
                        phase,
                        request.clone(),
                        tracker,
                        cancellation,
                    )
                    .await;
                match outcome {
                    AttemptResult::Succeeded { .. } | AttemptResult::Unverified { .. } => break,
                    AttemptResult::Cancelled => return false,
                    AttemptResult::Failed {
                        retryable: false, ..
                    } => break,
                    AttemptResult::Failed { .. } if attempt >= self.limits.retry.max_attempts => {
                        break;
                    }
                    AttemptResult::Failed { .. } => {
                        let backoff = self
                            .limits
                            .retry
                            .backoff_ms
                            .saturating_mul(u64::from(attempt));
                        if !sleep_or_cancel(
                            self.clock.as_ref(),
                            Duration::from_millis(backoff),
                            cancellation,
                        )
                        .await
                        {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    async fn attempt(
        &self,
        target: &AlarmDeviceTarget,
        invocation: &AlarmInvocation,
        phase: AlarmDeliveryPhase,
        request: HttpAlarmRequest,
        tracker: &AlarmJobTracker,
        cancellation: &AlarmCancellation,
    ) -> AttemptResult {
        let destination = self.destination_control(&target.destination_id).await;
        if !self
            .global_rate
            .acquire(self.clock.as_ref(), cancellation)
            .await
            || !destination
                .rate
                .acquire(self.clock.as_ref(), cancellation)
                .await
        {
            return AttemptResult::Cancelled;
        }
        let permits = match self
            .acquire_permits(destination.clone(), cancellation)
            .await
        {
            Some(permits) => permits,
            None => return AttemptResult::Cancelled,
        };

        tracker.begin_attempt(&target.device_id).await;
        let started_at = self.clock.now_ms();
        let success_rule = request.success_rule.clone();
        let outbound = OutboundAlarmRequest {
            destination_id: target.destination_id.clone(),
            device_id: target.device_id.clone(),
            phase,
            request,
        };
        let send = self.sender.send(outbound);
        let timeout = self
            .clock
            .sleep(Duration::from_millis(self.limits.request_timeout_ms));
        let outcome = tokio::select! {
            biased;
            _ = cancellation.cancelled() => AttemptResult::Cancelled,
            result = send => classify_sender_result(
                result,
                invocation.definition.evidence.is_platform_verified(target.platform),
                &success_rule,
                &self.limits.retry.retryable_statuses,
            ),
            _ = timeout => AttemptResult::Failed {
                code: "device_simulator.alarm.request_timeout".into(),
                details: Some(format!(
                    "no response within {} ms",
                    self.limits.request_timeout_ms
                )),
                retryable: true,
                timed_out: true,
                http_status: None,
            },
        };
        let duration = self.clock.now_ms().saturating_sub(started_at);
        tracker
            .finish_attempt(&target.device_id, outcome.clone(), duration)
            .await;
        drop(permits);
        outcome
    }

    async fn destination_control(&self, destination_id: &str) -> Arc<DestinationControl> {
        let mut destinations = self.destinations.lock().await;
        destinations
            .entry(destination_id.to_owned())
            .or_insert_with(|| {
                Arc::new(DestinationControl {
                    semaphore: Arc::new(Semaphore::new(self.limits.per_destination_max_in_flight)),
                    rate: RateGate::new(self.limits.per_destination_rate_per_second),
                })
            })
            .clone()
    }

    async fn acquire_permits(
        &self,
        destination: Arc<DestinationControl>,
        cancellation: &AlarmCancellation,
    ) -> Option<AttemptPermits> {
        let global = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return None,
            permit = self.global_semaphore.clone().acquire_owned() => permit.ok()?,
        };
        let per_destination = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return None,
            permit = destination.semaphore.clone().acquire_owned() => permit.ok()?,
        };
        Some(AttemptPermits {
            _global: global,
            _destination: per_destination,
        })
    }

    fn next_event_id(&self) -> u64 {
        NEXT_ALARM_EVENT_ID
            .get_or_init(|| {
                let bytes = *uuid::Uuid::new_v4().as_bytes();
                let seed = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as u64;
                AtomicU64::new(seed % 800_000_000 + 100_000_000)
            })
            .fetch_add(1, Ordering::Relaxed)
    }
}

#[derive(Debug)]
struct AttemptPermits {
    _global: OwnedSemaphorePermit,
    _destination: OwnedSemaphorePermit,
}

#[derive(Debug)]
struct DestinationControl {
    semaphore: Arc<Semaphore>,
    rate: RateGate,
}

#[derive(Debug)]
struct RateGate {
    interval_ms: u64,
    next_allowed_ms: Mutex<u64>,
}

impl RateGate {
    fn new(per_second: u32) -> Self {
        Self {
            interval_ms: 1_000_u64.div_ceil(u64::from(per_second)),
            next_allowed_ms: Mutex::new(0),
        }
    }

    async fn acquire(&self, clock: &dyn AlarmClock, cancellation: &AlarmCancellation) -> bool {
        loop {
            let wait_ms = {
                let mut next_allowed = self.next_allowed_ms.lock().await;
                let now = clock.now_ms();
                if now >= *next_allowed {
                    *next_allowed = now.saturating_add(self.interval_ms);
                    return true;
                }
                next_allowed.saturating_sub(now)
            };
            if !sleep_or_cancel(clock, Duration::from_millis(wait_ms), cancellation).await {
                return false;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AttemptResult {
    Succeeded {
        http_status: u16,
    },
    Unverified {
        http_status: u16,
    },
    Failed {
        code: String,
        details: Option<String>,
        retryable: bool,
        timed_out: bool,
        http_status: Option<u16>,
    },
    Cancelled,
}

impl AttemptResult {
    fn http_status(&self) -> Option<u16> {
        match self {
            Self::Succeeded { http_status } | Self::Unverified { http_status } => {
                Some(*http_status)
            }
            Self::Failed { http_status, .. } => *http_status,
            Self::Cancelled => None,
        }
    }
}

fn classify_sender_result(
    result: Result<AlarmSenderResponse, AlarmSendError>,
    platform_verified: bool,
    success_rule: &ResponseSuccessRule,
    retryable_statuses: &BTreeSet<u16>,
) -> AttemptResult {
    match result {
        Err(error) => AttemptResult::Failed {
            code: error.code,
            details: error.details,
            retryable: error.retryable,
            timed_out: false,
            http_status: None,
        },
        Ok(response) => {
            if !(200..=299).contains(&response.status) {
                return AttemptResult::Failed {
                    code: format!("device_simulator.alarm.http_status.{}", response.status),
                    details: response.endpoint.clone(),
                    retryable: retryable_statuses.contains(&response.status),
                    timed_out: false,
                    http_status: Some(response.status),
                };
            }
            match response.application_status {
                Some(AlarmApplicationStatus::Accepted) => {
                    return AttemptResult::Succeeded {
                        http_status: response.status,
                    };
                }
                Some(AlarmApplicationStatus::Rejected { details }) => {
                    return AttemptResult::Failed {
                        code: "device_simulator.alarm.application_rejected".into(),
                        details: Some(details),
                        retryable: false,
                        timed_out: false,
                        http_status: Some(response.status),
                    };
                }
                None => {}
            }
            if !platform_verified {
                return AttemptResult::Unverified {
                    http_status: response.status,
                };
            }
            match success_rule.evaluate(response.status) {
                None => AttemptResult::Unverified {
                    http_status: response.status,
                },
                Some(true) => AttemptResult::Succeeded {
                    http_status: response.status,
                },
                Some(false) => AttemptResult::Failed {
                    code: format!("device_simulator.alarm.http_status.{}", response.status),
                    details: response.endpoint,
                    retryable: retryable_statuses.contains(&response.status),
                    timed_out: false,
                    http_status: Some(response.status),
                },
            }
        }
    }
}

async fn sleep_or_cancel(
    clock: &dyn AlarmClock,
    duration: Duration,
    cancellation: &AlarmCancellation,
) -> bool {
    if duration.is_zero() {
        return !cancellation.is_cancelled();
    }
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => false,
        _ = clock.sleep(duration) => true,
    }
}

#[derive(Debug)]
struct DeterministicRandom(u64);

impl DeterministicRandom {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    fn next_index(&mut self, length: usize) -> usize {
        self.next_u64() as usize % length
    }

    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[derive(Debug, Clone, Default)]
struct ShuffledCycle {
    remaining: Vec<usize>,
}

impl ShuffledCycle {
    fn next(&mut self, length: usize, random: &mut DeterministicRandom) -> usize {
        debug_assert!(length > 0);
        if self.remaining.is_empty() {
            self.remaining.extend(0..length);
            for index in (1..length).rev() {
                let swap_with = random.next_index(index + 1);
                self.remaining.swap(index, swap_with);
            }
        }
        self.remaining
            .pop()
            .expect("a shuffled cycle is replenished before selection")
    }
}

fn validate_limits(limits: &AlarmSchedulerLimits) -> AlarmResult<()> {
    if limits.global_max_in_flight == 0
        || limits.global_max_in_flight > 512
        || limits.per_destination_max_in_flight == 0
        || limits.per_destination_max_in_flight > limits.global_max_in_flight
        || limits.max_queued_devices > MAX_ALARM_TARGETS
        || limits.global_rate_per_second == 0
        || limits.global_rate_per_second > 10_000
        || limits.per_destination_rate_per_second == 0
        || limits.per_destination_rate_per_second > limits.global_rate_per_second
        || limits.request_timeout_ms == 0
        || limits.request_timeout_ms > MAX_ALARM_REQUEST_TIMEOUT_MS
        || limits.shutdown_grace_ms == 0
        || limits.shutdown_grace_ms > MAX_ALARM_REQUEST_TIMEOUT_MS
    {
        return Err(AlarmError::new(
            "device_simulator.alarm.scheduler_limits_invalid",
            "alarm concurrency, queue, rate, timeout, or shutdown limits are invalid",
        ));
    }
    if limits.retry.max_attempts == 0
        || limits.retry.max_attempts > MAX_ALARM_RETRY_ATTEMPTS
        || limits.retry.backoff_ms > MAX_ALARM_REQUEST_TIMEOUT_MS
        || limits
            .retry
            .retryable_statuses
            .iter()
            .any(|status| !(100..=599).contains(status))
    {
        return Err(AlarmError::new(
            "device_simulator.alarm.retry_policy_invalid",
            "alarm retries must be finite and use valid HTTP status codes",
        ));
    }
    Ok(())
}

fn validate_job(
    job_id: &str,
    targets: &[AlarmDeviceTarget],
    mode: AlarmDispatchMode,
    recovery_delay_ms: Option<u64>,
    limits: &AlarmSchedulerLimits,
) -> AlarmResult<()> {
    if job_id.is_empty()
        || job_id.len() > 128
        || !job_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(AlarmError::new(
            "device_simulator.alarm.job_id_invalid",
            "alarm job ID is invalid",
        ));
    }
    let capacity = limits
        .global_max_in_flight
        .saturating_add(limits.max_queued_devices)
        .min(MAX_ALARM_TARGETS);
    if targets.is_empty() || targets.len() > capacity {
        return Err(AlarmError::new(
            "device_simulator.alarm.target_capacity_exceeded",
            "alarm target count exceeds the bounded scheduler capacity",
        ));
    }
    if recovery_delay_ms.is_some_and(|delay| delay > MAX_ALARM_INTERVAL_MS) {
        return Err(AlarmError::new(
            "device_simulator.alarm.recovery_delay_invalid",
            "alarm recovery delay exceeds 24 hours",
        ));
    }
    let mut device_ids = BTreeSet::new();
    for target in targets {
        if target.device_id.is_empty()
            || target.device_id.len() > 256
            || target.destination_id.is_empty()
            || target.destination_id.len() > 256
            || target
                .destination_id
                .bytes()
                .any(|byte| byte.is_ascii_control())
            || !device_ids.insert(target.device_id.as_str())
            || target.invocations.is_empty()
        {
            return Err(AlarmError::new(
                "device_simulator.alarm.target_invalid",
                "alarm targets must have unique IDs, destinations, and invocations",
            ));
        }
        if mode == AlarmDispatchMode::Specified && target.invocations.len() != 1 {
            return Err(AlarmError::new(
                "device_simulator.alarm.specified_mode_ambiguous",
                "specified alarm mode requires exactly one invocation per device",
            ));
        }
        for invocation in &target.invocations {
            if invocation.definition.profile_id != invocation.definition.handler_id.profile_id()
                || invocation.context.source_ip.is_none()
            {
                return Err(AlarmError::new(
                    "device_simulator.alarm.invocation_invalid",
                    "alarm invocation has a mismatched handler or missing source IP",
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::alarms::{
        AlarmHandlerId, AlarmRequestDefinition, AlarmTypeId, BodyEncoding, CompiledTemplate,
        FixtureProvenance, HandlerEvidence, HttpMethod, ImagePolicy, PlatformEvidence,
        PlatformVerification, RecoveryDefinition, RecoveryTrigger, ResponseSuccessRule,
        SourceBinding, TransportDefinition,
    };
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU64, AtomicUsize};

    #[derive(Default)]
    struct TestClock {
        now: AtomicU64,
    }

    impl AlarmClock for TestClock {
        fn now_ms(&self) -> u64 {
            self.now.load(Ordering::Acquire)
        }

        fn sleep(&self, duration: Duration) -> AlarmFuture<'_, ()> {
            Box::pin(async move {
                self.now.fetch_add(
                    duration.as_millis().min(u64::MAX as u128) as u64,
                    Ordering::AcqRel,
                );
            })
        }
    }

    #[derive(Default)]
    struct ScriptedSender {
        responses: Mutex<VecDeque<Result<AlarmSenderResponse, AlarmSendError>>>,
        requests: Mutex<Vec<OutboundAlarmRequest>>,
        send_times: Mutex<Vec<u64>>,
        clock: Option<Arc<dyn AlarmClock>>,
    }

    impl ScriptedSender {
        fn successful(clock: Arc<dyn AlarmClock>) -> Self {
            Self {
                clock: Some(clock),
                ..Self::default()
            }
        }
    }

    impl AlarmSender for ScriptedSender {
        fn send(
            &self,
            request: OutboundAlarmRequest,
        ) -> AlarmFuture<'_, Result<AlarmSenderResponse, AlarmSendError>> {
            Box::pin(async move {
                if let Some(clock) = &self.clock {
                    self.send_times.lock().await.push(clock.now_ms());
                }
                self.requests.lock().await.push(request);
                self.responses
                    .lock()
                    .await
                    .pop_front()
                    .unwrap_or(Ok(AlarmSenderResponse::new(200)))
            })
        }
    }

    struct PendingSender {
        calls: AtomicUsize,
        started: Notify,
    }

    impl Default for PendingSender {
        fn default() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                started: Notify::new(),
            }
        }
    }

    impl AlarmSender for PendingSender {
        fn send(
            &self,
            _request: OutboundAlarmRequest,
        ) -> AlarmFuture<'_, Result<AlarmSenderResponse, AlarmSendError>> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::AcqRel);
                self.started.notify_waiters();
                std::future::pending().await
            })
        }
    }

    fn definition(
        handler: AlarmHandlerId,
        success_rule: ResponseSuccessRule,
        verified: bool,
        with_recovery: bool,
    ) -> Arc<AlarmHandlerDefinition> {
        Arc::new(AlarmHandlerDefinition {
            handler_id: handler,
            alarm_type_id: AlarmTypeId::new("fixture").unwrap(),
            profile_id: handler.profile_id(),
            // Structured alarms are rendered through the structured legacy path,
            // which requires a StructureInfo.ImageInfoList array to normalize.
            template: CompiledTemplate::compile(
                br#"{"device":"{{device_id}}","timestamp":{{timestamp}},"eventId":{{event_id}},"StructureInfo":{"ImageInfoList":[]}}"#,
            )
            .unwrap(),
            image_policy: ImagePolicy::Forbidden,
            images: vec![],
            alternate_images: vec![],
            transport: TransportDefinition {
                method: HttpMethod::Post,
                path: "/fixture/alarm".into(),
                source_binding: SourceBinding::DeviceIp,
                body_encoding: BodyEncoding::Raw {
                    content_type: "application/json".into(),
                },
                success_rule: success_rule.clone(),
            },
            follow_up_requests: vec![],
            recovery: if with_recovery {
                RecoveryDefinition::RenderWith {
                    template: CompiledTemplate::compile(
                        br#"{"device":"{{device_id}}","state":"recovered"}"#,
                    )
                    .unwrap(),
                    transport: TransportDefinition {
                        method: HttpMethod::Post,
                        path: "/fixture/alarm".into(),
                        source_binding: SourceBinding::DeviceIp,
                        body_encoding: BodyEncoding::Raw {
                            content_type: "application/json".into(),
                        },
                        success_rule: success_rule.clone(),
                    },
                    trigger: RecoveryTrigger::RequestedDelay,
                    include_images: false,
                }
            } else {
                RecoveryDefinition::None
            },
            evidence: HandlerEvidence {
                legacy_sources: vec!["script/FixtureAlarm.py".into()],
                template_source: "test fixture".into(),
                fixture_provenance: FixtureProvenance::LegacyOrCaptureDerived,
                platforms: vec![PlatformEvidence {
                    platform: TargetPlatform::Ums,
                    verification: if verified {
                        PlatformVerification::PlatformVerified
                    } else {
                        PlatformVerification::SourceConfirmedPlatformUnverified
                    },
                }],
                intentional_changes: vec![],
            },
        })
    }

    fn target(device_id: &str, definitions: Vec<Arc<AlarmHandlerDefinition>>) -> AlarmDeviceTarget {
        AlarmDeviceTarget {
            device_id: device_id.into(),
            destination_id: "ums-a".into(),
            platform: TargetPlatform::Ums,
            invocations: definitions
                .into_iter()
                .map(|definition| AlarmInvocation {
                    definition,
                    context: AlarmBuildContext {
                        source_ip: Some("10.0.0.8".parse().unwrap()),
                        fields: BTreeMap::from([(
                            super::super::DynamicField::DeviceId,
                            device_id.into(),
                        )]),
                        multipart_boundary: None,
                        legacy_values: None,
                    },
                    image_cache: Arc::new(parking_lot::RwLock::new(ImageCache::default())),
                })
                .collect(),
        }
    }

    fn limits() -> AlarmSchedulerLimits {
        AlarmSchedulerLimits {
            global_max_in_flight: 4,
            per_destination_max_in_flight: 2,
            max_queued_devices: 16,
            global_rate_per_second: 1_000,
            per_destination_rate_per_second: 1_000,
            request_timeout_ms: 100,
            shutdown_grace_ms: 100,
            retry: AlarmRetryPolicy {
                max_attempts: 2,
                backoff_ms: 1,
                retryable_statuses: BTreeSet::from([500]),
            },
        }
    }

    #[test]
    fn random_cycles_use_all_one_hundred_entries_before_repeating() {
        let mut random = DeterministicRandom::new(42);
        let mut cycle = ShuffledCycle::default();
        let first = (0..100)
            .map(|_| cycle.next(100, &mut random))
            .collect::<BTreeSet<_>>();
        let second = (0..100)
            .map(|_| cycle.next(100, &mut random))
            .collect::<BTreeSet<_>>();

        assert_eq!(first, (0..100).collect());
        assert_eq!(second, (0..100).collect());
    }

    #[tokio::test]
    async fn one_shot_binds_each_request_to_the_device_source_ip() {
        let clock: Arc<dyn AlarmClock> = Arc::new(TestClock::default());
        let sender = Arc::new(ScriptedSender::successful(clock.clone()));
        let scheduler = AlarmScheduler::new(sender.clone(), clock, limits()).unwrap();
        let handler = definition(
            AlarmHandlerId::StructuredV1,
            ResponseSuccessRule::StatusRange {
                minimum: 200,
                maximum: 299,
            },
            true,
            false,
        );
        let snapshot = scheduler
            .trigger_once(OneShotAlarmJob {
                job_id: "once".into(),
                targets: vec![
                    target("one", vec![handler.clone()]),
                    target("two", vec![handler]),
                ],
                mode: AlarmDispatchMode::Specified,
                recovery_delay_ms: None,
                random_seed: 1,
            })
            .await
            .unwrap();
        assert_eq!(snapshot.state, ScheduledAlarmJobState::Completed);
        assert_eq!(snapshot.attempted, 2);
        assert_eq!(snapshot.succeeded, 2);
        let requests = sender.requests.lock().await;
        assert!(requests
            .iter()
            .all(|request| request.request.source_ip.to_string() == "10.0.0.8"));
        let payloads = requests
            .iter()
            .map(|request| {
                serde_json::from_slice::<serde_json::Value>(&request.request.body).unwrap()
            })
            .collect::<Vec<_>>();
        assert!(payloads.iter().all(|payload| {
            payload["timestamp"]
                .as_i64()
                .is_some_and(|timestamp| timestamp > 1_600_000_000)
        }));
        assert_ne!(payloads[0]["eventId"], payloads[1]["eventId"]);
    }

    #[test]
    fn separate_schedulers_share_a_non_repeating_event_sequence() {
        let clock: Arc<dyn AlarmClock> = Arc::new(TestClock::default());
        let first = AlarmScheduler::new(
            Arc::new(ScriptedSender::successful(clock.clone())),
            clock.clone(),
            limits(),
        )
        .unwrap();
        let second = AlarmScheduler::new(
            Arc::new(ScriptedSender::successful(clock.clone())),
            clock,
            limits(),
        )
        .unwrap();

        let first_id = first.next_event_id();
        let second_id = second.next_event_id();
        assert!(second_id > first_id);
        assert!((100_000_000..900_000_002).contains(&first_id));
    }

    #[tokio::test]
    async fn one_shot_sequential_sends_each_registered_invocation_once() {
        let clock: Arc<dyn AlarmClock> = Arc::new(TestClock::default());
        let sender = Arc::new(ScriptedSender::successful(clock.clone()));
        let scheduler = AlarmScheduler::new(sender.clone(), clock, limits()).unwrap();
        let first = definition(
            AlarmHandlerId::StructuredV1,
            ResponseSuccessRule::StatusRange {
                minimum: 200,
                maximum: 299,
            },
            true,
            false,
        );
        let mut second_value = (*first).clone();
        second_value.alarm_type_id = AlarmTypeId::new("fixture-second").unwrap();
        second_value.transport.path = "/fixture/second".into();
        let second = Arc::new(second_value);

        let snapshot = scheduler
            .trigger_once(OneShotAlarmJob {
                job_id: "once-all".into(),
                targets: vec![target("one", vec![first, second])],
                mode: AlarmDispatchMode::Sequential,
                recovery_delay_ms: None,
                random_seed: 1,
            })
            .await
            .unwrap();

        assert_eq!(snapshot.attempted, 2);
        let requests = sender.requests.lock().await;
        assert_eq!(requests[0].request.path, "/fixture/alarm");
        assert_eq!(requests[1].request.path, "/fixture/second");
    }

    #[tokio::test]
    async fn periodic_fixed_count_is_independent_per_device_and_sends_recovery() {
        let clock: Arc<dyn AlarmClock> = Arc::new(TestClock::default());
        let sender = Arc::new(ScriptedSender::successful(clock.clone()));
        let scheduler = AlarmScheduler::new(sender.clone(), clock, limits()).unwrap();
        let handler = definition(
            AlarmHandlerId::StructuredV1,
            ResponseSuccessRule::StatusRange {
                minimum: 200,
                maximum: 299,
            },
            true,
            true,
        );
        let job = scheduler
            .start_periodic(PeriodicAlarmJob {
                job_id: "periodic".into(),
                targets: vec![
                    target("one", vec![handler.clone()]),
                    target("two", vec![handler]),
                ],
                mode: AlarmDispatchMode::Specified,
                interval_ms: 5,
                send_count: Some(2),
                recovery_delay_ms: Some(1),
                random_seed: 1,
            })
            .unwrap();
        let snapshot = job.wait().await.unwrap();
        assert_eq!(snapshot.succeeded, 8);
        assert_eq!(snapshot.devices["one"].succeeded, 4);
        assert_eq!(snapshot.devices["two"].succeeded, 4);
        let requests = sender.requests.lock().await;
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.phase == AlarmDeliveryPhase::Recovery)
                .count(),
            4
        );
    }

    #[tokio::test]
    async fn success_is_never_claimed_without_verified_handler_evidence() {
        let clock: Arc<dyn AlarmClock> = Arc::new(TestClock::default());
        let sender = Arc::new(ScriptedSender::successful(clock.clone()));
        let scheduler = AlarmScheduler::new(sender, clock, limits()).unwrap();
        let handler = definition(
            AlarmHandlerId::StructuredV1,
            ResponseSuccessRule::StatusRange {
                minimum: 200,
                maximum: 299,
            },
            false,
            false,
        );
        let snapshot = scheduler
            .trigger_once(OneShotAlarmJob {
                job_id: "unverified".into(),
                targets: vec![target("one", vec![handler])],
                mode: AlarmDispatchMode::Specified,
                recovery_delay_ms: None,
                random_seed: 1,
            })
            .await
            .unwrap();
        assert_eq!(snapshot.succeeded, 0);
        assert_eq!(snapshot.failed, 0);
        assert_eq!(snapshot.unverified, 1);
        assert_eq!(snapshot.attempted, 1);
        assert_eq!(snapshot.last_error_code, None);
        assert_eq!(snapshot.last_http_status, Some(200));
        assert_eq!(snapshot.devices["one"].last_http_status, Some(200));
    }

    #[test]
    fn application_acknowledgement_distinguishes_acceptance_from_http_200_rejection() {
        let retryable = BTreeSet::new();
        let accepted = classify_sender_result(
            Ok(AlarmSenderResponse::new(200)
                .with_application_status(AlarmApplicationStatus::Accepted)),
            false,
            &ResponseSuccessRule::Unverified,
            &retryable,
        );
        assert_eq!(accepted, AttemptResult::Succeeded { http_status: 200 });

        let rejected = classify_sender_result(
            Ok(AlarmSenderResponse::new(200).with_application_status(
                AlarmApplicationStatus::Rejected {
                    details: "ResponseCode=17".into(),
                },
            )),
            true,
            &ResponseSuccessRule::StatusRange {
                minimum: 200,
                maximum: 299,
            },
            &retryable,
        );
        assert_eq!(
            rejected,
            AttemptResult::Failed {
                code: "device_simulator.alarm.application_rejected".into(),
                details: Some("ResponseCode=17".into()),
                retryable: false,
                timed_out: false,
                http_status: Some(200),
            }
        );
    }

    #[tokio::test]
    async fn unverified_handler_still_reports_http_failures() {
        let clock: Arc<dyn AlarmClock> = Arc::new(TestClock::default());
        let sender = Arc::new(ScriptedSender::successful(clock.clone()));
        sender.responses.lock().await.extend([
            Ok(AlarmSenderResponse::new(500)),
            Ok(AlarmSenderResponse::new(500)),
        ]);
        let scheduler = AlarmScheduler::new(sender, clock, limits()).unwrap();
        let handler = definition(
            AlarmHandlerId::StructuredV1,
            ResponseSuccessRule::StatusRange {
                minimum: 200,
                maximum: 299,
            },
            false,
            false,
        );

        let snapshot = scheduler
            .trigger_once(OneShotAlarmJob {
                job_id: "unverified-http-error".into(),
                targets: vec![target("one", vec![handler])],
                mode: AlarmDispatchMode::Specified,
                recovery_delay_ms: None,
                random_seed: 1,
            })
            .await
            .unwrap();

        assert_eq!(snapshot.attempted, 2);
        assert_eq!(snapshot.failed, 2);
        assert_eq!(snapshot.unverified, 0);
        assert_eq!(
            snapshot.last_error_code.as_deref(),
            Some("device_simulator.alarm.http_status.500")
        );
        assert_eq!(snapshot.last_http_status, Some(500));
        assert_eq!(snapshot.devices["one"].last_http_status, Some(500));
    }

    #[tokio::test]
    async fn source_confirmed_compound_flow_preserves_order_and_still_sends_recovery() {
        let clock: Arc<dyn AlarmClock> = Arc::new(TestClock::default());
        let sender = Arc::new(ScriptedSender::successful(clock.clone()));
        let scheduler = AlarmScheduler::new(sender.clone(), clock, limits()).unwrap();
        let mut handler = (*definition(
            AlarmHandlerId::StructuredV1,
            ResponseSuccessRule::Unverified,
            false,
            true,
        ))
        .clone();
        handler.transport.path = "/legacy/structure".into();
        handler.follow_up_requests.push(AlarmRequestDefinition {
            template: CompiledTemplate::compile(br#"{"related":"{{device_id}}"}"#).unwrap(),
            image_policy: ImagePolicy::Forbidden,
            images: vec![],
            transport: TransportDefinition {
                method: HttpMethod::Post,
                path: "/legacy/alarm".into(),
                source_binding: SourceBinding::DeviceIp,
                body_encoding: BodyEncoding::Raw {
                    content_type: "application/json".into(),
                },
                success_rule: ResponseSuccessRule::Unverified,
            },
        });
        let snapshot = scheduler
            .trigger_once(OneShotAlarmJob {
                job_id: "compound".into(),
                targets: vec![target("one", vec![Arc::new(handler)])],
                mode: AlarmDispatchMode::Specified,
                recovery_delay_ms: Some(1),
                random_seed: 1,
            })
            .await
            .unwrap();
        assert_eq!(snapshot.attempted, 3);
        assert_eq!(snapshot.unverified, 3);
        let requests = sender.requests.lock().await;
        assert_eq!(
            requests
                .iter()
                .map(|request| request.request.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/legacy/structure", "/legacy/alarm", "/fixture/alarm"]
        );
        assert_eq!(requests[2].phase, AlarmDeliveryPhase::Recovery);
    }

    #[tokio::test]
    async fn retry_is_finite_and_only_for_retryable_failures() {
        let clock: Arc<dyn AlarmClock> = Arc::new(TestClock::default());
        let sender = Arc::new(ScriptedSender::successful(clock.clone()));
        sender.responses.lock().await.extend([
            Ok(AlarmSenderResponse::new(500)),
            Ok(AlarmSenderResponse::new(500)),
            Ok(AlarmSenderResponse::new(200)),
        ]);
        let scheduler = AlarmScheduler::new(sender.clone(), clock, limits()).unwrap();
        let handler = definition(
            AlarmHandlerId::StructuredV1,
            ResponseSuccessRule::StatusRange {
                minimum: 200,
                maximum: 299,
            },
            true,
            false,
        );
        let snapshot = scheduler
            .trigger_once(OneShotAlarmJob {
                job_id: "retry".into(),
                targets: vec![target("one", vec![handler])],
                mode: AlarmDispatchMode::Specified,
                recovery_delay_ms: None,
                random_seed: 1,
            })
            .await
            .unwrap();
        assert_eq!(snapshot.attempted, 2);
        assert_eq!(snapshot.failed, 2);
        assert_eq!(sender.requests.lock().await.len(), 2);
    }

    #[tokio::test]
    async fn timeout_retries_are_bounded() {
        let clock: Arc<dyn AlarmClock> = Arc::new(TestClock::default());
        let sender = Arc::new(PendingSender::default());
        let scheduler = AlarmScheduler::new(sender.clone(), clock, limits()).unwrap();
        let handler = definition(
            AlarmHandlerId::StructuredV1,
            ResponseSuccessRule::StatusRange {
                minimum: 200,
                maximum: 299,
            },
            true,
            false,
        );
        let snapshot = scheduler
            .trigger_once(OneShotAlarmJob {
                job_id: "timeout".into(),
                targets: vec![target("one", vec![handler])],
                mode: AlarmDispatchMode::Specified,
                recovery_delay_ms: None,
                random_seed: 1,
            })
            .await
            .unwrap();
        assert_eq!(snapshot.attempted, 2);
        assert_eq!(snapshot.timed_out, 2);
        assert_eq!(snapshot.last_http_status, None);
        assert_eq!(snapshot.devices["one"].last_http_status, None);
        assert_eq!(sender.calls.load(Ordering::Acquire), 2);
    }

    #[tokio::test]
    async fn transport_failure_after_http_response_clears_the_attempt_status() {
        let clock: Arc<dyn AlarmClock> = Arc::new(TestClock::default());
        let sender = Arc::new(ScriptedSender::successful(clock.clone()));
        sender.responses.lock().await.extend([
            Ok(AlarmSenderResponse::new(500)),
            Err(AlarmSendError::new(
                "device_simulator.alarm.transport_failed",
                false,
            )),
        ]);
        let scheduler = AlarmScheduler::new(sender, clock, limits()).unwrap();
        let handler = definition(
            AlarmHandlerId::StructuredV1,
            ResponseSuccessRule::StatusRange {
                minimum: 200,
                maximum: 299,
            },
            true,
            false,
        );

        let snapshot = scheduler
            .trigger_once(OneShotAlarmJob {
                job_id: "response-then-transport-error".into(),
                targets: vec![target("one", vec![handler])],
                mode: AlarmDispatchMode::Specified,
                recovery_delay_ms: None,
                random_seed: 1,
            })
            .await
            .unwrap();

        assert_eq!(snapshot.attempted, 2);
        assert_eq!(snapshot.last_http_status, None);
        assert_eq!(snapshot.devices["one"].last_http_status, None);
        assert_eq!(
            snapshot.last_error_code.as_deref(),
            Some("device_simulator.alarm.transport_failed")
        );
    }

    #[tokio::test]
    async fn last_http_status_follows_attempt_completion_order_across_devices() {
        let handler = definition(
            AlarmHandlerId::StructuredV1,
            ResponseSuccessRule::StatusRange {
                minimum: 200,
                maximum: 299,
            },
            true,
            false,
        );
        let targets = vec![
            target("z-first", vec![handler.clone()]),
            target("a-last", vec![handler]),
        ];
        let tracker = AlarmJobTracker::new("completion-order".into(), &targets);

        tracker.begin_attempt("z-first").await;
        tracker
            .finish_attempt("z-first", AttemptResult::Succeeded { http_status: 201 }, 1)
            .await;
        tracker.begin_attempt("a-last").await;
        tracker
            .finish_attempt("a-last", AttemptResult::Succeeded { http_status: 202 }, 1)
            .await;

        assert_eq!(tracker.snapshot().await.last_http_status, Some(202));
    }

    #[tokio::test]
    async fn continuous_job_cancels_an_in_flight_send_without_new_requests() {
        let clock: Arc<dyn AlarmClock> = Arc::new(SystemAlarmClock::default());
        let sender = Arc::new(PendingSender::default());
        let scheduler = AlarmScheduler::new(sender.clone(), clock, limits()).unwrap();
        let handler = definition(
            AlarmHandlerId::StructuredV1,
            ResponseSuccessRule::StatusRange {
                minimum: 200,
                maximum: 299,
            },
            true,
            false,
        );
        let job = scheduler
            .start_periodic(PeriodicAlarmJob {
                job_id: "continuous".into(),
                targets: vec![target("one", vec![handler])],
                mode: AlarmDispatchMode::Specified,
                interval_ms: 1,
                send_count: None,
                recovery_delay_ms: None,
                random_seed: 1,
            })
            .unwrap();
        sender.started.notified().await;
        let snapshot = job.stop_and_wait().await.unwrap();
        assert_eq!(snapshot.state, ScheduledAlarmJobState::Cancelled);
        assert_eq!(snapshot.attempted, 1);
        assert_eq!(snapshot.cancelled, 1);
        assert_eq!(sender.calls.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn global_rate_gate_spaces_sender_calls() {
        let clock_impl = Arc::new(TestClock::default());
        let clock: Arc<dyn AlarmClock> = clock_impl.clone();
        let sender = Arc::new(ScriptedSender::successful(clock.clone()));
        let mut limits = limits();
        limits.global_rate_per_second = 2;
        limits.per_destination_rate_per_second = 2;
        let scheduler = AlarmScheduler::new(sender.clone(), clock, limits).unwrap();
        let handler = definition(
            AlarmHandlerId::StructuredV1,
            ResponseSuccessRule::StatusRange {
                minimum: 200,
                maximum: 299,
            },
            true,
            false,
        );
        scheduler
            .trigger_once(OneShotAlarmJob {
                job_id: "rate".into(),
                targets: vec![
                    target("one", vec![handler.clone()]),
                    target("two", vec![handler]),
                ],
                mode: AlarmDispatchMode::Specified,
                recovery_delay_ms: None,
                random_seed: 1,
            })
            .await
            .unwrap();
        let mut times = sender.send_times.lock().await.clone();
        times.sort_unstable();
        assert_eq!(times.len(), 2);
        assert!(times[1].saturating_sub(times[0]) >= 500);
    }

    #[tokio::test]
    async fn sequential_and_random_modes_select_only_registered_invocations() {
        let clock: Arc<dyn AlarmClock> = Arc::new(TestClock::default());
        let sender = Arc::new(ScriptedSender::successful(clock.clone()));
        let scheduler = AlarmScheduler::new(sender.clone(), clock, limits()).unwrap();
        let first = definition(
            AlarmHandlerId::StructuredV1,
            ResponseSuccessRule::StatusRange {
                minimum: 200,
                maximum: 299,
            },
            true,
            false,
        );
        let mut second_value = (*first).clone();
        second_value.alarm_type_id = AlarmTypeId::new("fixture-second").unwrap();
        second_value.transport.path = "/fixture/second".into();
        let second = Arc::new(second_value);
        let job = scheduler
            .start_periodic(PeriodicAlarmJob {
                job_id: "sequence".into(),
                targets: vec![target("one", vec![first.clone(), second.clone()])],
                mode: AlarmDispatchMode::Sequential,
                interval_ms: 1,
                send_count: Some(2),
                recovery_delay_ms: None,
                random_seed: 7,
            })
            .unwrap();
        job.wait().await.unwrap();
        let requests = sender.requests.lock().await;
        assert_eq!(requests[0].request.path, "/fixture/alarm");
        assert_eq!(requests[1].request.path, "/fixture/second");
        drop(requests);

        let random_job = scheduler
            .start_periodic(PeriodicAlarmJob {
                job_id: "random".into(),
                targets: vec![target("two", vec![first, second])],
                mode: AlarmDispatchMode::Random,
                interval_ms: 1,
                send_count: Some(4),
                recovery_delay_ms: None,
                random_seed: 9,
            })
            .unwrap();
        assert_eq!(random_job.wait().await.unwrap().attempted, 4);
    }

    #[test]
    fn invalid_zero_count_unbounded_retry_and_queue_overflow_are_rejected() {
        let clock: Arc<dyn AlarmClock> = Arc::new(TestClock::default());
        let sender: Arc<dyn AlarmSender> = Arc::new(ScriptedSender::default());
        let mut invalid_limits = limits();
        invalid_limits.retry.max_attempts = MAX_ALARM_RETRY_ATTEMPTS + 1;
        assert_eq!(
            AlarmScheduler::new(sender.clone(), clock.clone(), invalid_limits)
                .err()
                .unwrap()
                .code,
            "device_simulator.alarm.retry_policy_invalid"
        );

        let scheduler = AlarmScheduler::new(sender, clock, limits()).unwrap();
        let handler = definition(
            AlarmHandlerId::StructuredV1,
            ResponseSuccessRule::StatusRange {
                minimum: 200,
                maximum: 299,
            },
            true,
            false,
        );
        assert_eq!(
            scheduler
                .start_periodic(PeriodicAlarmJob {
                    job_id: "zero".into(),
                    targets: vec![target("one", vec![handler])],
                    mode: AlarmDispatchMode::Specified,
                    interval_ms: 1,
                    send_count: Some(0),
                    recovery_delay_ms: None,
                    random_seed: 1,
                })
                .err()
                .unwrap()
                .code,
            "device_simulator.alarm.send_count_invalid"
        );
    }
}
