//! Build WebView2 asset URLs under `${update_server_url}/webview2/`.

pub const INSTALLER_FILENAME: &str = "MicrosoftEdgeWebView2RuntimeInstallerX64.exe";

pub fn installer_url(base: &str) -> String {
    format!(
        "{}/webview2/{INSTALLER_FILENAME}",
        base.trim_end_matches('/')
    )
}

pub fn sha256_url(base: &str) -> String {
    format!("{}.sha256", installer_url(base))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_without_trailing_slash() {
        assert_eq!(
            installer_url("http://192.115.1.3:8080"),
            "http://192.115.1.3:8080/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe"
        );
    }

    #[test]
    fn tolerates_trailing_slash() {
        assert_eq!(
            installer_url("http://192.115.1.3:8080/"),
            "http://192.115.1.3:8080/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe"
        );
    }

    #[test]
    fn sha256_url_appends_suffix() {
        assert_eq!(
            sha256_url("http://192.115.1.3:8080"),
            "http://192.115.1.3:8080/webview2/MicrosoftEdgeWebView2RuntimeInstallerX64.exe.sha256"
        );
    }
}
