use super::{
    build_alarm_request, build_recovery_request, AlarmBuildContext, AlarmError,
    AlarmHandlerDefinition, AlarmResult, HttpAlarmRequest, ImageCache, RecoveryDefinition,
};
use crate::device_simulator::profiles::scope::TargetPlatform;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Notify, OwnedSemaphorePermit, Semaphore};
use tokio::task::{JoinHandle, JoinSet};

pub const MAX_ALARM_TARGETS: usize = 2_048;
pub const MAX_ALARM_RETRY_ATTEMPTS: u8 = 5;
pub const MAX_ALARM_REQUEST_TIMEOUT_MS: u64 = 60_000;
pub const MAX_ALARM_INTERVAL_MS: u64 = 24 * 60 * 60 * 1_000;

pub type AlarmFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlarmSenderResponse {
    pub status: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmSendError {
    pub code: String,
    pub retryable: bool,
}

impl AlarmSendError {
    pub fn new(code: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.into(),
            retryable,
        }
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
    pub image_cache: Arc<ImageCache>,
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
    pub last_error_code: Option<String>,
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
    pub last_error_code: Option<String>,
    pub devices: BTreeMap<String, DeviceAlarmStats>,
}

#[derive(Debug)]
struct JobStatsInner {
    state: ScheduledAlarmJobState,
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
        let mut inner = self.inner.lock().await;
        let stats = inner.devices.entry(device_id.to_owned()).or_default();
        stats.in_flight = stats.in_flight.saturating_sub(1);
        stats.total_duration_ms = stats.total_duration_ms.saturating_add(duration_ms);
        match outcome {
            AttemptResult::Succeeded => {
                stats.succeeded = stats.succeeded.saturating_add(1);
                stats.last_error_code = None;
            }
            AttemptResult::Failed {
                code,
                unverified,
                timed_out,
                ..
            } => {
                stats.failed = stats.failed.saturating_add(1);
                stats.unverified = stats.unverified.saturating_add(u64::from(unverified));
                stats.timed_out = stats.timed_out.saturating_add(u64::from(timed_out));
                stats.last_error_code = Some(code);
            }
            AttemptResult::Cancelled => {
                stats.cancelled = stats.cancelled.saturating_add(1);
                stats.last_error_code = Some("device_simulator.alarm.cancelled".into());
            }
        }
    }

    async fn reject(&self, device_id: &str, code: impl Into<String>) {
        let mut inner = self.inner.lock().await;
        let stats = inner.devices.entry(device_id.to_owned()).or_default();
        stats.rejected = stats.rejected.saturating_add(1);
        stats.last_error_code = Some(code.into());
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
        last_error_code,
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
            tasks.spawn(async move {
                scheduler
                    .run_device(
                        target,
                        mode,
                        send_count,
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
        while send_count.map_or(true, |limit| round < limit) && !cancellation.is_cancelled() {
            let invocation_index = match mode {
                AlarmDispatchMode::Specified => 0,
                AlarmDispatchMode::Sequential => round as usize % target.invocations.len(),
                AlarmDispatchMode::Random => random.next_index(target.invocations.len()),
            };
            let invocation = &target.invocations[invocation_index];
            let alarm_succeeded = self
                .deliver_invocation(
                    &target,
                    invocation,
                    AlarmDeliveryPhase::Alarm,
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
        tracker: &AlarmJobTracker,
        cancellation: &AlarmCancellation,
    ) -> bool {
        let mut context = invocation.context.clone();
        context.fields.insert(
            crate::device_simulator::alarms::DynamicField::Timestamp,
            self.clock.now_ms().to_string(),
        );
        let request = match phase {
            AlarmDeliveryPhase::Alarm => {
                build_alarm_request(&invocation.definition, &context, &invocation.image_cache)
                    .map(Some)
            }
            AlarmDeliveryPhase::Recovery => {
                build_recovery_request(&invocation.definition, &context, &invocation.image_cache)
            }
        };
        let request = match request {
            Ok(Some(request)) => request,
            Ok(None) => return true,
            Err(error) => {
                tracker.reject(&target.device_id, error.code).await;
                return false;
            }
        };

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
                AttemptResult::Succeeded => return true,
                AttemptResult::Cancelled => return false,
                AttemptResult::Failed {
                    retryable: false, ..
                } => return false,
                AttemptResult::Failed { .. } if attempt >= self.limits.retry.max_attempts => {
                    return false;
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
        false
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
                invocation,
                target.platform,
                &self.limits.retry.retryable_statuses,
            ),
            _ = timeout => AttemptResult::Failed {
                code: "device_simulator.alarm.request_timeout".into(),
                retryable: true,
                unverified: false,
                timed_out: true,
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
    Succeeded,
    Failed {
        code: String,
        retryable: bool,
        unverified: bool,
        timed_out: bool,
    },
    Cancelled,
}

fn classify_sender_result(
    result: Result<AlarmSenderResponse, AlarmSendError>,
    invocation: &AlarmInvocation,
    platform: TargetPlatform,
    retryable_statuses: &BTreeSet<u16>,
) -> AttemptResult {
    match result {
        Err(error) => AttemptResult::Failed {
            code: error.code,
            retryable: error.retryable,
            unverified: false,
            timed_out: false,
        },
        Ok(response) => {
            if !invocation
                .definition
                .evidence
                .is_platform_verified(platform)
            {
                return AttemptResult::Failed {
                    code: "device_simulator.alarm.success_evidence_unverified".into(),
                    retryable: false,
                    unverified: true,
                    timed_out: false,
                };
            }
            match invocation
                .definition
                .transport
                .success_rule
                .evaluate(response.status)
            {
                None => AttemptResult::Failed {
                    code: "device_simulator.alarm.success_rule_unverified".into(),
                    retryable: false,
                    unverified: true,
                    timed_out: false,
                },
                Some(true) => AttemptResult::Succeeded,
                Some(false) => AttemptResult::Failed {
                    code: format!("device_simulator.alarm.http_status.{}", response.status),
                    retryable: retryable_statuses.contains(&response.status),
                    unverified: false,
                    timed_out: false,
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
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 as usize % length
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
        AlarmHandlerId, AlarmTypeId, BodyEncoding, CompiledTemplate, FixtureProvenance,
        HandlerEvidence, HttpMethod, ImagePolicy, PlatformEvidence, PlatformVerification,
        RecoveryDefinition, RecoveryTrigger, ResponseSuccessRule, SourceBinding,
        TransportDefinition,
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
                    .unwrap_or(Ok(AlarmSenderResponse { status: 200 }))
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
            template: CompiledTemplate::compile(br#"{"device":"{{device_id}}"}"#).unwrap(),
            image_policy: ImagePolicy::Forbidden,
            images: vec![],
            transport: TransportDefinition {
                method: HttpMethod::Post,
                path: "/fixture/alarm".into(),
                source_binding: SourceBinding::DeviceIp,
                body_encoding: BodyEncoding::Raw {
                    content_type: "application/json".into(),
                },
                success_rule,
            },
            recovery: if with_recovery {
                RecoveryDefinition::RenderWith {
                    template: CompiledTemplate::compile(
                        br#"{"device":"{{device_id}}","state":"recovered"}"#,
                    )
                    .unwrap(),
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
                    platform: TargetPlatform::Vms,
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
            destination_id: "vms-a".into(),
            platform: TargetPlatform::Vms,
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
                    },
                    image_cache: Arc::new(ImageCache::default()),
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

    #[tokio::test]
    async fn one_shot_binds_each_request_to_the_device_source_ip() {
        let clock: Arc<dyn AlarmClock> = Arc::new(TestClock::default());
        let sender = Arc::new(ScriptedSender::successful(clock.clone()));
        let scheduler = AlarmScheduler::new(sender.clone(), clock, limits()).unwrap();
        let handler = definition(
            AlarmHandlerId::SmartV1,
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
    }

    #[tokio::test]
    async fn periodic_fixed_count_is_independent_per_device_and_sends_recovery() {
        let clock: Arc<dyn AlarmClock> = Arc::new(TestClock::default());
        let sender = Arc::new(ScriptedSender::successful(clock.clone()));
        let scheduler = AlarmScheduler::new(sender.clone(), clock, limits()).unwrap();
        let handler = definition(
            AlarmHandlerId::SmartV1,
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
            AlarmHandlerId::SmartV1,
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
        assert_eq!(snapshot.failed, 1);
        assert_eq!(snapshot.unverified, 1);
        assert_eq!(snapshot.attempted, 1);
    }

    #[tokio::test]
    async fn retry_is_finite_and_only_for_retryable_failures() {
        let clock: Arc<dyn AlarmClock> = Arc::new(TestClock::default());
        let sender = Arc::new(ScriptedSender::successful(clock.clone()));
        sender.responses.lock().await.extend([
            Ok(AlarmSenderResponse { status: 500 }),
            Ok(AlarmSenderResponse { status: 500 }),
            Ok(AlarmSenderResponse { status: 200 }),
        ]);
        let scheduler = AlarmScheduler::new(sender.clone(), clock, limits()).unwrap();
        let handler = definition(
            AlarmHandlerId::SmartV1,
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
            AlarmHandlerId::SmartV1,
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
        assert_eq!(sender.calls.load(Ordering::Acquire), 2);
    }

    #[tokio::test]
    async fn continuous_job_cancels_an_in_flight_send_without_new_requests() {
        let clock: Arc<dyn AlarmClock> = Arc::new(SystemAlarmClock::default());
        let sender = Arc::new(PendingSender::default());
        let scheduler = AlarmScheduler::new(sender.clone(), clock, limits()).unwrap();
        let handler = definition(
            AlarmHandlerId::SmartV1,
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
            AlarmHandlerId::SmartV1,
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
            AlarmHandlerId::SmartV1,
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
            AlarmHandlerId::SmartV1,
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
