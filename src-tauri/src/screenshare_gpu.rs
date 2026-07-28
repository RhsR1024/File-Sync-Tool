//! Windows Graphics Capture GPU preprocessing building blocks.
//!
//! The module deliberately accepts an existing D3D11 device and its immediate
//! context. It never creates a second device: WGC, the video processor, a future
//! `IMFDXGIDeviceManager`, and the hardware encoder must all use the caller's
//! device. `GpuVideoPreprocessor` is intentionally `!Send`/`!Sync` and every
//! context-using method checks its creator thread. NV12 surfaces may cross to the
//! encoder thread, but their pool lease must only be released after the encoder
//! has stopped retaining the Media Foundation buffer/sample.

use std::fmt;
use std::sync::{Arc, Mutex};

#[cfg(target_os = "windows")]
const BLACK_FRAME_PROBE_WIDTH: u32 = 64;
#[cfg(target_os = "windows")]
const BLACK_FRAME_PROBE_HEIGHT: u32 = 36;
#[cfg(target_os = "windows")]
const BLACK_FRAME_PROBE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

const NV12_TEXTURE_POOL_SIZE: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfacePhase {
    Free,
    InGpu,
    InEncoder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SurfaceLeaseKey {
    slot: usize,
    generation: u64,
    lease_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SurfaceSlotState {
    phase: SurfacePhase,
    generation: u64,
    lease_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceSlotSnapshot {
    pub slot: usize,
    pub phase: SurfacePhase,
    pub generation: u64,
    pub lease_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuFallbackCode {
    InvalidConfiguration,
    WrongContextThread,
    DeviceMismatch,
    VideoDeviceUnavailable,
    VideoContextUnavailable,
    VideoProcessorEnumeratorFailed,
    BgraInputUnsupported,
    Nv12OutputUnsupported,
    NoRateConversionMode,
    VideoProcessorCreationFailed,
    Nv12TextureCreationFailed,
    VideoProcessorInputViewFailed,
    VideoProcessorOutputViewFailed,
    SourceTextureUnsupported,
    VideoProcessorBltFailed,
    PoolExhausted,
    PoolBusy,
    StaleGeneration,
    InvalidSurfaceTransition,
    MediaFoundationDeviceManagerCreationFailed,
    MediaFoundationDeviceManagerResetFailed,
    MediaFoundationSurfaceWrapFailed,
    BlackFrameProbeCreationFailed,
    BlackFrameProbeFailed,
    StatePoisoned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuFallbackReason {
    pub code: GpuFallbackCode,
    pub operation: &'static str,
    pub hresult: Option<i32>,
    pub detail: String,
}

impl GpuFallbackReason {
    fn new(
        code: GpuFallbackCode,
        operation: &'static str,
        hresult: Option<i32>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            code,
            operation,
            hresult,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for GpuFallbackReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(hresult) = self.hresult {
            write!(
                formatter,
                "{} failed (HRESULT 0x{:08X}): {}",
                self.operation, hresult as u32, self.detail
            )
        } else {
            write!(formatter, "{} failed: {}", self.operation, self.detail)
        }
    }
}

impl std::error::Error for GpuFallbackReason {}

pub type GpuPreprocessResult<T> = Result<T, GpuFallbackReason>;

#[derive(Debug)]
struct SurfacePoolState {
    generation: u64,
    next_lease_id: u64,
    slots: [SurfaceSlotState; NV12_TEXTURE_POOL_SIZE],
}

impl SurfacePoolState {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            next_lease_id: 1,
            slots: [SurfaceSlotState {
                phase: SurfacePhase::Free,
                generation,
                lease_id: 0,
            }; NV12_TEXTURE_POOL_SIZE],
        }
    }

    fn acquire_for_gpu(&mut self, generation: u64) -> GpuPreprocessResult<SurfaceLeaseKey> {
        if generation != self.generation {
            return Err(GpuFallbackReason::new(
                GpuFallbackCode::StaleGeneration,
                "NV12 texture pool acquire",
                None,
                format!(
                    "requested generation {generation}, active generation {}",
                    self.generation
                ),
            ));
        }

        let slot = self
            .slots
            .iter()
            .position(|slot| slot.phase == SurfacePhase::Free)
            .ok_or_else(|| {
                GpuFallbackReason::new(
                    GpuFallbackCode::PoolExhausted,
                    "NV12 texture pool acquire",
                    None,
                    "all three surfaces are still owned by the GPU or encoder",
                )
            })?;

        let lease_id = self.next_lease_id;
        self.next_lease_id = self.next_lease_id.wrapping_add(1).max(1);
        self.slots[slot] = SurfaceSlotState {
            phase: SurfacePhase::InGpu,
            generation,
            lease_id,
        };
        Ok(SurfaceLeaseKey {
            slot,
            generation,
            lease_id,
        })
    }

    fn hand_off_to_encoder(&mut self, lease: SurfaceLeaseKey) -> GpuPreprocessResult<()> {
        let slot = self.validated_slot_mut(lease, SurfacePhase::InGpu)?;
        slot.phase = SurfacePhase::InEncoder;
        Ok(())
    }

    fn abort_gpu(&mut self, lease: SurfaceLeaseKey) -> GpuPreprocessResult<()> {
        let slot = self.validated_slot_mut(lease, SurfacePhase::InGpu)?;
        slot.phase = SurfacePhase::Free;
        Ok(())
    }

    fn release_encoder(&mut self, lease: SurfaceLeaseKey) -> GpuPreprocessResult<()> {
        let slot = self.validated_slot_mut(lease, SurfacePhase::InEncoder)?;
        slot.phase = SurfacePhase::Free;
        Ok(())
    }

    fn begin_generation(&mut self, generation: u64) -> GpuPreprocessResult<()> {
        if self
            .slots
            .iter()
            .any(|slot| slot.phase != SurfacePhase::Free)
        {
            return Err(GpuFallbackReason::new(
                GpuFallbackCode::PoolBusy,
                "NV12 texture pool generation switch",
                None,
                "an old-generation surface is still in GPU or encoder ownership",
            ));
        }
        self.generation = generation;
        for slot in &mut self.slots {
            slot.generation = generation;
            slot.lease_id = 0;
        }
        Ok(())
    }

    fn can_begin_generation(&self) -> bool {
        self.slots
            .iter()
            .all(|slot| slot.phase == SurfacePhase::Free)
    }

    fn snapshot(&self) -> [SurfaceSlotSnapshot; NV12_TEXTURE_POOL_SIZE] {
        std::array::from_fn(|index| {
            let slot = self.slots[index];
            SurfaceSlotSnapshot {
                slot: index,
                phase: slot.phase,
                generation: slot.generation,
                lease_id: slot.lease_id,
            }
        })
    }

    fn validated_slot_mut(
        &mut self,
        lease: SurfaceLeaseKey,
        expected_phase: SurfacePhase,
    ) -> GpuPreprocessResult<&mut SurfaceSlotState> {
        let Some(slot) = self.slots.get_mut(lease.slot) else {
            return Err(GpuFallbackReason::new(
                GpuFallbackCode::InvalidSurfaceTransition,
                "NV12 texture pool transition",
                None,
                format!("invalid slot {}", lease.slot),
            ));
        };
        if slot.generation != lease.generation || slot.lease_id != lease.lease_id {
            return Err(GpuFallbackReason::new(
                GpuFallbackCode::StaleGeneration,
                "NV12 texture pool transition",
                None,
                format!(
                    "stale lease generation/id {}/{}, current {}/{}",
                    lease.generation, lease.lease_id, slot.generation, slot.lease_id
                ),
            ));
        }
        if slot.phase != expected_phase {
            return Err(GpuFallbackReason::new(
                GpuFallbackCode::InvalidSurfaceTransition,
                "NV12 texture pool transition",
                None,
                format!(
                    "slot {} is {:?}, expected {:?}",
                    lease.slot, slot.phase, expected_phase
                ),
            ));
        }
        Ok(slot)
    }
}

#[cfg(target_os = "windows")]
mod windows_pipeline {
    use super::*;
    use std::marker::PhantomData;
    use std::mem::ManuallyDrop;
    use std::rc::Rc;
    use std::thread::{self, ThreadId};
    use std::time::Instant;
    use windows::Win32::Foundation::{BOOL, FALSE, RECT, TRUE};
    use windows::Win32::Graphics::Direct3D11::{
        ID3D11Device, ID3D11DeviceContext, ID3D11Query, ID3D11Resource, ID3D11Texture2D,
        ID3D11VideoContext, ID3D11VideoDevice, ID3D11VideoProcessor,
        ID3D11VideoProcessorEnumerator, ID3D11VideoProcessorOutputView, D3D11_BIND_RENDER_TARGET,
        D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE,
        D3D11_MAP_READ, D3D11_QUERY_DESC, D3D11_QUERY_EVENT, D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV,
        D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
        D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, D3D11_VIDEO_PROCESSOR_CAPS,
        D3D11_VIDEO_PROCESSOR_COLOR_SPACE, D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
        D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT, D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT,
        D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
        D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC, D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
        D3D11_VIDEO_PROCESSOR_STREAM, D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
        D3D11_VPIV_DIMENSION_TEXTURE2D, D3D11_VPOV_DIMENSION_TEXTURE2D,
    };
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
    };
    use windows::Win32::Media::MediaFoundation::{
        IMFDXGIDeviceManager, IMFMediaBuffer, MFCreateDXGIDeviceManager, MFCreateDXGISurfaceBuffer,
    };
    use windows_core::Interface;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct GpuPreprocessConfig {
        pub input_width: u32,
        pub input_height: u32,
        pub output_width: u32,
        pub output_height: u32,
        pub frame_rate_numerator: u32,
        pub frame_rate_denominator: u32,
        pub generation: u64,
    }

    impl GpuPreprocessConfig {
        fn validate(self) -> GpuPreprocessResult<Self> {
            if self.input_width == 0
                || self.input_height == 0
                || self.output_width == 0
                || self.output_height == 0
                || self.frame_rate_numerator == 0
                || self.frame_rate_denominator == 0
            {
                return Err(GpuFallbackReason::new(
                    GpuFallbackCode::InvalidConfiguration,
                    "GPU preprocessor configuration",
                    None,
                    "dimensions and frame-rate numerator/denominator must be non-zero",
                ));
            }
            if self.output_width % 2 != 0 || self.output_height % 2 != 0 {
                return Err(GpuFallbackReason::new(
                    GpuFallbackCode::InvalidConfiguration,
                    "GPU preprocessor configuration",
                    None,
                    "NV12 output width and height must be even",
                ));
            }
            Ok(self)
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct GpuVideoProcessorCapabilities {
        pub bgra_input: bool,
        pub nv12_output: bool,
        pub rate_conversion_modes: u32,
        pub pool_size: usize,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum GpuBlackFrameProbeStatus {
        Implemented {
            width: u32,
            height: u32,
            interval_ms: u64,
            asynchronous: bool,
        },
    }

    pub fn black_frame_probe_status() -> GpuBlackFrameProbeStatus {
        GpuBlackFrameProbeStatus::Implemented {
            width: BLACK_FRAME_PROBE_WIDTH,
            height: BLACK_FRAME_PROBE_HEIGHT,
            interval_ms: BLACK_FRAME_PROBE_INTERVAL.as_millis() as u64,
            asynchronous: true,
        }
    }

    /// Media Foundation device manager bound to the exact D3D11 device supplied
    /// by WGC. The caller passes `manager` to an MFT through
    /// `MFT_MESSAGE_SET_D3D_MANAGER`; this helper never creates another device.
    pub struct MfDxgiDeviceManager {
        pub manager: IMFDXGIDeviceManager,
        pub reset_token: u32,
    }

    pub fn create_mf_dxgi_device_manager(
        device: &ID3D11Device,
    ) -> GpuPreprocessResult<MfDxgiDeviceManager> {
        let mut reset_token = 0;
        let mut manager = None;
        unsafe { MFCreateDXGIDeviceManager(&mut reset_token, &mut manager) }.map_err(|error| {
            windows_error(
                GpuFallbackCode::MediaFoundationDeviceManagerCreationFailed,
                "MFCreateDXGIDeviceManager",
                error,
            )
        })?;
        let manager = manager.ok_or_else(|| {
            GpuFallbackReason::new(
                GpuFallbackCode::MediaFoundationDeviceManagerCreationFailed,
                "MFCreateDXGIDeviceManager",
                None,
                "Media Foundation returned success without a device manager",
            )
        })?;
        unsafe { manager.ResetDevice(device, reset_token) }.map_err(|error| {
            windows_error(
                GpuFallbackCode::MediaFoundationDeviceManagerResetFailed,
                "IMFDXGIDeviceManager::ResetDevice",
                error,
            )
        })?;
        Ok(MfDxgiDeviceManager {
            manager,
            reset_token,
        })
    }

    struct Nv12Slot {
        texture: ID3D11Texture2D,
        output_view: ID3D11VideoProcessorOutputView,
    }

    struct PipelineResources {
        enumerator: ID3D11VideoProcessorEnumerator,
        processor: ID3D11VideoProcessor,
        slots: Vec<Nv12Slot>,
        capabilities: GpuVideoProcessorCapabilities,
    }

    struct AsyncBlackFrameProbe {
        enumerator: ID3D11VideoProcessorEnumerator,
        processor: ID3D11VideoProcessor,
        output_texture: ID3D11Texture2D,
        output_view: ID3D11VideoProcessorOutputView,
        staging_texture: ID3D11Texture2D,
        completion_query: ID3D11Query,
        in_flight: bool,
        last_submitted_at: Option<Instant>,
        last_result: Option<bool>,
    }

    /// A D3D11 VideoProcessor bound to the WGC caller's device and immediate
    /// context. `&mut self` plus the `Rc` marker and runtime thread check make the
    /// immediate-context single-thread rule explicit.
    pub struct GpuVideoPreprocessor {
        device: ID3D11Device,
        context: ID3D11DeviceContext,
        video_device: ID3D11VideoDevice,
        video_context: ID3D11VideoContext,
        config: GpuPreprocessConfig,
        resources: PipelineResources,
        black_probe: AsyncBlackFrameProbe,
        pool: Arc<Mutex<SurfacePoolState>>,
        owner_thread: ThreadId,
        _context_thread_only: PhantomData<Rc<()>>,
    }

    impl GpuVideoPreprocessor {
        pub fn new(
            device: ID3D11Device,
            context: ID3D11DeviceContext,
            config: GpuPreprocessConfig,
        ) -> GpuPreprocessResult<Self> {
            let config = config.validate()?;
            let context_device = unsafe { context.GetDevice() }.map_err(|error| {
                windows_error(
                    GpuFallbackCode::DeviceMismatch,
                    "ID3D11DeviceContext::GetDevice",
                    error,
                )
            })?;
            if context_device.as_raw() != device.as_raw() {
                return Err(GpuFallbackReason::new(
                    GpuFallbackCode::DeviceMismatch,
                    "GPU preprocessor device validation",
                    None,
                    "the immediate context does not belong to the supplied WGC device",
                ));
            }

            let video_device: ID3D11VideoDevice = device.cast().map_err(|error| {
                windows_error(
                    GpuFallbackCode::VideoDeviceUnavailable,
                    "ID3D11Device::cast(ID3D11VideoDevice)",
                    error,
                )
            })?;
            let video_context: ID3D11VideoContext = context.cast().map_err(|error| {
                windows_error(
                    GpuFallbackCode::VideoContextUnavailable,
                    "ID3D11DeviceContext::cast(ID3D11VideoContext)",
                    error,
                )
            })?;
            let resources = build_resources(&device, &video_device, config)?;
            let black_probe = build_black_frame_probe(&device, &video_device, config)?;
            configure_bgra_to_nv12_color_space(&video_context, &resources.processor);

            Ok(Self {
                device,
                context,
                video_device,
                video_context,
                config,
                resources,
                black_probe,
                pool: Arc::new(Mutex::new(SurfacePoolState::new(config.generation))),
                owner_thread: thread::current().id(),
                _context_thread_only: PhantomData,
            })
        }

        pub fn capabilities(&self) -> GpuVideoProcessorCapabilities {
            self.resources.capabilities
        }

        pub fn config(&self) -> GpuPreprocessConfig {
            self.config
        }

        pub fn pool_snapshot(
            &self,
        ) -> GpuPreprocessResult<[SurfaceSlotSnapshot; NV12_TEXTURE_POOL_SIZE]> {
            self.pool
                .lock()
                .map(|pool| pool.snapshot())
                .map_err(|_| state_poisoned("NV12 texture pool snapshot"))
        }

        /// Rebuilds size-dependent VideoProcessor resources. It is rejected while
        /// any old-generation surface is still in GPU/encoder ownership.
        pub fn reconfigure(&mut self, config: GpuPreprocessConfig) -> GpuPreprocessResult<()> {
            self.ensure_context_thread()?;
            let config = config.validate()?;
            {
                let pool = self
                    .pool
                    .lock()
                    .map_err(|_| state_poisoned("NV12 texture pool reconfigure check"))?;
                if !pool.can_begin_generation() {
                    return Err(GpuFallbackReason::new(
                        GpuFallbackCode::PoolBusy,
                        "GPU preprocessor reconfigure",
                        None,
                        "old-generation surfaces must be released before reconfigure",
                    ));
                }
            }

            let resources = build_resources(&self.device, &self.video_device, config)?;
            let black_probe = build_black_frame_probe(&self.device, &self.video_device, config)?;
            configure_bgra_to_nv12_color_space(&self.video_context, &resources.processor);
            self.pool
                .lock()
                .map_err(|_| state_poisoned("NV12 texture pool generation switch"))?
                .begin_generation(config.generation)?;
            self.config = config;
            self.resources = resources;
            self.black_probe = black_probe;
            Ok(())
        }

        /// Polls the previous tiny readback without waiting, then schedules a new
        /// 64x36 BGRA probe at no more than 2 FPS. The full-size WGC surface is
        /// never mapped and an incomplete GPU query simply keeps the last result.
        pub fn poll_black_frame_probe(
            &mut self,
            source: &ID3D11Texture2D,
            now: Instant,
        ) -> GpuPreprocessResult<Option<bool>> {
            self.ensure_context_thread()?;
            self.validate_source(source)?;

            if self.black_probe.in_flight {
                let mut completed = FALSE;
                unsafe {
                    self.context
                        .GetData(
                            &self.black_probe.completion_query,
                            Some((&mut completed as *mut BOOL).cast()),
                            std::mem::size_of_val(&completed) as u32,
                            windows::Win32::Graphics::Direct3D11::D3D11_ASYNC_GETDATA_DONOTFLUSH.0
                                as u32,
                        )
                        .map_err(|error| {
                            windows_error(
                                GpuFallbackCode::BlackFrameProbeFailed,
                                "ID3D11DeviceContext::GetData(black-frame probe)",
                                error,
                            )
                        })?;
                }
                if completed.as_bool() {
                    self.black_probe.last_result = Some(read_black_probe_staging(
                        &self.context,
                        &self.black_probe.staging_texture,
                    )?);
                    self.black_probe.in_flight = false;
                }
            }

            let due = !self.black_probe.in_flight
                && self.black_probe.last_submitted_at.is_none_or(|submitted| {
                    now.saturating_duration_since(submitted) >= BLACK_FRAME_PROBE_INTERVAL
                });
            if due {
                submit_black_frame_probe(
                    &self.video_device,
                    &self.video_context,
                    &self.context,
                    source,
                    self.config,
                    &mut self.black_probe,
                )?;
                self.black_probe.last_submitted_at = Some(now);
                self.black_probe.in_flight = true;
            }

            Ok(self.black_probe.last_result)
        }

        /// Queues BGRA -> NV12 conversion/scaling on the caller-owned immediate
        /// context and returns a surface that remains non-recyclable until the
        /// encoder explicitly releases it.
        pub fn preprocess(
            &mut self,
            source: &ID3D11Texture2D,
        ) -> GpuPreprocessResult<GpuNv12Surface> {
            self.ensure_context_thread()?;
            self.validate_source(source)?;

            let lease = self
                .pool
                .lock()
                .map_err(|_| state_poisoned("NV12 texture pool acquire"))?
                .acquire_for_gpu(self.config.generation)?;

            let result = self.video_processor_blt(source, lease.slot);
            if let Err(error) = result {
                let _ = self
                    .pool
                    .lock()
                    .map_err(|_| state_poisoned("NV12 texture pool abort"))?
                    .abort_gpu(lease);
                return Err(error);
            }

            self.pool
                .lock()
                .map_err(|_| state_poisoned("NV12 texture pool handoff"))?
                .hand_off_to_encoder(lease)?;

            Ok(GpuNv12Surface {
                texture: self.resources.slots[lease.slot].texture.clone(),
                width: self.config.output_width,
                height: self.config.output_height,
                generation: lease.generation,
                lease,
                pool: Arc::clone(&self.pool),
                released: false,
            })
        }

        fn ensure_context_thread(&self) -> GpuPreprocessResult<()> {
            if thread::current().id() == self.owner_thread {
                return Ok(());
            }
            Err(GpuFallbackReason::new(
                GpuFallbackCode::WrongContextThread,
                "D3D11 immediate context access",
                None,
                "GpuVideoPreprocessor must only be called from its creator thread",
            ))
        }

        fn validate_source(&self, source: &ID3D11Texture2D) -> GpuPreprocessResult<()> {
            let source_device = unsafe { source.GetDevice() }.map_err(|error| {
                windows_error(
                    GpuFallbackCode::DeviceMismatch,
                    "ID3D11Texture2D::GetDevice",
                    error,
                )
            })?;
            if source_device.as_raw() != self.device.as_raw() {
                return Err(GpuFallbackReason::new(
                    GpuFallbackCode::DeviceMismatch,
                    "GPU source texture validation",
                    None,
                    "WGC source texture belongs to a different D3D11 device",
                ));
            }

            let mut descriptor = D3D11_TEXTURE2D_DESC::default();
            unsafe { source.GetDesc(&mut descriptor) };
            if descriptor.Format != DXGI_FORMAT_B8G8R8A8_UNORM
                || descriptor.Width != self.config.input_width
                || descriptor.Height != self.config.input_height
            {
                return Err(GpuFallbackReason::new(
                    GpuFallbackCode::SourceTextureUnsupported,
                    "GPU source texture validation",
                    None,
                    format!(
                        "expected BGRA {}x{}, got DXGI format {} {}x{}",
                        self.config.input_width,
                        self.config.input_height,
                        descriptor.Format.0,
                        descriptor.Width,
                        descriptor.Height
                    ),
                ));
            }
            Ok(())
        }

        fn video_processor_blt(
            &mut self,
            source: &ID3D11Texture2D,
            output_slot: usize,
        ) -> GpuPreprocessResult<()> {
            let input_descriptor = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                FourCC: 0,
                ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VPIV {
                        MipSlice: 0,
                        ArraySlice: 0,
                    },
                },
            };
            let mut input_view = None;
            unsafe {
                self.video_device.CreateVideoProcessorInputView(
                    source,
                    &self.resources.enumerator,
                    &input_descriptor,
                    Some(&mut input_view),
                )
            }
            .map_err(|error| {
                windows_error(
                    GpuFallbackCode::VideoProcessorInputViewFailed,
                    "ID3D11VideoDevice::CreateVideoProcessorInputView",
                    error,
                )
            })?;
            let input_view = input_view.ok_or_else(|| {
                GpuFallbackReason::new(
                    GpuFallbackCode::VideoProcessorInputViewFailed,
                    "ID3D11VideoDevice::CreateVideoProcessorInputView",
                    None,
                    "Windows returned success without an input view",
                )
            })?;

            let source_rect = RECT {
                left: 0,
                top: 0,
                right: self.config.input_width as i32,
                bottom: self.config.input_height as i32,
            };
            let destination_rect = RECT {
                left: 0,
                top: 0,
                right: self.config.output_width as i32,
                bottom: self.config.output_height as i32,
            };
            unsafe {
                self.video_context.VideoProcessorSetStreamFrameFormat(
                    &self.resources.processor,
                    0,
                    D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                );
                self.video_context.VideoProcessorSetStreamSourceRect(
                    &self.resources.processor,
                    0,
                    TRUE,
                    Some(&source_rect),
                );
                self.video_context.VideoProcessorSetStreamDestRect(
                    &self.resources.processor,
                    0,
                    TRUE,
                    Some(&destination_rect),
                );
                self.video_context.VideoProcessorSetOutputTargetRect(
                    &self.resources.processor,
                    TRUE,
                    Some(&destination_rect),
                );
            }

            let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
                Enable: TRUE,
                pInputSurface: ManuallyDrop::new(Some(input_view)),
                ..Default::default()
            };
            let blt_result = unsafe {
                self.video_context.VideoProcessorBlt(
                    &self.resources.processor,
                    &self.resources.slots[output_slot].output_view,
                    0,
                    std::slice::from_ref(&stream),
                )
            };
            // windows-rs models the COM pointers in this ABI struct as
            // ManuallyDrop because the native call only borrows them.
            unsafe { ManuallyDrop::drop(&mut stream.pInputSurface) };
            blt_result.map_err(|error| {
                windows_error(
                    GpuFallbackCode::VideoProcessorBltFailed,
                    "ID3D11VideoContext::VideoProcessorBlt",
                    error,
                )
            })
        }
    }

    fn configure_bgra_to_nv12_color_space(
        context: &ID3D11VideoContext,
        processor: &ID3D11VideoProcessor,
    ) {
        // Match the scalar/SIMD fallback coefficients: full-range RGB input,
        // BT.601 matrix, limited-range NV12 output. Leaving the bitfields at
        // their all-zero defaults makes nominal range driver-dependent and can
        // produce washed-out fallback transitions on older Intel drivers.
        let rgb_full = D3D11_VIDEO_PROCESSOR_COLOR_SPACE { _bitfield: 0 };
        let yuv_bt601_limited = D3D11_VIDEO_PROCESSOR_COLOR_SPACE {
            // Nominal_Range = D3D11_VIDEO_PROCESSOR_NOMINAL_RANGE_16_235 (1).
            _bitfield: 1 << 4,
        };
        unsafe {
            context.VideoProcessorSetStreamColorSpace(processor, 0, &rgb_full);
            context.VideoProcessorSetOutputColorSpace(processor, &yuv_bt601_limited);
        }
    }

    pub struct GpuNv12Surface {
        texture: ID3D11Texture2D,
        width: u32,
        height: u32,
        generation: u64,
        lease: SurfaceLeaseKey,
        pool: Arc<Mutex<SurfacePoolState>>,
        released: bool,
    }

    impl GpuNv12Surface {
        pub fn texture(&self) -> &ID3D11Texture2D {
            &self.texture
        }

        pub fn width(&self) -> u32 {
            self.width
        }

        pub fn height(&self) -> u32 {
            self.height
        }

        pub fn generation(&self) -> u64 {
            self.generation
        }

        /// Wraps this exact texture for a Media Foundation sample. No device or
        /// copy is created. The returned buffer retains the texture.
        pub fn create_mf_surface_buffer(&self) -> GpuPreprocessResult<IMFMediaBuffer> {
            let buffer = unsafe {
                MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &self.texture, 0, FALSE)
            }
            .map_err(|error| {
                windows_error(
                    GpuFallbackCode::MediaFoundationSurfaceWrapFailed,
                    "MFCreateDXGISurfaceBuffer",
                    error,
                )
            })?;
            let buffer_length = unsafe { buffer.GetMaxLength() }.map_err(|error| {
                windows_error(
                    GpuFallbackCode::MediaFoundationSurfaceWrapFailed,
                    "IMFMediaBuffer::GetMaxLength(DXGI surface)",
                    error,
                )
            })?;
            unsafe { buffer.SetCurrentLength(buffer_length) }.map_err(|error| {
                windows_error(
                    GpuFallbackCode::MediaFoundationSurfaceWrapFailed,
                    "IMFMediaBuffer::SetCurrentLength(DXGI surface)",
                    error,
                )
            })?;
            Ok(buffer)
        }

        /// Recycles the pool slot. The caller must invoke this only after the MFT
        /// no longer retains the corresponding buffer/sample. Dropping a surface
        /// without calling this method intentionally leaves the slot occupied,
        /// preferring bounded backpressure over an unsafe early reuse.
        pub fn release_after_encoder_done(mut self) -> GpuPreprocessResult<()> {
            let result = self
                .pool
                .lock()
                .map_err(|_| state_poisoned("NV12 texture pool encoder release"))?
                .release_encoder(self.lease);
            match result {
                Ok(()) => {
                    self.released = true;
                    Ok(())
                }
                Err(error) => {
                    // Most call sites run on shutdown/error paths where there is
                    // no useful error return channel. Never let a failed recycle
                    // silently turn into a permanently exhausted three-slot pool.
                    log::error!(
                        "screen_share_gpu_surface_release failed code={:?} operation={} hresult={:?} detail={}",
                        error.code,
                        error.operation,
                        error.hresult,
                        error.detail,
                    );
                    Err(error)
                }
            }
        }
    }

    impl Drop for GpuNv12Surface {
        fn drop(&mut self) {
            // See release_after_encoder_done: an implicit release cannot prove
            // that an asynchronous MFT has stopped referencing this texture.
            let _ = self.released;
        }
    }

    fn build_black_frame_probe(
        device: &ID3D11Device,
        video_device: &ID3D11VideoDevice,
        config: GpuPreprocessConfig,
    ) -> GpuPreprocessResult<AsyncBlackFrameProbe> {
        let descriptor = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: DXGI_RATIONAL {
                Numerator: config.frame_rate_numerator,
                Denominator: config.frame_rate_denominator,
            },
            InputWidth: config.input_width,
            InputHeight: config.input_height,
            OutputFrameRate: DXGI_RATIONAL {
                Numerator: config.frame_rate_numerator,
                Denominator: config.frame_rate_denominator,
            },
            OutputWidth: BLACK_FRAME_PROBE_WIDTH,
            OutputHeight: BLACK_FRAME_PROBE_HEIGHT,
            Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
        };
        let enumerator = unsafe { video_device.CreateVideoProcessorEnumerator(&descriptor) }
            .map_err(|error| {
                windows_error(
                    GpuFallbackCode::BlackFrameProbeCreationFailed,
                    "ID3D11VideoDevice::CreateVideoProcessorEnumerator(black-frame probe)",
                    error,
                )
            })?;
        let bgra_support =
            unsafe { enumerator.CheckVideoProcessorFormat(DXGI_FORMAT_B8G8R8A8_UNORM) }.map_err(
                |error| {
                    windows_error(
                        GpuFallbackCode::BlackFrameProbeCreationFailed,
                        "CheckVideoProcessorFormat(BGRA black-frame probe)",
                        error,
                    )
                },
            )?;
        if bgra_support & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT.0 as u32 == 0 {
            return Err(GpuFallbackReason::new(
                GpuFallbackCode::BlackFrameProbeCreationFailed,
                "CheckVideoProcessorFormat(BGRA black-frame probe)",
                None,
                "the WGC D3D11 device cannot produce a tiny BGRA probe surface",
            ));
        }
        let processor =
            unsafe { video_device.CreateVideoProcessor(&enumerator, 0) }.map_err(|error| {
                windows_error(
                    GpuFallbackCode::BlackFrameProbeCreationFailed,
                    "ID3D11VideoDevice::CreateVideoProcessor(black-frame probe)",
                    error,
                )
            })?;

        let output_descriptor = D3D11_TEXTURE2D_DESC {
            Width: BLACK_FRAME_PROBE_WIDTH,
            Height: BLACK_FRAME_PROBE_HEIGHT,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut output_texture = None;
        unsafe { device.CreateTexture2D(&output_descriptor, None, Some(&mut output_texture)) }
            .map_err(|error| {
                windows_error(
                    GpuFallbackCode::BlackFrameProbeCreationFailed,
                    "ID3D11Device::CreateTexture2D(black-frame probe output)",
                    error,
                )
            })?;
        let output_texture = output_texture.ok_or_else(|| {
            GpuFallbackReason::new(
                GpuFallbackCode::BlackFrameProbeCreationFailed,
                "ID3D11Device::CreateTexture2D(black-frame probe output)",
                None,
                "Windows returned success without a texture",
            )
        })?;
        let output_view_descriptor = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };
        let mut output_view = None;
        unsafe {
            video_device.CreateVideoProcessorOutputView(
                &output_texture,
                &enumerator,
                &output_view_descriptor,
                Some(&mut output_view),
            )
        }
        .map_err(|error| {
            windows_error(
                GpuFallbackCode::BlackFrameProbeCreationFailed,
                "ID3D11VideoDevice::CreateVideoProcessorOutputView(black-frame probe)",
                error,
            )
        })?;
        let output_view = output_view.ok_or_else(|| {
            GpuFallbackReason::new(
                GpuFallbackCode::BlackFrameProbeCreationFailed,
                "ID3D11VideoDevice::CreateVideoProcessorOutputView(black-frame probe)",
                None,
                "Windows returned success without an output view",
            )
        })?;

        let staging_descriptor = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            ..output_descriptor
        };
        let mut staging_texture = None;
        unsafe { device.CreateTexture2D(&staging_descriptor, None, Some(&mut staging_texture)) }
            .map_err(|error| {
                windows_error(
                    GpuFallbackCode::BlackFrameProbeCreationFailed,
                    "ID3D11Device::CreateTexture2D(black-frame probe staging)",
                    error,
                )
            })?;
        let staging_texture = staging_texture.ok_or_else(|| {
            GpuFallbackReason::new(
                GpuFallbackCode::BlackFrameProbeCreationFailed,
                "ID3D11Device::CreateTexture2D(black-frame probe staging)",
                None,
                "Windows returned success without a staging texture",
            )
        })?;
        let query_descriptor = D3D11_QUERY_DESC {
            Query: D3D11_QUERY_EVENT,
            MiscFlags: 0,
        };
        let mut completion_query = None;
        unsafe { device.CreateQuery(&query_descriptor, Some(&mut completion_query)) }.map_err(
            |error| {
                windows_error(
                    GpuFallbackCode::BlackFrameProbeCreationFailed,
                    "ID3D11Device::CreateQuery(black-frame probe)",
                    error,
                )
            },
        )?;
        let completion_query = completion_query.ok_or_else(|| {
            GpuFallbackReason::new(
                GpuFallbackCode::BlackFrameProbeCreationFailed,
                "ID3D11Device::CreateQuery(black-frame probe)",
                None,
                "Windows returned success without a query",
            )
        })?;

        Ok(AsyncBlackFrameProbe {
            enumerator,
            processor,
            output_texture,
            output_view,
            staging_texture,
            completion_query,
            in_flight: false,
            last_submitted_at: None,
            last_result: None,
        })
    }

    fn submit_black_frame_probe(
        video_device: &ID3D11VideoDevice,
        video_context: &ID3D11VideoContext,
        context: &ID3D11DeviceContext,
        source: &ID3D11Texture2D,
        config: GpuPreprocessConfig,
        probe: &mut AsyncBlackFrameProbe,
    ) -> GpuPreprocessResult<()> {
        let input_descriptor = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let mut input_view = None;
        unsafe {
            video_device.CreateVideoProcessorInputView(
                source,
                &probe.enumerator,
                &input_descriptor,
                Some(&mut input_view),
            )
        }
        .map_err(|error| {
            windows_error(
                GpuFallbackCode::BlackFrameProbeFailed,
                "ID3D11VideoDevice::CreateVideoProcessorInputView(black-frame probe)",
                error,
            )
        })?;
        let input_view = input_view.ok_or_else(|| {
            GpuFallbackReason::new(
                GpuFallbackCode::BlackFrameProbeFailed,
                "ID3D11VideoDevice::CreateVideoProcessorInputView(black-frame probe)",
                None,
                "Windows returned success without an input view",
            )
        })?;
        let source_rect = RECT {
            left: 0,
            top: 0,
            right: config.input_width as i32,
            bottom: config.input_height as i32,
        };
        let destination_rect = RECT {
            left: 0,
            top: 0,
            right: BLACK_FRAME_PROBE_WIDTH as i32,
            bottom: BLACK_FRAME_PROBE_HEIGHT as i32,
        };
        unsafe {
            video_context.VideoProcessorSetStreamFrameFormat(
                &probe.processor,
                0,
                D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            );
            video_context.VideoProcessorSetStreamSourceRect(
                &probe.processor,
                0,
                TRUE,
                Some(&source_rect),
            );
            video_context.VideoProcessorSetStreamDestRect(
                &probe.processor,
                0,
                TRUE,
                Some(&destination_rect),
            );
            video_context.VideoProcessorSetOutputTargetRect(
                &probe.processor,
                TRUE,
                Some(&destination_rect),
            );
        }
        let mut stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: TRUE,
            pInputSurface: ManuallyDrop::new(Some(input_view)),
            ..Default::default()
        };
        let result = unsafe {
            video_context.VideoProcessorBlt(
                &probe.processor,
                &probe.output_view,
                0,
                std::slice::from_ref(&stream),
            )
        };
        unsafe { ManuallyDrop::drop(&mut stream.pInputSurface) };
        result.map_err(|error| {
            windows_error(
                GpuFallbackCode::BlackFrameProbeFailed,
                "ID3D11VideoContext::VideoProcessorBlt(black-frame probe)",
                error,
            )
        })?;
        let output_resource: ID3D11Resource = probe.output_texture.cast().map_err(|error| {
            windows_error(
                GpuFallbackCode::BlackFrameProbeFailed,
                "ID3D11Texture2D::cast(black-frame probe output)",
                error,
            )
        })?;
        let staging_resource: ID3D11Resource = probe.staging_texture.cast().map_err(|error| {
            windows_error(
                GpuFallbackCode::BlackFrameProbeFailed,
                "ID3D11Texture2D::cast(black-frame probe staging)",
                error,
            )
        })?;
        unsafe {
            context.CopyResource(&staging_resource, &output_resource);
            context.End(&probe.completion_query);
        }
        Ok(())
    }

    fn read_black_probe_staging(
        context: &ID3D11DeviceContext,
        staging: &ID3D11Texture2D,
    ) -> GpuPreprocessResult<bool> {
        let resource: ID3D11Resource = staging.cast().map_err(|error| {
            windows_error(
                GpuFallbackCode::BlackFrameProbeFailed,
                "ID3D11Texture2D::cast(black-frame probe map)",
                error,
            )
        })?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { context.Map(&resource, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }.map_err(
            |error| {
                windows_error(
                    GpuFallbackCode::BlackFrameProbeFailed,
                    "ID3D11DeviceContext::Map(black-frame probe)",
                    error,
                )
            },
        )?;
        let mut sampled = 0usize;
        let mut bright = 0usize;
        if !mapped.pData.is_null() {
            for y in 0..BLACK_FRAME_PROBE_HEIGHT as usize {
                let row = unsafe {
                    std::slice::from_raw_parts(
                        (mapped.pData as *const u8).add(y * mapped.RowPitch as usize),
                        BLACK_FRAME_PROBE_WIDTH as usize * 4,
                    )
                };
                for pixel in row.chunks_exact(4) {
                    sampled += 1;
                    if pixel[0].max(pixel[1]).max(pixel[2]) > 12 {
                        bright += 1;
                    }
                }
            }
        }
        unsafe { context.Unmap(&resource, 0) };
        Ok(sampled > 0 && bright * 10_000 <= sampled * 35)
    }

    fn build_resources(
        device: &ID3D11Device,
        video_device: &ID3D11VideoDevice,
        config: GpuPreprocessConfig,
    ) -> GpuPreprocessResult<PipelineResources> {
        let content_descriptor = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: DXGI_RATIONAL {
                Numerator: config.frame_rate_numerator,
                Denominator: config.frame_rate_denominator,
            },
            InputWidth: config.input_width,
            InputHeight: config.input_height,
            OutputFrameRate: DXGI_RATIONAL {
                Numerator: config.frame_rate_numerator,
                Denominator: config.frame_rate_denominator,
            },
            OutputWidth: config.output_width,
            OutputHeight: config.output_height,
            Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
        };
        let enumerator =
            unsafe { video_device.CreateVideoProcessorEnumerator(&content_descriptor) }.map_err(
                |error| {
                    windows_error(
                        GpuFallbackCode::VideoProcessorEnumeratorFailed,
                        "ID3D11VideoDevice::CreateVideoProcessorEnumerator",
                        error,
                    )
                },
            )?;

        let bgra_support =
            unsafe { enumerator.CheckVideoProcessorFormat(DXGI_FORMAT_B8G8R8A8_UNORM) }.map_err(
                |error| {
                    windows_error(
                        GpuFallbackCode::BgraInputUnsupported,
                        "CheckVideoProcessorFormat(BGRA)",
                        error,
                    )
                },
            )?;
        if bgra_support & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_INPUT.0 as u32 == 0 {
            return Err(GpuFallbackReason::new(
                GpuFallbackCode::BgraInputUnsupported,
                "CheckVideoProcessorFormat(BGRA)",
                None,
                format!("format support flags were 0x{bgra_support:08X}"),
            ));
        }

        let nv12_support = unsafe { enumerator.CheckVideoProcessorFormat(DXGI_FORMAT_NV12) }
            .map_err(|error| {
                windows_error(
                    GpuFallbackCode::Nv12OutputUnsupported,
                    "CheckVideoProcessorFormat(NV12)",
                    error,
                )
            })?;
        if nv12_support & D3D11_VIDEO_PROCESSOR_FORMAT_SUPPORT_OUTPUT.0 as u32 == 0 {
            return Err(GpuFallbackReason::new(
                GpuFallbackCode::Nv12OutputUnsupported,
                "CheckVideoProcessorFormat(NV12)",
                None,
                format!("format support flags were 0x{nv12_support:08X}"),
            ));
        }

        let mut processor_caps = D3D11_VIDEO_PROCESSOR_CAPS::default();
        unsafe { enumerator.GetVideoProcessorCaps(&mut processor_caps) }.map_err(|error| {
            windows_error(
                GpuFallbackCode::NoRateConversionMode,
                "ID3D11VideoProcessorEnumerator::GetVideoProcessorCaps",
                error,
            )
        })?;
        if processor_caps.RateConversionCapsCount == 0 {
            return Err(GpuFallbackReason::new(
                GpuFallbackCode::NoRateConversionMode,
                "ID3D11VideoProcessorEnumerator::GetVideoProcessorCaps",
                None,
                "driver exposed no rate-conversion mode",
            ));
        }

        let processor =
            unsafe { video_device.CreateVideoProcessor(&enumerator, 0) }.map_err(|error| {
                windows_error(
                    GpuFallbackCode::VideoProcessorCreationFailed,
                    "ID3D11VideoDevice::CreateVideoProcessor",
                    error,
                )
            })?;

        let mut slots = Vec::with_capacity(NV12_TEXTURE_POOL_SIZE);
        for _ in 0..NV12_TEXTURE_POOL_SIZE {
            let descriptor = D3D11_TEXTURE2D_DESC {
                Width: config.output_width,
                Height: config.output_height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_NV12,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
            };
            let mut texture = None;
            unsafe { device.CreateTexture2D(&descriptor, None, Some(&mut texture)) }.map_err(
                |error| {
                    windows_error(
                        GpuFallbackCode::Nv12TextureCreationFailed,
                        "ID3D11Device::CreateTexture2D(NV12)",
                        error,
                    )
                },
            )?;
            let texture = texture.ok_or_else(|| {
                GpuFallbackReason::new(
                    GpuFallbackCode::Nv12TextureCreationFailed,
                    "ID3D11Device::CreateTexture2D(NV12)",
                    None,
                    "Windows returned success without a texture",
                )
            })?;

            let output_descriptor = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
                },
            };
            let mut output_view = None;
            unsafe {
                video_device.CreateVideoProcessorOutputView(
                    &texture,
                    &enumerator,
                    &output_descriptor,
                    Some(&mut output_view),
                )
            }
            .map_err(|error| {
                windows_error(
                    GpuFallbackCode::VideoProcessorOutputViewFailed,
                    "ID3D11VideoDevice::CreateVideoProcessorOutputView",
                    error,
                )
            })?;
            let output_view = output_view.ok_or_else(|| {
                GpuFallbackReason::new(
                    GpuFallbackCode::VideoProcessorOutputViewFailed,
                    "ID3D11VideoDevice::CreateVideoProcessorOutputView",
                    None,
                    "Windows returned success without an output view",
                )
            })?;
            slots.push(Nv12Slot {
                texture,
                output_view,
            });
        }

        Ok(PipelineResources {
            enumerator,
            processor,
            slots,
            capabilities: GpuVideoProcessorCapabilities {
                bgra_input: true,
                nv12_output: true,
                rate_conversion_modes: processor_caps.RateConversionCapsCount,
                pool_size: NV12_TEXTURE_POOL_SIZE,
            },
        })
    }

    fn windows_error(
        code: GpuFallbackCode,
        operation: &'static str,
        error: windows_core::Error,
    ) -> GpuFallbackReason {
        GpuFallbackReason::new(code, operation, Some(error.code().0), error.message())
    }
}

#[cfg(target_os = "windows")]
#[allow(unused_imports)]
// The adapter is exported now and wired into WGC/MFT in the next M3 step.
pub use windows_pipeline::{
    black_frame_probe_status, create_mf_dxgi_device_manager, GpuBlackFrameProbeStatus,
    GpuNv12Surface, GpuPreprocessConfig, GpuVideoPreprocessor, GpuVideoProcessorCapabilities,
    MfDxgiDeviceManager,
};

fn state_poisoned(operation: &'static str) -> GpuFallbackReason {
    GpuFallbackReason::new(
        GpuFallbackCode::StatePoisoned,
        operation,
        None,
        "surface pool mutex was poisoned",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
    #[test]
    fn black_frame_probe_is_small_rate_limited_and_asynchronous() {
        assert_eq!(
            black_frame_probe_status(),
            GpuBlackFrameProbeStatus::Implemented {
                width: 64,
                height: 36,
                interval_ms: 500,
                asynchronous: true,
            }
        );
    }

    #[test]
    fn pool_has_exactly_three_free_surfaces() {
        let pool = SurfacePoolState::new(7);
        let snapshot = pool.snapshot();
        assert_eq!(snapshot.len(), 3);
        assert!(snapshot.iter().all(|slot| {
            slot.phase == SurfacePhase::Free && slot.generation == 7 && slot.lease_id == 0
        }));
    }

    #[test]
    fn pool_exhaustion_is_bounded_and_structured() {
        let mut pool = SurfacePoolState::new(1);
        let leases = [
            pool.acquire_for_gpu(1).unwrap(),
            pool.acquire_for_gpu(1).unwrap(),
            pool.acquire_for_gpu(1).unwrap(),
        ];
        assert_eq!(leases.map(|lease| lease.slot), [0, 1, 2]);
        let error = pool.acquire_for_gpu(1).unwrap_err();
        assert_eq!(error.code, GpuFallbackCode::PoolExhausted);
    }

    #[test]
    fn valid_lifecycle_is_free_gpu_encoder_free() {
        let mut pool = SurfacePoolState::new(1);
        let lease = pool.acquire_for_gpu(1).unwrap();
        assert_eq!(pool.snapshot()[lease.slot].phase, SurfacePhase::InGpu);
        pool.hand_off_to_encoder(lease).unwrap();
        assert_eq!(pool.snapshot()[lease.slot].phase, SurfacePhase::InEncoder);
        pool.release_encoder(lease).unwrap();
        assert_eq!(pool.snapshot()[lease.slot].phase, SurfacePhase::Free);
    }

    #[test]
    fn gpu_failure_can_abort_without_encoder_handoff() {
        let mut pool = SurfacePoolState::new(5);
        let lease = pool.acquire_for_gpu(5).unwrap();
        pool.abort_gpu(lease).unwrap();
        assert_eq!(pool.snapshot()[lease.slot].phase, SurfacePhase::Free);
    }

    #[test]
    fn encoder_cannot_release_surface_still_in_gpu() {
        let mut pool = SurfacePoolState::new(2);
        let lease = pool.acquire_for_gpu(2).unwrap();
        let error = pool.release_encoder(lease).unwrap_err();
        assert_eq!(error.code, GpuFallbackCode::InvalidSurfaceTransition);
        assert_eq!(pool.snapshot()[lease.slot].phase, SurfacePhase::InGpu);
    }

    #[test]
    fn double_handoff_and_double_release_are_rejected() {
        let mut pool = SurfacePoolState::new(2);
        let lease = pool.acquire_for_gpu(2).unwrap();
        pool.hand_off_to_encoder(lease).unwrap();
        assert_eq!(
            pool.hand_off_to_encoder(lease).unwrap_err().code,
            GpuFallbackCode::InvalidSurfaceTransition
        );
        pool.release_encoder(lease).unwrap();
        assert_eq!(
            pool.release_encoder(lease).unwrap_err().code,
            GpuFallbackCode::InvalidSurfaceTransition
        );
    }

    #[test]
    fn stale_lease_id_cannot_release_reused_slot_in_same_generation() {
        let mut pool = SurfacePoolState::new(9);
        let old = pool.acquire_for_gpu(9).unwrap();
        pool.hand_off_to_encoder(old).unwrap();
        pool.release_encoder(old).unwrap();
        let current = pool.acquire_for_gpu(9).unwrap();
        assert_eq!(old.slot, current.slot);
        assert_ne!(old.lease_id, current.lease_id);
        assert_eq!(
            pool.release_encoder(old).unwrap_err().code,
            GpuFallbackCode::StaleGeneration
        );
        assert_eq!(pool.snapshot()[current.slot].phase, SurfacePhase::InGpu);
    }

    #[test]
    fn generation_switch_waits_for_encoder_ownership_to_end() {
        let mut pool = SurfacePoolState::new(3);
        let lease = pool.acquire_for_gpu(3).unwrap();
        pool.hand_off_to_encoder(lease).unwrap();
        assert_eq!(
            pool.begin_generation(4).unwrap_err().code,
            GpuFallbackCode::PoolBusy
        );
        pool.release_encoder(lease).unwrap();
        pool.begin_generation(4).unwrap();
        assert!(pool
            .snapshot()
            .iter()
            .all(|slot| slot.generation == 4 && slot.phase == SurfacePhase::Free));
    }

    #[test]
    fn old_generation_cannot_acquire_after_switch() {
        let mut pool = SurfacePoolState::new(10);
        pool.begin_generation(11).unwrap();
        let error = pool.acquire_for_gpu(10).unwrap_err();
        assert_eq!(error.code, GpuFallbackCode::StaleGeneration);
        assert!(pool
            .snapshot()
            .iter()
            .all(|slot| slot.phase == SurfacePhase::Free));
    }

    #[test]
    fn old_generation_token_cannot_mutate_new_generation() {
        let mut pool = SurfacePoolState::new(12);
        let old = pool.acquire_for_gpu(12).unwrap();
        pool.abort_gpu(old).unwrap();
        pool.begin_generation(13).unwrap();
        let current = pool.acquire_for_gpu(13).unwrap();
        assert_eq!(
            pool.hand_off_to_encoder(old).unwrap_err().code,
            GpuFallbackCode::StaleGeneration
        );
        assert_eq!(pool.snapshot()[current.slot].phase, SurfacePhase::InGpu);
    }
}
