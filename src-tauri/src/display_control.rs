use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DisplayControlBackend {
    DdcCi,
    Wmi,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DisplayControlMonitor {
    pub id: String,
    pub index: usize,
    pub name: String,
    pub device_name: String,
    pub is_primary: bool,
    pub is_internal: bool,
    pub backend: DisplayControlBackend,
    pub brightness: Option<u32>,
    pub brightness_min: u32,
    pub brightness_max: u32,
    pub brightness_supported: bool,
    pub contrast: Option<u32>,
    pub contrast_min: u32,
    pub contrast_max: u32,
    pub contrast_supported: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MonitorControlFeature {
    Brightness,
    Contrast,
}

impl MonitorControlFeature {
    fn label(self) -> &'static str {
        match self {
            Self::Brightness => "brightness",
            Self::Contrast => "contrast",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MonitorControlSetRequest {
    pub monitor_id: String,
    pub feature: MonitorControlFeature,
    /// Percentage in the inclusive range 0..=100.
    pub value: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RawFeatureRange {
    minimum: u32,
    current: u32,
    maximum: u32,
}

fn normalize_to_percent(range: RawFeatureRange) -> u32 {
    if range.maximum <= range.minimum {
        return u32::from(range.current > range.minimum) * 100;
    }

    let span = u64::from(range.maximum - range.minimum);
    let offset = u64::from(range.current.clamp(range.minimum, range.maximum) - range.minimum);
    ((offset * 100 + span / 2) / span).min(100) as u32
}

fn percent_to_raw(percent: u32, minimum: u32, maximum: u32) -> u32 {
    if maximum <= minimum {
        return minimum;
    }

    let percent = percent.min(100);
    let span = u64::from(maximum - minimum);
    (u64::from(minimum) + (span * u64::from(percent) + 50) / 100) as u32
}

#[tauri::command]
pub async fn monitor_control_list() -> Result<Vec<DisplayControlMonitor>, String> {
    tokio::task::spawn_blocking(platform::list_monitors)
        .await
        .map_err(|error| format!("Monitor enumeration task failed: {error}"))?
}

#[tauri::command]
pub async fn monitor_control_set(request: MonitorControlSetRequest) -> Result<(), String> {
    if request.value > 100 {
        return Err(format!(
            "{} must be between 0 and 100",
            request.feature.label()
        ));
    }

    tokio::task::spawn_blocking(move || platform::set_feature(request))
        .await
        .map_err(|error| format!("Monitor control task failed: {error}"))?
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{
        normalize_to_percent, percent_to_raw, DisplayControlBackend, DisplayControlMonitor,
        MonitorControlFeature, MonitorControlSetRequest, RawFeatureRange,
    };
    use serde::Deserialize;
    use std::mem::size_of;
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use std::ptr;
    use windows::core::{Error as WindowsError, PCWSTR};
    use windows::Win32::Devices::Display::{
        DestroyPhysicalMonitors, GetMonitorBrightness, GetMonitorContrast,
        GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR,
        GetVCPFeatureAndVCPFeatureReply, SetMonitorBrightness, SetMonitorContrast, SetVCPFeature,
        PHYSICAL_MONITOR,
    };
    use windows::Win32::Foundation::{BOOL, HANDLE, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, DISPLAY_DEVICEW, HDC, HMONITOR,
        MONITORINFO, MONITORINFOEXW,
    };

    const MONITORINFOF_PRIMARY: u32 = 0x0000_0001;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const BRIGHTNESS_VCP_CODES: &[u8] = &[0x10, 0x13, 0x6B];
    const CONTRAST_VCP_CODES: &[u8] = &[0x12];

    #[derive(Debug, Clone)]
    struct LogicalMonitor {
        handle: HMONITOR,
        device_name: String,
        display_name: String,
        hardware_id: String,
        is_primary: bool,
    }

    #[derive(Debug, Clone, Deserialize)]
    struct WmiBrightnessEntry {
        instance_name: String,
        current_brightness: u32,
    }

    #[derive(Debug, Clone, Copy)]
    enum FeatureAccess {
        HighLevel,
        Vcp(u8),
    }

    #[derive(Debug, Clone, Copy)]
    struct FeatureReading {
        range: RawFeatureRange,
        access: FeatureAccess,
    }

    struct PhysicalMonitors(Vec<PHYSICAL_MONITOR>);

    impl PhysicalMonitors {
        fn open(logical_monitor: HMONITOR) -> Result<Self, String> {
            let mut count = 0_u32;
            unsafe {
                GetNumberOfPhysicalMonitorsFromHMONITOR(logical_monitor, &mut count)
                    .map_err(|error| format!("GetNumberOfPhysicalMonitorsFromHMONITOR: {error}"))?;
            }
            if count == 0 {
                return Ok(Self(Vec::new()));
            }

            let mut monitors = vec![PHYSICAL_MONITOR::default(); count as usize];
            let result = unsafe { GetPhysicalMonitorsFromHMONITOR(logical_monitor, &mut monitors) };
            if let Err(error) = result {
                // The API can populate part of the array before reporting an
                // error. Release any handles it did return before propagating
                // the failure so a refresh cannot leak physical monitor
                // handles.
                unsafe {
                    let _ = DestroyPhysicalMonitors(&monitors);
                }
                return Err(format!("GetPhysicalMonitorsFromHMONITOR: {error}"));
            }
            Ok(Self(monitors))
        }

        fn empty() -> Self {
            Self(Vec::new())
        }
    }

    impl Drop for PhysicalMonitors {
        fn drop(&mut self) {
            if !self.0.is_empty() {
                unsafe {
                    let _ = DestroyPhysicalMonitors(&self.0);
                }
            }
        }
    }

    pub fn list_monitors() -> Result<Vec<DisplayControlMonitor>, String> {
        let logical_monitors = enumerate_logical_monitors()?;
        let mut result = Vec::new();
        let mut wmi_entries: Option<Vec<WmiBrightnessEntry>> = None;
        let mut used_wmi = Vec::<usize>::new();

        for logical in logical_monitors {
            let physical_monitors = PhysicalMonitors::open(logical.handle)
                .unwrap_or_else(|_| PhysicalMonitors::empty());

            let physical_readings = physical_monitors
                .0
                .iter()
                .enumerate()
                .map(|(physical_index, physical)| {
                    let handle = physical_monitor_handle(physical);
                    (
                        physical_index,
                        physical_monitor_description(physical),
                        read_feature(handle, MonitorControlFeature::Brightness),
                        read_feature(handle, MonitorControlFeature::Contrast),
                    )
                })
                .collect::<Vec<_>>();
            let has_ddc_feature = physical_readings
                .iter()
                .any(|(_, _, brightness, contrast)| brightness.is_some() || contrast.is_some());

            if has_ddc_feature {
                for (physical_index, physical_name, brightness, contrast) in physical_readings {
                    let name = choose_monitor_name(
                        &logical.display_name,
                        &physical_name,
                        &logical.device_name,
                    );

                    result.push(build_monitor(
                        format!("ddc:{}:{physical_index}", logical.device_name),
                        result.len(),
                        name,
                        logical.device_name.clone(),
                        logical.is_primary,
                        false,
                        DisplayControlBackend::DdcCi,
                        brightness,
                        contrast,
                    ));
                }
                continue;
            }

            let entries =
                wmi_entries.get_or_insert_with(|| query_wmi_brightness().unwrap_or_default());
            let matching_wmi = find_wmi_entry(entries, &used_wmi, &logical.hardware_id);
            if let Some(wmi_index) = matching_wmi {
                used_wmi.push(wmi_index);
                let wmi = &entries[wmi_index];
                let brightness = FeatureReading {
                    range: RawFeatureRange {
                        minimum: 0,
                        current: wmi.current_brightness.min(100),
                        maximum: 100,
                    },
                    access: FeatureAccess::HighLevel,
                };
                result.push(build_monitor(
                    format!("wmi:{}", wmi.instance_name),
                    result.len(),
                    choose_monitor_name(&logical.display_name, "", &logical.device_name),
                    logical.device_name,
                    logical.is_primary,
                    true,
                    DisplayControlBackend::Wmi,
                    Some(brightness),
                    None,
                ));
            } else if !physical_readings.is_empty() {
                // Some internal panels expose a physical monitor handle but
                // reject all DDC/CI requests. Keep the display visible and
                // mark it unsupported when WMI cannot identify it.
                for (physical_index, physical_name, brightness, contrast) in physical_readings {
                    result.push(build_monitor(
                        format!("ddc:{}:{physical_index}", logical.device_name),
                        result.len(),
                        choose_monitor_name(
                            &logical.display_name,
                            &physical_name,
                            &logical.device_name,
                        ),
                        logical.device_name.clone(),
                        logical.is_primary,
                        false,
                        DisplayControlBackend::DdcCi,
                        brightness,
                        contrast,
                    ));
                }
            } else {
                result.push(build_monitor(
                    format!("logical:{}", logical.device_name),
                    result.len(),
                    choose_monitor_name(&logical.display_name, "", &logical.device_name),
                    logical.device_name,
                    logical.is_primary,
                    false,
                    DisplayControlBackend::Unsupported,
                    None,
                    None,
                ));
            }
        }

        if let Some(entries) = wmi_entries {
            for (wmi_index, wmi) in entries.into_iter().enumerate() {
                if used_wmi.contains(&wmi_index) {
                    continue;
                }
                let brightness = FeatureReading {
                    range: RawFeatureRange {
                        minimum: 0,
                        current: wmi.current_brightness.min(100),
                        maximum: 100,
                    },
                    access: FeatureAccess::HighLevel,
                };
                result.push(build_monitor(
                    format!("wmi:{}", wmi.instance_name),
                    result.len(),
                    format!("Built-in display {}", wmi_index + 1),
                    wmi.instance_name,
                    result.is_empty(),
                    true,
                    DisplayControlBackend::Wmi,
                    Some(brightness),
                    None,
                ));
            }
        }

        Ok(result)
    }

    pub fn set_feature(request: MonitorControlSetRequest) -> Result<(), String> {
        if let Some(instance_name) = request.monitor_id.strip_prefix("wmi:") {
            if request.feature != MonitorControlFeature::Brightness {
                return Err("WMI displays do not support contrast control".into());
            }
            return set_wmi_brightness(instance_name, request.value);
        }

        let Some(ddc_id) = request.monitor_id.strip_prefix("ddc:") else {
            return Err("This display does not expose a controllable monitor interface".into());
        };
        let (device_name, physical_index_text) = ddc_id
            .rsplit_once(':')
            .ok_or_else(|| "Invalid DDC/CI monitor id".to_string())?;
        let physical_index = physical_index_text
            .parse::<usize>()
            .map_err(|_| "Invalid DDC/CI physical monitor index".to_string())?;

        let logical = enumerate_logical_monitors()?
            .into_iter()
            .find(|monitor| monitor.device_name.eq_ignore_ascii_case(device_name))
            .ok_or_else(|| "The selected display is no longer connected".to_string())?;
        let physical_monitors = PhysicalMonitors::open(logical.handle)?;
        let physical = physical_monitors
            .0
            .get(physical_index)
            .ok_or_else(|| "The selected physical monitor is no longer available".to_string())?;
        let handle = physical_monitor_handle(physical);
        set_ddc_feature(handle, request.feature, request.value)
    }

    fn build_monitor(
        id: String,
        index: usize,
        name: String,
        device_name: String,
        is_primary: bool,
        is_internal: bool,
        backend: DisplayControlBackend,
        brightness: Option<FeatureReading>,
        contrast: Option<FeatureReading>,
    ) -> DisplayControlMonitor {
        DisplayControlMonitor {
            id,
            index,
            name,
            device_name,
            is_primary,
            is_internal,
            backend,
            brightness: brightness.map(|reading| normalize_to_percent(reading.range)),
            // Keep the UI range stable even when a monitor does not expose a
            // feature; `*_supported` is the authoritative capability flag.
            brightness_min: 0,
            brightness_max: 100,
            brightness_supported: brightness.is_some(),
            contrast: contrast.map(|reading| normalize_to_percent(reading.range)),
            contrast_min: 0,
            contrast_max: 100,
            contrast_supported: contrast.is_some(),
        }
    }

    fn enumerate_logical_monitors() -> Result<Vec<LogicalMonitor>, String> {
        unsafe extern "system" fn callback(
            monitor: HMONITOR,
            _hdc: HDC,
            _rect: *mut RECT,
            data: LPARAM,
        ) -> BOOL {
            let monitors = &mut *(data.0 as *mut Vec<HMONITOR>);
            monitors.push(monitor);
            BOOL(1)
        }

        let mut handles = Vec::<HMONITOR>::new();
        let enumeration_ok = unsafe {
            EnumDisplayMonitors(
                None,
                None,
                Some(callback),
                LPARAM(ptr::addr_of_mut!(handles) as isize),
            )
        };
        if enumeration_ok.0 == 0 {
            return Err(win32_error("EnumDisplayMonitors"));
        }

        let mut monitors = Vec::with_capacity(handles.len());
        for handle in handles {
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
            let info_ok = unsafe {
                GetMonitorInfoW(handle, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO)
            };
            if info_ok.0 == 0 {
                continue;
            }

            let device_name = wide_string(&info.szDevice);
            let display_device = display_device_details(&info.szDevice);
            monitors.push(LogicalMonitor {
                handle,
                device_name: device_name.clone(),
                display_name: display_device
                    .as_ref()
                    .map(|details| details.name.clone())
                    .unwrap_or_default(),
                hardware_id: display_device
                    .map(|details| details.hardware_id)
                    .unwrap_or_default(),
                is_primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            });
        }
        Ok(monitors)
    }

    #[derive(Debug)]
    struct DisplayDeviceDetails {
        name: String,
        hardware_id: String,
    }

    fn display_device_details(device_name: &[u16; 32]) -> Option<DisplayDeviceDetails> {
        let mut display_device = DISPLAY_DEVICEW::default();
        display_device.cb = size_of::<DISPLAY_DEVICEW>() as u32;
        let found =
            unsafe { EnumDisplayDevicesW(PCWSTR(device_name.as_ptr()), 0, &mut display_device, 0) };
        if found.0 == 0 {
            return None;
        }
        Some(DisplayDeviceDetails {
            name: wide_string(&display_device.DeviceString),
            hardware_id: wide_string(&display_device.DeviceID),
        })
    }

    fn read_feature(handle: HANDLE, feature: MonitorControlFeature) -> Option<FeatureReading> {
        read_high_level_feature(handle, feature).or_else(|| read_vcp_feature(handle, feature))
    }

    fn read_high_level_feature(
        handle: HANDLE,
        feature: MonitorControlFeature,
    ) -> Option<FeatureReading> {
        let mut minimum = 0_u32;
        let mut current = 0_u32;
        let mut maximum = 0_u32;
        let result = unsafe {
            match feature {
                MonitorControlFeature::Brightness => {
                    GetMonitorBrightness(handle, &mut minimum, &mut current, &mut maximum)
                }
                MonitorControlFeature::Contrast => {
                    GetMonitorContrast(handle, &mut minimum, &mut current, &mut maximum)
                }
            }
        };
        (result != 0 && maximum > minimum).then_some(FeatureReading {
            range: RawFeatureRange {
                minimum,
                current,
                maximum,
            },
            access: FeatureAccess::HighLevel,
        })
    }

    fn read_vcp_feature(handle: HANDLE, feature: MonitorControlFeature) -> Option<FeatureReading> {
        feature_vcp_codes(feature).iter().find_map(|code| {
            let mut current = 0_u32;
            let mut maximum = 0_u32;
            let result = unsafe {
                GetVCPFeatureAndVCPFeatureReply(
                    handle,
                    *code,
                    None,
                    &mut current,
                    Some(&mut maximum),
                )
            };
            (result != 0 && maximum > 0).then_some(FeatureReading {
                range: RawFeatureRange {
                    minimum: 0,
                    current,
                    maximum,
                },
                access: FeatureAccess::Vcp(*code),
            })
        })
    }

    fn set_ddc_feature(
        handle: HANDLE,
        feature: MonitorControlFeature,
        percent: u32,
    ) -> Result<(), String> {
        let reading = read_feature(handle, feature)
            .ok_or_else(|| format!("The monitor does not support {} control", feature.label()))?;
        let raw = percent_to_raw(percent, reading.range.minimum, reading.range.maximum);

        let succeeded = unsafe {
            match reading.access {
                FeatureAccess::HighLevel => match feature {
                    MonitorControlFeature::Brightness => SetMonitorBrightness(handle, raw) != 0,
                    MonitorControlFeature::Contrast => SetMonitorContrast(handle, raw) != 0,
                },
                FeatureAccess::Vcp(code) => SetVCPFeature(handle, code, raw) != 0,
            }
        };
        if succeeded {
            return Ok(());
        }

        if matches!(reading.access, FeatureAccess::HighLevel) {
            if let Some(vcp) = read_vcp_feature(handle, feature) {
                let fallback_raw = percent_to_raw(percent, vcp.range.minimum, vcp.range.maximum);
                let FeatureAccess::Vcp(code) = vcp.access else {
                    unreachable!("VCP fallback must carry its selected code");
                };
                let fallback_ok = unsafe { SetVCPFeature(handle, code, fallback_raw) != 0 };
                if fallback_ok {
                    return Ok(());
                }
            }
        }

        Err(win32_error(&format!("Set monitor {}", feature.label())))
    }

    fn feature_vcp_codes(feature: MonitorControlFeature) -> &'static [u8] {
        match feature {
            MonitorControlFeature::Brightness => BRIGHTNESS_VCP_CODES,
            MonitorControlFeature::Contrast => CONTRAST_VCP_CODES,
        }
    }

    fn query_wmi_brightness() -> Result<Vec<WmiBrightnessEntry>, String> {
        let script = concat!(
            "$ErrorActionPreference = 'Stop'; ",
            "$items = @(Get-CimInstance -Namespace 'root/WMI' -ClassName 'WmiMonitorBrightness' | ",
            "ForEach-Object { [pscustomobject]@{ instance_name = [string]$_.InstanceName; ",
            "current_brightness = [int]$_.CurrentBrightness } }); ",
            "ConvertTo-Json -Compress -InputObject $items"
        );
        let output = powershell_output(script)?;
        let json = output.trim().trim_start_matches('\u{feff}');
        if json.is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(json)
            .map_err(|error| format!("Failed to parse WMI brightness: {error}"))
    }

    fn set_wmi_brightness(instance_name: &str, brightness: u32) -> Result<(), String> {
        let known = query_wmi_brightness()?
            .into_iter()
            .any(|entry| entry.instance_name.eq_ignore_ascii_case(instance_name));
        if !known {
            return Err("The selected built-in display is no longer connected".into());
        }

        let instance_name = instance_name.replace('\'', "''");
        let script = format!(
            "$ErrorActionPreference = 'Stop'; \
             $target = Get-CimInstance -Namespace 'root/WMI' -ClassName 'WmiMonitorBrightnessMethods' | \
             Where-Object {{ $_.InstanceName -eq '{instance_name}' }} | Select-Object -First 1; \
             if ($null -eq $target) {{ throw 'Brightness method is unavailable' }}; \
             $result = Invoke-CimMethod -InputObject $target -MethodName 'WmiSetBrightness' \
             -Arguments @{{ Timeout = [uint32]1; Brightness = [byte]{brightness} }}; \
             if ($null -eq $result -or [int]$result.ReturnValue -ne 0) {{ throw ('WMI returned code ' + $result.ReturnValue) }}; \
             Write-Output 'ok'"
        );
        let output = powershell_output(&script)?;
        if output.trim().eq_ignore_ascii_case("ok") {
            Ok(())
        } else {
            Err("WMI brightness command did not confirm success".into())
        }
    }

    fn powershell_output(script: &str) -> Result<String, String> {
        let mut command = Command::new("powershell.exe");
        command
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                script,
            ])
            .creation_flags(CREATE_NO_WINDOW);
        let output = command
            .output()
            .map_err(|error| format!("Failed to start Windows PowerShell: {error}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(if stderr.is_empty() {
                format!("Windows PowerShell exited with {}", output.status)
            } else {
                stderr
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn find_wmi_entry(
        entries: &[WmiBrightnessEntry],
        used: &[usize],
        hardware_id: &str,
    ) -> Option<usize> {
        let model_key = monitor_model_key(hardware_id);
        if let Some(model_key) = model_key {
            if let Some(index) = entries.iter().enumerate().find_map(|(index, entry)| {
                (!used.contains(&index)
                    && monitor_model_key(&entry.instance_name).as_deref()
                        == Some(model_key.as_str()))
                .then_some(index)
            }) {
                return Some(index);
            }
        }

        let remaining: Vec<usize> = (0..entries.len())
            .filter(|index| !used.contains(index))
            .collect();
        (remaining.len() == 1).then_some(remaining[0])
    }

    fn monitor_model_key(value: &str) -> Option<String> {
        value
            .split('\\')
            .nth(1)
            .filter(|part| !part.is_empty())
            .map(str::to_ascii_uppercase)
    }

    fn choose_monitor_name(display_name: &str, physical_name: &str, fallback: &str) -> String {
        for candidate in [display_name, physical_name, fallback] {
            let candidate = candidate.trim();
            if !candidate.is_empty() && !candidate.eq_ignore_ascii_case("Generic PnP Monitor") {
                return candidate.to_string();
            }
        }
        fallback.to_string()
    }

    fn physical_monitor_handle(monitor: &PHYSICAL_MONITOR) -> HANDLE {
        unsafe { ptr::addr_of!(monitor.hPhysicalMonitor).read_unaligned() }
    }

    fn physical_monitor_description(monitor: &PHYSICAL_MONITOR) -> String {
        let description =
            unsafe { ptr::addr_of!(monitor.szPhysicalMonitorDescription).read_unaligned() };
        wide_string(&description)
    }

    fn wide_string(value: &[u16]) -> String {
        let length = value
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(value.len());
        String::from_utf16_lossy(&value[..length])
    }

    fn win32_error(operation: &str) -> String {
        format!("{operation} failed: {}", WindowsError::from_win32())
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::{DisplayControlMonitor, MonitorControlSetRequest};

    pub fn list_monitors() -> Result<Vec<DisplayControlMonitor>, String> {
        Err("Monitor control is only available on Windows".into())
    }

    pub fn set_feature(_request: MonitorControlSetRequest) -> Result<(), String> {
        Err("Monitor control is only available on Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_to_percent, percent_to_raw, DisplayControlBackend, DisplayControlMonitor,
        RawFeatureRange,
    };

    #[test]
    fn normalizes_non_standard_monitor_ranges() {
        assert_eq!(
            normalize_to_percent(RawFeatureRange {
                minimum: 20,
                current: 60,
                maximum: 100,
            }),
            50
        );
        assert_eq!(percent_to_raw(50, 20, 100), 60);
    }

    #[test]
    fn clamps_percent_values_and_current_readings() {
        assert_eq!(percent_to_raw(150, 0, 255), 255);
        assert_eq!(
            normalize_to_percent(RawFeatureRange {
                minimum: 0,
                current: 300,
                maximum: 255,
            }),
            100
        );
    }

    #[test]
    fn serializes_the_frontend_percentage_contract() {
        let monitor = DisplayControlMonitor {
            id: "ddc:display:0".into(),
            index: 0,
            name: "Test display".into(),
            device_name: "DISPLAY1".into(),
            is_primary: true,
            is_internal: false,
            backend: DisplayControlBackend::DdcCi,
            brightness: Some(42),
            brightness_min: 0,
            brightness_max: 100,
            brightness_supported: true,
            contrast: None,
            contrast_min: 0,
            contrast_max: 100,
            contrast_supported: false,
        };

        let value = serde_json::to_value(monitor).expect("serialize monitor contract");
        assert_eq!(value["backend"], "ddc_ci");
        assert_eq!(value["brightness"], 42);
        assert_eq!(value["brightness_min"], 0);
        assert_eq!(value["brightness_max"], 100);
        assert!(value["contrast"].is_null());
    }
}
