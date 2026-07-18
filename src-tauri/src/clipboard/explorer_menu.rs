//! Windows Explorer image-file context-menu registration.

#[cfg(target_os = "windows")]
const MENU_KEY: &str =
    r"Software\Classes\SystemFileAssociations\image\shell\FileSyncTool.CopyImageData";

#[cfg(target_os = "windows")]
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    if !enabled {
        match current_user.delete_subkey_all(MENU_KEY) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("remove Explorer context menu: {error}")),
        }
    }

    let executable =
        std::env::current_exe().map_err(|error| format!("resolve executable: {error}"))?;
    let executable_text = executable.to_string_lossy();
    let command_text = format!("\"{executable_text}\" --copy-image-data \"%1\"");
    let icon_text = format!("{executable_text},0");

    let (menu, _) = current_user
        .create_subkey(MENU_KEY)
        .map_err(|error| format!("create Explorer context menu: {error}"))?;
    menu.set_value("", &"复制为图片数据")
        .map_err(|error| format!("set Explorer menu label: {error}"))?;
    menu.set_value("Icon", &icon_text)
        .map_err(|error| format!("set Explorer menu icon: {error}"))?;
    menu.set_value("MultiSelectModel", &"Single")
        .map_err(|error| format!("set Explorer selection mode: {error}"))?;
    let (command, _) = menu
        .create_subkey("command")
        .map_err(|error| format!("create Explorer menu command: {error}"))?;
    command
        .set_value("", &command_text)
        .map_err(|error| format!("set Explorer menu command: {error}"))?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn set_enabled(_enabled: bool) -> Result<(), String> {
    Err("Explorer context menu is only supported on Windows".to_string())
}

#[cfg(target_os = "windows")]
pub fn is_registered() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(command) = current_user.open_subkey(format!(r"{MENU_KEY}\command")) else {
        return false;
    };
    let Ok(value) = command.get_value::<String, _>("") else {
        return false;
    };
    let Ok(executable) = std::env::current_exe() else {
        return false;
    };
    value.contains(executable.to_string_lossy().as_ref()) && value.contains("--copy-image-data")
}

#[cfg(not(target_os = "windows"))]
pub fn is_registered() -> bool {
    false
}
