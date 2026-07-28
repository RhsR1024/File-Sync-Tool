//! 跨提权等级的单实例守卫（在进入 `tauri::Builder` 之前执行）。
//!
//! 背景：`tauri-plugin-single-instance` 用**默认 DACL** 创建命名互斥体判重。
//! 提权进程创建的内核对象默认只授权 Administrators/SYSTEM，普通实例打开它时
//! `CreateMutexW` 返回的是 `ERROR_ACCESS_DENIED` 而不是 `ERROR_ALREADY_EXISTS`，
//! 插件据此误判自己是首个实例。开机场景下管理员计划任务（ONLOGON）先于
//! explorer 处理 Run 键拉起提权实例，随后 Run 键再拉起普通实例，就会出现
//! 两个主窗口同时存在且互相争抢剪贴板数据库/计划任务的双开故障。
//!
//! 这里用「NULL DACL（Everyone 完全访问）」的互斥体先判重，让判重结果只取决
//! 于是否已有实例、与两个实例的提权等级和启动顺序无关；发现重复时通过插件
//! 的隐藏窗口通知已有实例显示主窗口，然后本进程直接退出。

/// 与 `tauri-plugin-single-instance` windows 实现约定一致的 WM_COPYDATA 标记值
/// （插件源码中的 `WMCOPYDATA_SINGLE_INSTANCE_DATA`），复用它可直接触发插件回调。
#[cfg(target_os = "windows")]
const PLUGIN_COPYDATA_MARKER: usize = 1542;

/// 构造与插件 `WM_COPYDATA` 解析格式一致的负载：`"{cwd}|{arg0}|{arg1}...\0"`，
/// 插件按 `'|'` 切分后取第一段为 cwd、其余为 args。
#[cfg(any(target_os = "windows", test))]
fn build_notify_payload(cwd: &str, args: impl Iterator<Item = String>) -> String {
    let args = args.collect::<Vec<_>>().join("|");
    format!("{cwd}|{args}\0")
}

/// 首个实例：占住守卫互斥体后返回；发现已有实例：通知其显示主窗口并退出本进程。
/// 判重失败（非权限原因）时按“无守卫”放行，由单实例插件兜底，避免误杀唯一实例。
#[cfg(target_os = "windows")]
pub fn ensure_single_instance(identifier: &str) {
    windows_impl::ensure_single_instance(identifier);
}

#[cfg(not(target_os = "windows"))]
pub fn ensure_single_instance(_identifier: &str) {}

/// 允许低完整性等级实例向本（可能提权的）实例投递 WM_COPYDATA：UIPI 默认
/// 拦截“低→高”的窗口消息，不放行的话普通实例双击 exe 无法唤起提权实例的主
/// 窗口。应在单实例插件建好隐藏窗口后（setup 阶段）调用；未提权时是无害空操作。
#[cfg(target_os = "windows")]
pub fn allow_notifications_from_lower_integrity(identifier: &str) {
    windows_impl::allow_notifications_from_lower_integrity(identifier);
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{
        GetLastError, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, LPARAM, LRESULT, WPARAM,
    };
    use windows::Win32::Security::{
        InitializeSecurityDescriptor, SetSecurityDescriptorDacl, PSECURITY_DESCRIPTOR,
        SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR,
    };
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::{
        ChangeWindowMessageFilterEx, FindWindowW, MessageBoxW, SendMessageTimeoutW, MB_ICONWARNING,
        MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MSGFLT_ALLOW, SMTO_ABORTIFHUNG, WM_COPYDATA,
    };

    /// `SECURITY_DESCRIPTOR_REVISION`。windows crate 需要 Win32_System_SystemServices
    /// 特性才导出该常量，这里直接用协议固定值。
    const SECURITY_DESCRIPTOR_REVISION1: u32 = 1;
    /// 已有实例可能正处于启动阶段（主线程忙于 setup），限时投递避免本进程
    /// 卡死在同步 SendMessage 上。
    const NOTIFY_TIMEOUT_MS: u32 = 5_000;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn ensure_single_instance(identifier: &str) {
        let mutex_name = wide(&format!("{identifier}-si-guard"));
        unsafe {
            // NULL DACL = Everyone 完全访问：提权与普通实例都能打开同名互斥体，
            // 判重不再受创建方提权等级影响。
            let mut sd = SECURITY_DESCRIPTOR::default();
            let psd = PSECURITY_DESCRIPTOR(&mut sd as *mut _ as *mut _);
            let sd_ok = InitializeSecurityDescriptor(psd, SECURITY_DESCRIPTOR_REVISION1).is_ok()
                && SetSecurityDescriptorDacl(psd, true, None, false).is_ok();
            let sa = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: psd.0,
                bInheritHandle: false.into(),
            };
            let attrs = sd_ok.then_some(&sa as *const SECURITY_ATTRIBUTES);

            match CreateMutexW(attrs, true, PCWSTR(mutex_name.as_ptr())) {
                Ok(_handle) => {
                    if GetLastError() == ERROR_ALREADY_EXISTS {
                        notify_primary_and_exit(identifier);
                    }
                    // 首个实例：句柄故意不关闭，互斥体存活到进程退出由系统回收
                    // （windows::HANDLE 无 Drop，离开作用域不会 CloseHandle）。
                }
                Err(error) if error.code() == ERROR_ACCESS_DENIED.to_hresult() => {
                    // 对端实例用默认 DACL 创建了同名互斥体（如旧版本）：权限被拒
                    // 本身就证明互斥体已存在，同样按“已有实例”处理。
                    notify_primary_and_exit(identifier);
                }
                Err(error) => {
                    crate::startup_log(
                        "warn",
                        &format!("单实例守卫互斥体创建失败，跳过守卫由插件兜底：{error}"),
                    );
                }
            }
        }
    }

    /// 通过单实例插件在已有实例里创建的隐藏窗口（类名 `{id}-sic`、窗口名
    /// `{id}-siw`）投递 WM_COPYDATA，触发其回调显示主窗口，然后退出本进程。
    fn notify_primary_and_exit(identifier: &str) -> ! {
        crate::startup_log(
            "info",
            "单实例守卫：检测到已有实例正在运行，通知其显示主窗口后退出本实例",
        );
        unsafe {
            let class_name = wide(&format!("{identifier}-sic"));
            let window_name = wide(&format!("{identifier}-siw"));
            match FindWindowW(PCWSTR(class_name.as_ptr()), PCWSTR(window_name.as_ptr())) {
                Ok(hwnd) => {
                    let cwd = std::env::current_dir().unwrap_or_default();
                    let payload =
                        super::build_notify_payload(&cwd.to_string_lossy(), std::env::args());
                    let bytes = payload.as_bytes();
                    let cds = COPYDATASTRUCT {
                        dwData: super::PLUGIN_COPYDATA_MARKER,
                        cbData: bytes.len() as u32,
                        lpData: bytes.as_ptr() as *mut core::ffi::c_void,
                    };
                    let send_result = SendMessageTimeoutW(
                        hwnd,
                        WM_COPYDATA,
                        WPARAM(0),
                        LPARAM(&cds as *const _ as isize),
                        SMTO_ABORTIFHUNG,
                        NOTIFY_TIMEOUT_MS,
                        None,
                    );
                    // 返回 0 = 超时或投递失败（如 UIPI 拦截“低完整性 → 提权实例”），
                    // 此时已有实例不会弹出主窗口——落日志便于现场定位“双击没反应”。
                    if send_result == LRESULT(0) {
                        crate::startup_log(
                            "warn",
                            &format!(
                                "单实例守卫：通知已有实例超时或被拦截（GetLastError={:?}），其主窗口可能不会弹出",
                                GetLastError()
                            ),
                        );
                        // 用户视角这里等价于“双击没反应”：已有实例真实存在但唤醒失败，
                        // 不弹提示的话本进程会直接静默退出，看起来像 exe 打不开。
                        show_wake_failed_dialog();
                    } else {
                        crate::startup_log("info", "单实例守卫：已成功通知已有实例显示主窗口");
                    }
                }
                Err(error) => {
                    crate::startup_log(
                        "warn",
                        &format!(
                            "单实例守卫：未找到已有实例的接收窗口（可能仍在启动中），无法通知其显示主窗口：{error}"
                        ),
                    );
                }
            }
        }
        std::process::exit(0);
    }

    /// 已有实例存在但唤醒失败时的用户可见反馈：不弹的话本进程会静默退出，
    /// 双击 exe 在用户看来就是“毫无反应”。无父窗口可依附，用置顶+抢前台
    /// 保证消息框不会被已有实例（哪怕它卡死）挡住或压到后台。
    fn show_wake_failed_dialog() {
        let title = wide("文件同步工具");
        let text = wide(
            "检测到程序已在运行，但未能唤醒其窗口（可能已卡死）。\n\n\
             请在任务管理器中结束所有相关的旧进程后重新打开本程序。",
        );
        unsafe {
            MessageBoxW(
                None,
                PCWSTR(text.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONWARNING | MB_TOPMOST | MB_SETFOREGROUND,
            );
        }
    }

    pub fn allow_notifications_from_lower_integrity(identifier: &str) {
        unsafe {
            let class_name = wide(&format!("{identifier}-sic"));
            let window_name = wide(&format!("{identifier}-siw"));
            if let Ok(hwnd) = FindWindowW(PCWSTR(class_name.as_ptr()), PCWSTR(window_name.as_ptr()))
            {
                let _ = ChangeWindowMessageFilterEx(hwnd, WM_COPYDATA, MSGFLT_ALLOW, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::build_notify_payload;

    /// 负载格式必须与插件解析约定（首段 cwd、其余 args、NUL 结尾）保持一致。
    #[test]
    fn notify_payload_matches_plugin_parsing_contract() {
        let payload = build_notify_payload(
            r"C:\Users\me",
            ["app.exe".to_string(), "--flag".to_string()].into_iter(),
        );
        assert_eq!(payload, "C:\\Users\\me|app.exe|--flag\0");

        let trimmed = payload.trim_end_matches('\0');
        let mut parts = trimmed.split('|');
        assert_eq!(parts.next(), Some(r"C:\Users\me"));
        assert_eq!(parts.collect::<Vec<_>>(), vec!["app.exe", "--flag"]);
    }
}
