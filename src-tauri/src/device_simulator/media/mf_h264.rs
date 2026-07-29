use std::time::Duration;

#[cfg(target_os = "windows")]
fn win<T>(result: windows::core::Result<T>, context: &'static str) -> Result<T, String> {
    result.map_err(|error| format!("{context}: {error}"))
}

#[derive(Debug, Clone)]
pub struct EncodedH264AccessUnit {
    pub nals: Vec<Vec<u8>>,
    pub keyframe: bool,
    pub sample_time_100ns: i64,
}

#[derive(Debug, Clone)]
pub struct H264TranscoderDescriptor {
    pub width: u32,
    pub height: u32,
    pub encoder_name: String,
    pub hardware: bool,
}

#[cfg(target_os = "windows")]
pub struct H264WatermarkTranscoder {
    decoder: MfH264Decoder,
    encoder: MfH264Encoder,
    descriptor: H264TranscoderDescriptor,
    // Must be dropped after every COM/MFT field. Rust drops fields in
    // declaration order; shutting Media Foundation down first can make the
    // subsequent IMFTransform releases access invalid native state.
    _runtime: MediaFoundationRuntime,
}

#[cfg(target_os = "windows")]
impl H264WatermarkTranscoder {
    pub fn new(
        width: u32,
        height: u32,
        frame_rate_numerator: u32,
        frame_rate_denominator: u32,
        bitrate_bps: u32,
    ) -> Result<Self, String> {
        let runtime = MediaFoundationRuntime::startup()?;
        let decoder =
            MfH264Decoder::new(width, height, frame_rate_numerator, frame_rate_denominator)?;
        let encoder = MfH264Encoder::new(
            width,
            height,
            frame_rate_numerator,
            frame_rate_denominator,
            bitrate_bps,
        )?;
        let descriptor = H264TranscoderDescriptor {
            width,
            height,
            encoder_name: encoder.name.clone(),
            hardware: encoder.hardware,
        };
        Ok(Self {
            decoder,
            encoder,
            descriptor,
            _runtime: runtime,
        })
    }

    pub fn descriptor(&self) -> &H264TranscoderDescriptor {
        &self.descriptor
    }

    pub fn transcode(
        &mut self,
        annex_b_access_unit: &[u8],
        sample_time_100ns: i64,
        render: impl Fn(&mut [u8]) -> Result<(), String>,
    ) -> Result<Vec<EncodedH264AccessUnit>, String> {
        let decoded = self
            .decoder
            .decode(annex_b_access_unit, sample_time_100ns)?;
        let mut encoded = Vec::new();
        for mut frame in decoded {
            render(&mut frame.nv12)?;
            for output in self.encoder.encode(&frame.nv12, frame.sample_time_100ns)? {
                let nals = split_h264_access_unit(&output.bytes)?;
                if nals.is_empty() {
                    continue;
                }
                let keyframe = nals
                    .iter()
                    .any(|nal| nal.first().is_some_and(|header| header & 0x1f == 5));
                encoded.push(EncodedH264AccessUnit {
                    nals,
                    keyframe,
                    sample_time_100ns: output.sample_time_100ns,
                });
            }
        }
        Ok(encoded)
    }

    pub fn request_keyframe(&self) -> Result<(), String> {
        self.encoder.request_keyframe()
    }
}

#[cfg(not(target_os = "windows"))]
pub struct H264WatermarkTranscoder;

#[cfg(not(target_os = "windows"))]
impl H264WatermarkTranscoder {
    pub fn new(
        _width: u32,
        _height: u32,
        _frame_rate_numerator: u32,
        _frame_rate_denominator: u32,
        _bitrate_bps: u32,
    ) -> Result<Self, String> {
        Err("device simulator time watermark requires Windows Media Foundation".into())
    }
}

pub fn h264_sps_dimensions(sps: &[u8]) -> Result<(u32, u32), String> {
    if sps.len() < 4 || sps[0] & 0x1f != 7 {
        return Err("invalid H.264 SPS".into());
    }
    let mut rbsp = Vec::with_capacity(sps.len());
    let mut zero_count = 0u8;
    for byte in sps.iter().copied().skip(1) {
        if zero_count >= 2 && byte == 0x03 {
            zero_count = 0;
            continue;
        }
        rbsp.push(byte);
        zero_count = if byte == 0 {
            zero_count.saturating_add(1)
        } else {
            0
        };
    }
    let mut bits = BitReader::new(&rbsp);
    let profile_idc = bits.read_bits(8)? as u8;
    bits.read_bits(8)?;
    bits.read_bits(8)?;
    bits.read_ue()?;

    let mut chroma_format_idc = 1u32;
    let mut separate_colour_plane_flag = false;
    if matches!(
        profile_idc,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        chroma_format_idc = bits.read_ue()?;
        if chroma_format_idc == 3 {
            separate_colour_plane_flag = bits.read_bit()? != 0;
        }
        bits.read_ue()?;
        bits.read_ue()?;
        bits.read_bit()?;
        if bits.read_bit()? != 0 {
            let scaling_lists = if chroma_format_idc != 3 { 8 } else { 12 };
            for index in 0..scaling_lists {
                if bits.read_bit()? != 0 {
                    skip_scaling_list(&mut bits, if index < 6 { 16 } else { 64 })?;
                }
            }
        }
    }

    bits.read_ue()?;
    let pic_order_cnt_type = bits.read_ue()?;
    if pic_order_cnt_type == 0 {
        bits.read_ue()?;
    } else if pic_order_cnt_type == 1 {
        bits.read_bit()?;
        bits.read_se()?;
        bits.read_se()?;
        for _ in 0..bits.read_ue()? {
            bits.read_se()?;
        }
    }
    bits.read_ue()?;
    bits.read_bit()?;
    let pic_width_in_mbs_minus1 = bits.read_ue()?;
    let pic_height_in_map_units_minus1 = bits.read_ue()?;
    let frame_mbs_only_flag = bits.read_bit()? as u32;
    if frame_mbs_only_flag == 0 {
        bits.read_bit()?;
    }
    bits.read_bit()?;
    let frame_cropping_flag = bits.read_bit()? != 0;
    let (crop_left, crop_right, crop_top, crop_bottom) = if frame_cropping_flag {
        (
            bits.read_ue()?,
            bits.read_ue()?,
            bits.read_ue()?,
            bits.read_ue()?,
        )
    } else {
        (0, 0, 0, 0)
    };

    let chroma_array_type = if separate_colour_plane_flag {
        0
    } else {
        chroma_format_idc
    };
    let (sub_width_c, sub_height_c) = match chroma_array_type {
        0 => (1, 1),
        1 => (2, 2),
        2 => (2, 1),
        3 => (1, 1),
        _ => return Err("unsupported H.264 chroma format".into()),
    };
    let crop_unit_x = if chroma_array_type == 0 {
        1
    } else {
        sub_width_c
    };
    let crop_unit_y = if chroma_array_type == 0 {
        2 - frame_mbs_only_flag
    } else {
        sub_height_c * (2 - frame_mbs_only_flag)
    };
    let coded_width = (pic_width_in_mbs_minus1 + 1)
        .checked_mul(16)
        .ok_or_else(|| "H.264 SPS width overflow".to_string())?;
    let coded_height = (2 - frame_mbs_only_flag)
        .checked_mul(pic_height_in_map_units_minus1 + 1)
        .and_then(|value| value.checked_mul(16))
        .ok_or_else(|| "H.264 SPS height overflow".to_string())?;
    let crop_width = (crop_left + crop_right)
        .checked_mul(crop_unit_x)
        .ok_or_else(|| "H.264 SPS crop width overflow".to_string())?;
    let crop_height = (crop_top + crop_bottom)
        .checked_mul(crop_unit_y)
        .ok_or_else(|| "H.264 SPS crop height overflow".to_string())?;
    let width = coded_width
        .checked_sub(crop_width)
        .ok_or_else(|| "H.264 SPS crop exceeds width".to_string())?;
    let height = coded_height
        .checked_sub(crop_height)
        .ok_or_else(|| "H.264 SPS crop exceeds height".to_string())?;
    if width < 64 || height < 32 || width % 2 != 0 || height % 2 != 0 {
        return Err(format!("unsupported H.264 SPS dimensions {width}x{height}"));
    }
    Ok((width, height))
}

fn skip_scaling_list(bits: &mut BitReader<'_>, size: usize) -> Result<(), String> {
    let mut last_scale = 8i32;
    let mut next_scale = 8i32;
    for _ in 0..size {
        if next_scale != 0 {
            next_scale = (last_scale + bits.read_se()? + 256) % 256;
        }
        if next_scale != 0 {
            last_scale = next_scale;
        }
    }
    Ok(())
}

struct BitReader<'a> {
    bytes: &'a [u8],
    bit_offset: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            bit_offset: 0,
        }
    }

    fn read_bit(&mut self) -> Result<u8, String> {
        let byte = *self
            .bytes
            .get(self.bit_offset / 8)
            .ok_or_else(|| "truncated H.264 SPS".to_string())?;
        let value = (byte >> (7 - self.bit_offset % 8)) & 1;
        self.bit_offset += 1;
        Ok(value)
    }

    fn read_bits(&mut self, count: usize) -> Result<u32, String> {
        if count > 32 {
            return Err("H.264 bit field is too wide".into());
        }
        let mut value = 0u32;
        for _ in 0..count {
            value = (value << 1) | u32::from(self.read_bit()?);
        }
        Ok(value)
    }

    fn read_ue(&mut self) -> Result<u32, String> {
        let mut leading_zero_bits = 0usize;
        while self.read_bit()? == 0 {
            leading_zero_bits += 1;
            if leading_zero_bits > 31 {
                return Err("invalid H.264 Exp-Golomb value".into());
            }
        }
        let suffix = self.read_bits(leading_zero_bits)?;
        Ok(((1u32 << leading_zero_bits) - 1) + suffix)
    }

    fn read_se(&mut self) -> Result<i32, String> {
        let code_num = self.read_ue()?;
        let magnitude = ((code_num + 1) / 2) as i32;
        Ok(if code_num % 2 == 0 {
            -magnitude
        } else {
            magnitude
        })
    }
}

fn split_h264_access_unit(bytes: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let starts = annex_b_start_codes(bytes);
    if !starts.is_empty() {
        let mut units = Vec::new();
        for (index, (offset, prefix)) in starts.iter().copied().enumerate() {
            let start = offset + prefix;
            let end = starts
                .get(index + 1)
                .map(|(next, _)| *next)
                .unwrap_or(bytes.len());
            if start < end {
                units.push(bytes[start..end].to_vec());
            }
        }
        return Ok(units);
    }

    // A few MFTs return AVCC length-prefixed samples despite advertising H.264.
    let mut units = Vec::new();
    let mut offset = 0usize;
    while offset + 4 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let end = offset
            .checked_add(length)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| "invalid length-prefixed H.264 access unit".to_string())?;
        if length != 0 {
            units.push(bytes[offset..end].to_vec());
        }
        offset = end;
    }
    if units.is_empty() || offset != bytes.len() {
        return Err("H.264 encoder returned neither Annex-B nor AVCC data".into());
    }
    Ok(units)
}

fn annex_b_start_codes(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let mut index = 0usize;
    while index + 3 <= bytes.len() {
        if bytes[index..].starts_with(&[0, 0, 1]) {
            result.push((index, 3));
            index += 3;
        } else if bytes[index..].starts_with(&[0, 0, 0, 1]) {
            result.push((index, 4));
            index += 4;
        } else {
            index += 1;
        }
    }
    result
}

#[cfg(target_os = "windows")]
struct MediaFoundationRuntime {
    com_initialized: bool,
}

#[cfg(target_os = "windows")]
impl MediaFoundationRuntime {
    fn startup() -> Result<Self, String> {
        use windows::core::HRESULT;
        use windows::Win32::Media::MediaFoundation::{MFStartup, MFSTARTUP_FULL, MF_VERSION};
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

        const RPC_E_CHANGED_MODE: HRESULT = HRESULT(0x80010106u32 as i32);
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let com_initialized = if com.is_ok() {
            true
        } else if com == RPC_E_CHANGED_MODE {
            false
        } else {
            return Err(format!("time watermark COM initialization failed: {com:?}"));
        };
        if let Err(error) = unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) } {
            if com_initialized {
                unsafe { windows::Win32::System::Com::CoUninitialize() };
            }
            return Err(format!(
                "time watermark Media Foundation startup failed: {error}"
            ));
        }
        Ok(Self { com_initialized })
    }
}

#[cfg(target_os = "windows")]
impl Drop for MediaFoundationRuntime {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Media::MediaFoundation::MFShutdown();
            if self.com_initialized {
                windows::Win32::System::Com::CoUninitialize();
            }
        }
    }
}

#[cfg(target_os = "windows")]
struct DecodedNv12Frame {
    nv12: Vec<u8>,
    sample_time_100ns: i64,
}

#[cfg(target_os = "windows")]
struct MfH264Decoder {
    transform: windows::Win32::Media::MediaFoundation::IMFTransform,
    width: u32,
    height: u32,
    stride: usize,
    output_size: u32,
    output_provides_samples: bool,
    frame_duration_100ns: i64,
}

#[cfg(target_os = "windows")]
impl MfH264Decoder {
    fn new(
        width: u32,
        height: u32,
        frame_rate_numerator: u32,
        frame_rate_denominator: u32,
    ) -> Result<Self, String> {
        use windows::Win32::Media::MediaFoundation::*;
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

        let transform: IMFTransform =
            unsafe { CoCreateInstance(&CLSID_MSH264DecoderMFT, None, CLSCTX_INPROC_SERVER) }
                .map_err(|error| format!("H.264 decoder activation failed: {error}"))?;
        if let Ok(attributes) = unsafe { transform.GetAttributes() } {
            let _ = unsafe { attributes.SetUINT32(&MF_LOW_LATENCY, 1) };
        }
        let frame_size = (u64::from(width) << 32) | u64::from(height);
        let frame_rate = (u64::from(frame_rate_numerator.max(1)) << 32)
            | u64::from(frame_rate_denominator.max(1));
        let input = unsafe { MFCreateMediaType() }
            .map_err(|error| format!("H.264 decoder input media type failed: {error}"))?;
        unsafe {
            win(
                input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video),
                "H.264 decoder major type setup failed",
            )?;
            win(
                input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264),
                "H.264 decoder subtype setup failed",
            )?;
            win(
                input.SetUINT64(&MF_MT_FRAME_SIZE, frame_size),
                "H.264 decoder frame size setup failed",
            )?;
            win(
                input.SetUINT64(&MF_MT_FRAME_RATE, frame_rate),
                "H.264 decoder frame rate setup failed",
            )?;
            win(
                input.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32),
                "H.264 decoder interlace mode setup failed",
            )?;
            win(
                transform.SetInputType(0, &input, 0),
                "H.264 decoder input type setup failed",
            )?;
        }
        let (stride, output_size, output_provides_samples) =
            select_decoder_nv12_output(&transform, width, height)?;
        unsafe {
            win(
                transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0),
                "H.264 decoder begin streaming failed",
            )?;
            win(
                transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0),
                "H.264 decoder start of stream failed",
            )?;
        }
        let frame_duration_100ns = (10_000_000u64
            .saturating_mul(u64::from(frame_rate_denominator.max(1)))
            / u64::from(frame_rate_numerator.max(1)))
        .max(1) as i64;
        Ok(Self {
            transform,
            width,
            height,
            stride,
            output_size,
            output_provides_samples,
            frame_duration_100ns,
        })
    }

    fn decode(
        &mut self,
        bytes: &[u8],
        sample_time_100ns: i64,
    ) -> Result<Vec<DecodedNv12Frame>, String> {
        use windows::Win32::Media::MediaFoundation::*;
        let buffer = media_buffer_from_bytes(bytes, "H.264 decoder input")?;
        let sample = unsafe { MFCreateSample() }
            .map_err(|error| format!("H.264 decoder input sample failed: {error}"))?;
        unsafe {
            win(
                sample.AddBuffer(&buffer),
                "H.264 decoder input buffer attach failed",
            )?;
            win(
                sample.SetSampleTime(sample_time_100ns),
                "H.264 decoder sample time setup failed",
            )?;
            win(
                sample.SetSampleDuration(self.frame_duration_100ns),
                "H.264 decoder sample duration setup failed",
            )?;
        }
        match unsafe { self.transform.ProcessInput(0, &sample, 0) } {
            Ok(()) => {}
            Err(error) if error.code() == MF_E_NOTACCEPTING => {
                let mut drained = self.drain_outputs(sample_time_100ns)?;
                unsafe { self.transform.ProcessInput(0, &sample, 0) }.map_err(|retry| {
                    format!("H.264 decoder rejected input after drain: {retry}")
                })?;
                drained.extend(self.drain_outputs(sample_time_100ns)?);
                return Ok(drained);
            }
            Err(error) => return Err(format!("H.264 decoder ProcessInput failed: {error}")),
        }
        self.drain_outputs(sample_time_100ns)
    }

    fn drain_outputs(&mut self, fallback_time: i64) -> Result<Vec<DecodedNv12Frame>, String> {
        use std::mem::ManuallyDrop;
        use windows::Win32::Media::MediaFoundation::*;

        let mut frames = Vec::new();
        for _ in 0..16 {
            let requested = if self.output_provides_samples {
                None
            } else {
                let buffer = unsafe { MFCreateMemoryBuffer(self.output_size) }
                    .map_err(|error| format!("H.264 decoder output buffer failed: {error}"))?;
                let sample = unsafe { MFCreateSample() }
                    .map_err(|error| format!("H.264 decoder output sample failed: {error}"))?;
                unsafe { sample.AddBuffer(&buffer) }
                    .map_err(|error| format!("H.264 decoder output attach failed: {error}"))?;
                Some(sample)
            };
            let mut output = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(requested),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            };
            let mut status = 0;
            let result = unsafe {
                self.transform
                    .ProcessOutput(0, std::slice::from_mut(&mut output), &mut status)
            };
            let produced = unsafe { ManuallyDrop::take(&mut output.pSample) };
            drop(unsafe { ManuallyDrop::take(&mut output.pEvents) });
            match result {
                Ok(()) => {
                    let sample = produced
                        .ok_or_else(|| "H.264 decoder returned no output sample".to_string())?;
                    let sample_time = unsafe { sample.GetSampleTime() }.unwrap_or(fallback_time);
                    let buffer =
                        unsafe { sample.ConvertToContiguousBuffer() }.map_err(|error| {
                            format!("H.264 decoder output coalesce failed: {error}")
                        })?;
                    let bytes = copy_media_buffer(&buffer, "H.264 decoder output")?;
                    let nv12 = normalize_nv12(
                        &bytes,
                        self.width as usize,
                        self.height as usize,
                        self.stride,
                    )?;
                    frames.push(DecodedNv12Frame {
                        nv12,
                        sample_time_100ns: sample_time,
                    });
                }
                Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(frames),
                Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    let (stride, size, provides) =
                        select_decoder_nv12_output(&self.transform, self.width, self.height)?;
                    self.stride = stride;
                    self.output_size = size;
                    self.output_provides_samples = provides;
                }
                Err(error) => return Err(format!("H.264 decoder ProcessOutput failed: {error}")),
            }
        }
        Err("H.264 decoder exceeded the bounded output drain".into())
    }
}

#[cfg(target_os = "windows")]
impl Drop for MfH264Decoder {
    fn drop(&mut self) {
        use windows::Win32::Media::MediaFoundation::{
            MFT_MESSAGE_NOTIFY_END_OF_STREAM, MFT_MESSAGE_NOTIFY_END_STREAMING,
        };
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
        }
    }
}

#[cfg(target_os = "windows")]
fn select_decoder_nv12_output(
    transform: &windows::Win32::Media::MediaFoundation::IMFTransform,
    width: u32,
    height: u32,
) -> Result<(usize, u32, bool), String> {
    use windows::Win32::Media::MediaFoundation::*;
    for index in 0..64u32 {
        let output = match unsafe { transform.GetOutputAvailableType(0, index) } {
            Ok(output) => output,
            Err(error) if error.code() == MF_E_NO_MORE_TYPES => break,
            Err(error) => return Err(format!("H.264 decoder output enumeration failed: {error}")),
        };
        let subtype = unsafe { output.GetGUID(&MF_MT_SUBTYPE) }
            .map_err(|error| format!("H.264 decoder output subtype failed: {error}"))?;
        if subtype != MFVideoFormat_NV12 {
            continue;
        }
        unsafe { transform.SetOutputType(0, &output, 0) }
            .map_err(|error| format!("H.264 decoder rejected NV12 output: {error}"))?;
        let stride = unsafe { output.GetUINT32(&MF_MT_DEFAULT_STRIDE) }
            .ok()
            .map(|value| (value as i32).unsigned_abs() as usize)
            .filter(|value| *value >= width as usize)
            .unwrap_or(width as usize);
        let info = unsafe { transform.GetOutputStreamInfo(0) }
            .map_err(|error| format!("H.264 decoder output stream info failed: {error}"))?;
        let fallback = stride
            .saturating_mul(height as usize)
            .saturating_mul(3)
            .saturating_div(2)
            .min(u32::MAX as usize) as u32;
        let provides = info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
        return Ok((stride, info.cbSize.max(fallback).max(1), provides));
    }
    Err("H.264 decoder exposes no NV12 output".into())
}

#[cfg(target_os = "windows")]
struct EncodedOutput {
    bytes: Vec<u8>,
    sample_time_100ns: i64,
}

/// `ProcessOutput` 的两类结果：真正的失败，以及"MFT 要求重新协商输出类型"这个
/// 正常协议事件。分开是为了让后者能在协商后重试，而不是把编码器整体否掉。
#[cfg(target_os = "windows")]
enum EncoderOutputError {
    StreamChange,
    Failed(String),
}

#[cfg(target_os = "windows")]
impl From<String> for EncoderOutputError {
    fn from(value: String) -> Self {
        Self::Failed(value)
    }
}

#[cfg(target_os = "windows")]
enum EncoderOutputOutcome {
    Produced(EncodedOutput),
    Renegotiated,
    NeedMoreInput,
}

#[cfg(target_os = "windows")]
struct MfH264Encoder {
    transform: windows::Win32::Media::MediaFoundation::IMFTransform,
    codec_api: Option<windows::Win32::Media::MediaFoundation::ICodecAPI>,
    async_adapter: Option<AsyncMftAdapter>,
    width: u32,
    height: u32,
    output_size: u32,
    output_provides_samples: bool,
    frame_duration_100ns: i64,
    name: String,
    hardware: bool,
}

#[cfg(target_os = "windows")]
impl MfH264Encoder {
    fn new(
        width: u32,
        height: u32,
        frame_rate_numerator: u32,
        frame_rate_denominator: u32,
        bitrate_bps: u32,
    ) -> Result<Self, String> {
        use windows::Win32::Media::MediaFoundation::IMFTransform;
        use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

        let mut failures = Vec::new();
        match enumerate_hardware_encoders() {
            Ok(candidates) => {
                for candidate in candidates {
                    let transform = match unsafe {
                        candidate.activation.ActivateObject::<IMFTransform>()
                    } {
                        Ok(transform) => transform,
                        Err(error) => {
                            failures.push(format!("{} activation failed: {error}", candidate.name));
                            continue;
                        }
                    };
                    match Self::from_transform(
                        transform,
                        candidate.name.clone(),
                        true,
                        width,
                        height,
                        frame_rate_numerator,
                        frame_rate_denominator,
                        bitrate_bps,
                    ) {
                        Ok(encoder) => return Ok(encoder),
                        Err(error) => failures.push(format!("{}: {error}", candidate.name)),
                    }
                }
            }
            Err(error) => failures.push(error),
        }

        let transform: IMFTransform = unsafe {
            CoCreateInstance(
                &windows::Win32::Media::MediaFoundation::CLSID_MSH264EncoderMFT,
                None,
                CLSCTX_INPROC_SERVER,
            )
        }
        .map_err(|error| {
            format!(
                "no usable H.264 encoder; hardware failures: {}; software activation failed: {error}",
                failures.join("; ")
            )
        })?;
        Self::from_transform(
            transform,
            "Microsoft H.264 Video Encoder MFT".into(),
            false,
            width,
            height,
            frame_rate_numerator,
            frame_rate_denominator,
            bitrate_bps,
        )
        .map_err(|error| {
            format!(
                "no usable H.264 encoder; hardware failures: {}; software encoder: {error}",
                failures.join("; ")
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn from_transform(
        transform: windows::Win32::Media::MediaFoundation::IMFTransform,
        name: String,
        hardware: bool,
        width: u32,
        height: u32,
        frame_rate_numerator: u32,
        frame_rate_denominator: u32,
        bitrate_bps: u32,
    ) -> Result<Self, String> {
        use windows::core::{Interface, VARIANT};
        use windows::Win32::Media::MediaFoundation::*;

        if let Ok(attributes) = unsafe { transform.GetAttributes() } {
            let _ = unsafe { attributes.SetUINT32(&MF_LOW_LATENCY, 1) };
        }
        let async_adapter = AsyncMftAdapter::for_transform(&transform)?;
        let frame_size = (u64::from(width) << 32) | u64::from(height);
        let frame_rate = (u64::from(frame_rate_numerator.max(1)) << 32)
            | u64::from(frame_rate_denominator.max(1));
        let output = unsafe { MFCreateMediaType() }
            .map_err(|error| format!("H.264 encoder output type failed: {error}"))?;
        unsafe {
            win(
                output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video),
                "H.264 encoder output major type setup failed",
            )?;
            win(
                output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264),
                "H.264 encoder output subtype setup failed",
            )?;
            win(
                output.SetUINT64(&MF_MT_FRAME_SIZE, frame_size),
                "H.264 encoder output frame size setup failed",
            )?;
            win(
                output.SetUINT64(&MF_MT_FRAME_RATE, frame_rate),
                "H.264 encoder output frame rate setup failed",
            )?;
            win(
                output.SetUINT32(&MF_MT_AVG_BITRATE, bitrate_bps),
                "H.264 encoder bitrate setup failed",
            )?;
            win(
                output.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32),
                "H.264 encoder interlace mode setup failed",
            )?;
            win(
                output.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base.0 as u32),
                "H.264 encoder profile setup failed",
            )?;
            win(
                output.SetUINT32(
                    &MF_MT_MAX_KEYFRAME_SPACING,
                    frame_rate_numerator.max(1).saturating_mul(2),
                ),
                "H.264 encoder keyframe spacing setup failed",
            )?;
            win(
                output.SetUINT32(&MF_MT_REALTIME_CONTENT, 1),
                "H.264 encoder realtime mode setup failed",
            )?;
            win(
                transform.SetOutputType(0, &output, 0),
                "H.264 encoder output type setup failed",
            )?;
        }
        let input = unsafe { MFCreateMediaType() }
            .map_err(|error| format!("H.264 encoder input type failed: {error}"))?;
        unsafe {
            win(
                input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video),
                "H.264 encoder input major type setup failed",
            )?;
            win(
                input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12),
                "H.264 encoder input subtype setup failed",
            )?;
            win(
                input.SetUINT64(&MF_MT_FRAME_SIZE, frame_size),
                "H.264 encoder input frame size setup failed",
            )?;
            win(
                input.SetUINT64(&MF_MT_FRAME_RATE, frame_rate),
                "H.264 encoder input frame rate setup failed",
            )?;
            win(
                input.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32),
                "H.264 encoder input interlace mode setup failed",
            )?;
            win(
                input.SetUINT32(&MF_MT_FIXED_SIZE_SAMPLES, 1),
                "H.264 encoder fixed sample setup failed",
            )?;
            win(
                input.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1),
                "H.264 encoder independent sample setup failed",
            )?;
            win(
                transform.SetInputType(0, &input, 0),
                "H.264 encoder input type setup failed",
            )?;
        }
        let codec_api = transform.cast::<ICodecAPI>().ok();
        if let Some(api) = codec_api.as_ref() {
            for (property, value) in [
                (&CODECAPI_AVEncMPVDefaultBPictureCount, 0u32),
                (&CODECAPI_AVEncCommonMeanBitRate, bitrate_bps),
                (
                    &CODECAPI_AVEncMPVGOPSize,
                    frame_rate_numerator.max(1).saturating_mul(2),
                ),
            ] {
                if unsafe { api.IsSupported(property) }.is_ok() {
                    let _ = unsafe { api.SetValue(property, &VARIANT::from(value)) };
                }
            }
            if unsafe { api.IsSupported(&CODECAPI_AVLowLatencyMode) }.is_ok() {
                let _ = unsafe { api.SetValue(&CODECAPI_AVLowLatencyMode, &VARIANT::from(true)) };
            }
        }
        let info = unsafe { transform.GetOutputStreamInfo(0) }
            .map_err(|error| format!("H.264 encoder output stream info failed: {error}"))?;
        let output_provides_samples =
            info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
        unsafe {
            win(
                transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0),
                "H.264 encoder begin streaming failed",
            )?;
            win(
                transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0),
                "H.264 encoder start of stream failed",
            )?;
        }
        let frame_duration_100ns = (10_000_000u64
            .saturating_mul(u64::from(frame_rate_denominator.max(1)))
            / u64::from(frame_rate_numerator.max(1)))
        .max(1) as i64;
        Ok(Self {
            transform,
            codec_api,
            async_adapter,
            width,
            height,
            output_size: info.cbSize.max(width.saturating_mul(height)).max(1),
            output_provides_samples,
            frame_duration_100ns,
            name,
            hardware,
        })
    }

    fn encode(&mut self, nv12: &[u8], sample_time: i64) -> Result<Vec<EncodedOutput>, String> {
        use windows::Win32::Media::MediaFoundation::*;
        let buffer = media_buffer_from_bytes(nv12, "H.264 encoder input")?;
        let sample = unsafe { MFCreateSample() }
            .map_err(|error| format!("H.264 encoder input sample failed: {error}"))?;
        unsafe {
            win(
                sample.AddBuffer(&buffer),
                "H.264 encoder input buffer attach failed",
            )?;
            win(
                sample.SetSampleTime(sample_time),
                "H.264 encoder sample time setup failed",
            )?;
            win(
                sample.SetSampleDuration(self.frame_duration_100ns),
                "H.264 encoder sample duration setup failed",
            )?;
        }
        if let Some(adapter) = self.async_adapter.as_mut() {
            adapter.wait_for_input_credit(Duration::from_millis(500))?;
        }
        unsafe { self.transform.ProcessInput(0, &sample, 0) }
            .map_err(|error| format!("H.264 encoder ProcessInput failed: {error}"))?;
        let mut outputs = Vec::new();
        if let Some(adapter) = self.async_adapter.as_mut() {
            adapter.wait_for_output_or_next_input(Duration::from_millis(500))?;
            while self
                .async_adapter
                .as_mut()
                .is_some_and(AsyncMftAdapter::take_output_credit)
            {
                match self.process_one_output(sample_time)? {
                    EncoderOutputOutcome::Produced(output) => outputs.push(output),
                    // 格式变更消耗了这次 HaveOutput 事件，等下一个事件再取输出。
                    EncoderOutputOutcome::Renegotiated => {}
                    EncoderOutputOutcome::NeedMoreInput => {}
                }
                if let Some(adapter) = self.async_adapter.as_mut() {
                    adapter.poll_available()?;
                }
            }
        } else {
            loop {
                match self.process_one_output(sample_time)? {
                    EncoderOutputOutcome::Produced(output) => outputs.push(output),
                    // 同步 MFT 在协商后已就地重试，这里不会出现；保留分支只为穷尽。
                    EncoderOutputOutcome::Renegotiated => {}
                    EncoderOutputOutcome::NeedMoreInput => break,
                }
            }
        }
        Ok(outputs)
    }

    /// `MF_E_TRANSFORM_STREAM_CHANGE` 是 Media Foundation 的正常协议事件，不是
    /// 失败：MFT 要求重新协商输出类型后才继续产出。Intel Quick Sync 等硬件编码器
    /// 常在首帧输入之后才最终确定输出类型，把它当硬错误会让整条水印管线起不来。
    ///
    /// 异步 MFT 的格式变更会消耗掉本次 `METransformHaveOutput`：协商完成后必须
    /// 等待下一个事件，立即重调 `ProcessOutput` 会得到 `E_UNEXPECTED`。同步 MFT
    /// 没有事件模型，协商后直接重试。
    fn process_one_output(&mut self, fallback_time: i64) -> Result<EncoderOutputOutcome, String> {
        match self.try_process_one_output(fallback_time) {
            Ok(Some(output)) => Ok(EncoderOutputOutcome::Produced(output)),
            Ok(None) => Ok(EncoderOutputOutcome::NeedMoreInput),
            Err(EncoderOutputError::Failed(message)) => Err(message),
            Err(EncoderOutputError::StreamChange) => {
                self.renegotiate_output_type()?;
                if self.async_adapter.is_some() {
                    return Ok(EncoderOutputOutcome::Renegotiated);
                }
                match self.try_process_one_output(fallback_time) {
                    Ok(Some(output)) => Ok(EncoderOutputOutcome::Produced(output)),
                    Ok(None) => Ok(EncoderOutputOutcome::NeedMoreInput),
                    Err(EncoderOutputError::Failed(message)) => Err(message),
                    Err(EncoderOutputError::StreamChange) => Err(
                        "H.264 encoder requested another output type change immediately after renegotiation"
                            .to_string(),
                    ),
                }
            }
        }
    }

    /// 按 MFT 协议重新协商输出类型。新类型可能改变输出缓冲大小，也可能改变由谁
    /// 分配样本，沿用旧值会让随后的 `ProcessOutput` 收到无效参数，因此必须重新读取
    /// `GetOutputStreamInfo`。帧尺寸也要复核：水印管线的 SDP 已按原尺寸发布，编码器
    /// 换分辨率会让下游 SPS 校验在更晚、更难定位的地方失败。
    fn renegotiate_output_type(&mut self) -> Result<(), String> {
        use windows::Win32::Media::MediaFoundation::{
            MFVideoFormat_H264, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MF_E_NO_MORE_TYPES,
            MF_MT_FRAME_SIZE, MF_MT_SUBTYPE,
        };

        let expected_frame_size = (u64::from(self.width) << 32) | u64::from(self.height);
        // 上限只是防御一个不断返回类型的异常 MFT，不是协议要求。
        for index in 0..32u32 {
            let candidate = match unsafe { self.transform.GetOutputAvailableType(0, index) } {
                Ok(candidate) => candidate,
                Err(error) if error.code() == MF_E_NO_MORE_TYPES => break,
                Err(error) => {
                    return Err(format!(
                        "H.264 encoder output enumeration after a stream change failed: {error}"
                    ))
                }
            };
            let is_h264 = unsafe { candidate.GetGUID(&MF_MT_SUBTYPE) }
                .is_ok_and(|subtype| subtype == MFVideoFormat_H264);
            if !is_h264 {
                continue;
            }
            // 未声明帧尺寸的类型交给 SetOutputType 判定，不在这里提前否掉。
            let frame_size_matches = match unsafe { candidate.GetUINT64(&MF_MT_FRAME_SIZE) } {
                Ok(size) => size == expected_frame_size,
                Err(_) => true,
            };
            if !frame_size_matches {
                continue;
            }
            win(
                unsafe { self.transform.SetOutputType(0, &candidate, 0) },
                "H.264 encoder rejected its own renegotiated output type",
            )?;
            let info = unsafe { self.transform.GetOutputStreamInfo(0) }.map_err(|error| {
                format!("H.264 encoder output stream info after renegotiation failed: {error}")
            })?;
            self.output_size = info
                .cbSize
                .max(self.width.saturating_mul(self.height))
                .max(1);
            self.output_provides_samples =
                info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;
            log::debug!(
                "H.264 encoder '{}' renegotiated its output type: {} byte buffer, provides_samples={}",
                self.name,
                self.output_size,
                self.output_provides_samples
            );
            return Ok(());
        }
        Err(format!(
            "H.264 encoder '{}' exposed no {}x{} H.264 output type after requesting a stream change",
            self.name, self.width, self.height
        ))
    }

    fn try_process_one_output(
        &self,
        fallback_time: i64,
    ) -> Result<Option<EncodedOutput>, EncoderOutputError> {
        use std::mem::ManuallyDrop;
        use windows::Win32::Media::MediaFoundation::*;
        let requested = if self.output_provides_samples {
            None
        } else {
            let buffer = unsafe { MFCreateMemoryBuffer(self.output_size) }
                .map_err(|error| format!("H.264 encoder output buffer failed: {error}"))?;
            let sample = unsafe { MFCreateSample() }
                .map_err(|error| format!("H.264 encoder output sample failed: {error}"))?;
            unsafe { sample.AddBuffer(&buffer) }
                .map_err(|error| format!("H.264 encoder output attach failed: {error}"))?;
            Some(sample)
        };
        let mut output = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(requested),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        };
        let mut status = 0;
        let result = unsafe {
            self.transform
                .ProcessOutput(0, std::slice::from_mut(&mut output), &mut status)
        };
        let produced = unsafe { ManuallyDrop::take(&mut output.pSample) };
        drop(unsafe { ManuallyDrop::take(&mut output.pEvents) });
        match result {
            Ok(()) => {
                let sample = produced
                    .ok_or_else(|| "H.264 encoder returned no output sample".to_string())?;
                let time = unsafe { sample.GetSampleTime() }.unwrap_or(fallback_time);
                let buffer = unsafe { sample.ConvertToContiguousBuffer() }
                    .map_err(|error| format!("H.264 encoder output coalesce failed: {error}"))?;
                let bytes = copy_media_buffer(&buffer, "H.264 encoder output")?;
                Ok((!bytes.is_empty()).then_some(EncodedOutput {
                    bytes,
                    sample_time_100ns: time,
                }))
            }
            Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => Ok(None),
            Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                Err(EncoderOutputError::StreamChange)
            }
            Err(error) => Err(EncoderOutputError::Failed(format!(
                "H.264 encoder ProcessOutput failed: {error}"
            ))),
        }
    }

    fn request_keyframe(&self) -> Result<(), String> {
        use windows::core::VARIANT;
        use windows::Win32::Media::MediaFoundation::CODECAPI_AVEncVideoForceKeyFrame;
        let Some(api) = self.codec_api.as_ref() else {
            return Ok(());
        };
        if unsafe { api.IsSupported(&CODECAPI_AVEncVideoForceKeyFrame) }.is_ok() {
            unsafe { api.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &VARIANT::from(1u32)) }
                .map_err(|error| format!("H.264 keyframe request failed: {error}"))?;
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
impl Drop for MfH264Encoder {
    fn drop(&mut self) {
        use windows::Win32::Media::MediaFoundation::{
            MFT_MESSAGE_NOTIFY_END_OF_STREAM, MFT_MESSAGE_NOTIFY_END_STREAMING,
        };
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
        }
    }
}

#[cfg(target_os = "windows")]
struct EncoderCandidate {
    activation: windows::Win32::Media::MediaFoundation::IMFActivate,
    name: String,
}

#[cfg(target_os = "windows")]
fn enumerate_hardware_encoders() -> Result<Vec<EncoderCandidate>, String> {
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::CoTaskMemFree;

    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let mut raw: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input),
            Some(&output),
            &mut raw,
            &mut count,
        )
    }
    .map_err(|error| format!("hardware H.264 encoder enumeration failed: {error}"))?;
    if raw.is_null() || count == 0 {
        if !raw.is_null() {
            unsafe { CoTaskMemFree(Some(raw.cast())) };
        }
        return Ok(Vec::new());
    }
    let slots = unsafe { std::slice::from_raw_parts_mut(raw, count as usize) };
    let mut candidates = Vec::with_capacity(count as usize);
    for (index, slot) in slots.iter_mut().enumerate() {
        let Some(activation) = slot.take() else {
            continue;
        };
        let name = media_foundation_string(&activation, &MFT_FRIENDLY_NAME_Attribute)
            .unwrap_or_else(|| format!("Hardware H.264 MFT #{}", index + 1));
        candidates.push(EncoderCandidate { activation, name });
    }
    unsafe { CoTaskMemFree(Some(raw.cast())) };
    Ok(candidates)
}

#[cfg(target_os = "windows")]
fn media_foundation_string(
    attributes: &windows::Win32::Media::MediaFoundation::IMFAttributes,
    key: &windows::core::GUID,
) -> Option<String> {
    let length = unsafe { attributes.GetStringLength(key) }.ok()? as usize;
    let mut buffer = vec![0u16; length + 1];
    unsafe { attributes.GetString(key, &mut buffer, None) }.ok()?;
    Some(String::from_utf16_lossy(&buffer[..length]))
}

const ASYNC_EVENT_CREDIT_LIMIT: u8 = 8;

#[cfg(target_os = "windows")]
#[derive(Default)]
struct AsyncMftState {
    input_credits: u8,
    output_credits: u8,
}

#[cfg(target_os = "windows")]
struct AsyncMftAdapter {
    generator: windows::Win32::Media::MediaFoundation::IMFMediaEventGenerator,
    state: AsyncMftState,
}

#[cfg(target_os = "windows")]
impl AsyncMftAdapter {
    fn for_transform(
        transform: &windows::Win32::Media::MediaFoundation::IMFTransform,
    ) -> Result<Option<Self>, String> {
        use windows::core::Interface;
        use windows::Win32::Media::MediaFoundation::{
            IMFMediaEventGenerator, MF_TRANSFORM_ASYNC, MF_TRANSFORM_ASYNC_UNLOCK,
        };
        let Ok(attributes) = (unsafe { transform.GetAttributes() }) else {
            return Ok(None);
        };
        if unsafe { attributes.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0) == 0 {
            return Ok(None);
        }
        unsafe { attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }
            .map_err(|error| format!("async H.264 MFT unlock failed: {error}"))?;
        let generator = transform
            .cast::<IMFMediaEventGenerator>()
            .map_err(|error| format!("async H.264 MFT event generator unavailable: {error}"))?;
        Ok(Some(Self {
            generator,
            state: AsyncMftState::default(),
        }))
    }

    fn poll_available(&mut self) -> Result<(), String> {
        use windows::Win32::Media::MediaFoundation::{
            METransformHaveOutput, METransformNeedInput, MF_EVENT_FLAG_NO_WAIT,
            MF_E_NO_EVENTS_AVAILABLE,
        };
        for _ in 0..32 {
            let event = match unsafe { self.generator.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
                Ok(event) => event,
                Err(error) if error.code() == MF_E_NO_EVENTS_AVAILABLE => return Ok(()),
                Err(error) => return Err(format!("async H.264 MFT GetEvent failed: {error}")),
            };
            let status = unsafe { event.GetStatus() }
                .map_err(|error| format!("async H.264 MFT event status failed: {error}"))?;
            status
                .ok()
                .map_err(|error| format!("async H.264 MFT event reported failure: {error}"))?;
            let kind = unsafe { event.GetType() }
                .map_err(|error| format!("async H.264 MFT event type failed: {error}"))?;
            if kind == METransformNeedInput.0 as u32 {
                self.state.input_credits = self
                    .state
                    .input_credits
                    .checked_add(1)
                    .filter(|value| *value <= ASYNC_EVENT_CREDIT_LIMIT)
                    .ok_or_else(|| "async H.264 MFT exceeded NeedInput credit limit".to_string())?;
            } else if kind == METransformHaveOutput.0 as u32 {
                self.state.output_credits = self
                    .state
                    .output_credits
                    .checked_add(1)
                    .filter(|value| *value <= ASYNC_EVENT_CREDIT_LIMIT)
                    .ok_or_else(|| {
                        "async H.264 MFT exceeded HaveOutput credit limit".to_string()
                    })?;
            }
        }
        Err("async H.264 MFT produced too many events".into())
    }

    fn wait_for_input_credit(&mut self, timeout: Duration) -> Result<(), String> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if self.state.input_credits != 0 {
                self.state.input_credits -= 1;
                return Ok(());
            }
            self.poll_available()?;
            if std::time::Instant::now() >= deadline {
                return Err("async H.264 MFT timed out waiting for input".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn wait_for_output_or_next_input(&mut self, timeout: Duration) -> Result<(), String> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            self.poll_available()?;
            if self.state.output_credits != 0 || self.state.input_credits != 0 {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err("async H.264 MFT timed out waiting for output".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn take_output_credit(&mut self) -> bool {
        if self.state.output_credits == 0 {
            return false;
        }
        self.state.output_credits -= 1;
        true
    }
}

#[cfg(target_os = "windows")]
fn media_buffer_from_bytes(
    bytes: &[u8],
    label: &str,
) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaBuffer, String> {
    use windows::Win32::Media::MediaFoundation::MFCreateMemoryBuffer;
    let buffer = unsafe { MFCreateMemoryBuffer(bytes.len().min(u32::MAX as usize) as u32) }
        .map_err(|error| format!("{label} buffer creation failed: {error}"))?;
    let mut pointer = std::ptr::null_mut();
    unsafe {
        buffer
            .Lock(&mut pointer, None, None)
            .map_err(|error| format!("{label} buffer lock failed: {error}"))?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len());
        buffer
            .Unlock()
            .map_err(|error| format!("{label} buffer unlock failed: {error}"))?;
        buffer
            .SetCurrentLength(bytes.len() as u32)
            .map_err(|error| format!("{label} buffer length failed: {error}"))?;
    }
    Ok(buffer)
}

#[cfg(target_os = "windows")]
fn copy_media_buffer(
    buffer: &windows::Win32::Media::MediaFoundation::IMFMediaBuffer,
    label: &str,
) -> Result<Vec<u8>, String> {
    let mut pointer = std::ptr::null_mut();
    let mut length = 0u32;
    unsafe { buffer.Lock(&mut pointer, None, Some(&mut length)) }
        .map_err(|error| format!("{label} buffer lock failed: {error}"))?;
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length as usize) }.to_vec();
    unsafe { buffer.Unlock() }.map_err(|error| format!("{label} buffer unlock failed: {error}"))?;
    Ok(bytes)
}

fn normalize_nv12(
    bytes: &[u8],
    width: usize,
    height: usize,
    stride: usize,
) -> Result<Vec<u8>, String> {
    let source_required = stride
        .checked_mul(height)
        .and_then(|luma| luma.checked_add(stride.checked_mul(height / 2)?))
        .ok_or_else(|| "decoded NV12 size overflow".to_string())?;
    if stride < width || bytes.len() < source_required {
        return Err(format!(
            "decoded NV12 buffer is invalid ({} bytes, stride {stride}, {width}x{height})",
            bytes.len()
        ));
    }
    let mut output = vec![0u8; width * height * 3 / 2];
    for row in 0..height {
        let source = row * stride;
        let target = row * width;
        output[target..target + width].copy_from_slice(&bytes[source..source + width]);
    }
    let source_chroma = stride * height;
    let target_chroma = width * height;
    for row in 0..height / 2 {
        let source = source_chroma + row * stride;
        let target = target_chroma + row * width;
        output[target..target + width].copy_from_slice(&bytes[source..source + width]);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_annex_b_and_length_prefixed_access_units() {
        let annex_b = [0, 0, 0, 1, 0x67, 1, 0, 0, 1, 0x65, 2, 3];
        assert_eq!(
            split_h264_access_unit(&annex_b).unwrap(),
            vec![vec![0x67, 1], vec![0x65, 2, 3]]
        );
        let avcc = [0, 0, 0, 2, 0x67, 1, 0, 0, 0, 3, 0x65, 2, 3];
        assert_eq!(
            split_h264_access_unit(&avcc).unwrap(),
            vec![vec![0x67, 1], vec![0x65, 2, 3]]
        );
    }

    #[test]
    fn normalizes_padded_nv12_rows() {
        let mut source = vec![0u8; 8 * 4 * 3 / 2];
        for row in 0..4 {
            source[row * 8..row * 8 + 4].fill((row + 1) as u8);
        }
        source[32..36].fill(9);
        source[40..44].fill(10);
        let output = normalize_nv12(&source, 4, 4, 8).unwrap();
        assert_eq!(&output[0..4], &[1; 4]);
        assert_eq!(&output[12..16], &[4; 4]);
        assert_eq!(&output[16..20], &[9; 4]);
        assert_eq!(&output[20..24], &[10; 4]);
    }

    /// 在真实编码器上跑完一段帧序列。硬件 MFT（Intel Quick Sync 等）常在首帧输入
    /// 之后才最终确定输出类型并返回 `MF_E_TRANSFORM_STREAM_CHANGE`；把它当硬错误
    /// 会让整条水印管线在预热阶段就起不来。
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "requires a Windows Media Foundation H.264 encoder"]
    fn encoder_keeps_producing_output_across_a_stream_change() {
        let _runtime = MediaFoundationRuntime::startup().unwrap();
        // 主码流通常是 1080p，子码流是 720p，两种尺寸都要能撑过重新协商。
        for (width, height) in [(1920u32, 1080u32), (1280, 720)] {
            let mut encoder = MfH264Encoder::new(width, height, 25, 1, 4_000_000).unwrap();
            eprintln!(
                "{width}x{height}: encoder='{}', path={}",
                encoder.name,
                if encoder.hardware {
                    "hardware"
                } else {
                    "software"
                }
            );

            let luma_len = (width * height) as usize;
            let mut nv12 = vec![128u8; luma_len * 3 / 2];
            let mut frame_index = 0i64;
            let mut encode_next = |encoder: &mut MfH264Encoder, index: &mut i64| {
                // 每帧改变亮度，避免编码器把完全静止的画面整段跳过。
                nv12[..luma_len].fill((*index * 3) as u8);
                let sample_time = *index * encoder.frame_duration_100ns;
                *index += 1;
                encoder.encode(&nv12, sample_time).unwrap().len()
            };

            let before = (0..30)
                .map(|_| encode_next(&mut encoder, &mut frame_index))
                .sum::<usize>();
            assert!(
                before > 0,
                "{width}x{height}: no output before renegotiation"
            );

            // 编码器是否自发请求换类型取决于驱动，这里直接走一遍协商代码，确保
            // 它选出的输出类型能让后续 ProcessOutput 继续产出，而不是留成死路。
            encoder.renegotiate_output_type().unwrap();
            let after = (0..30)
                .map(|_| encode_next(&mut encoder, &mut frame_index))
                .sum::<usize>();
            assert!(after > 0, "{width}x{height}: no output after renegotiation");
            eprintln!("{width}x{height}: {before} frames before, {after} frames after");
        }
    }
}
