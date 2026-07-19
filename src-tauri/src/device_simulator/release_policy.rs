//! Approved first-release boundaries that later runtime phases must enforce.

pub const FIRST_RELEASE_APP_VERSION: &str = "1.2.0";
pub const FIRST_RELEASE_ENGINE_API: u32 = 1;
pub const FIRST_RELEASE_SCHEMA_VERSION: u32 = 1;

pub const MANAGE_FIREWALL_BY_DEFAULT: bool = true;
pub const REQUIRE_FIREWALL_START_CONFIRMATION: bool = true;

pub const USER_IMAGE_MAX_BYTES: u64 = 20 * 1024 * 1024;
pub const USER_IMAGE_MAX_DIMENSION: u32 = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetServerAuthenticationPolicy {
    None,
}

pub const ASSET_SERVER_AUTHENTICATION: AssetServerAuthenticationPolicy =
    AssetServerAuthenticationPolicy::None;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirewallCleanupPolicy {
    SessionOwnedJournalEntriesOnly,
}

pub const FIREWALL_CLEANUP_POLICY: FirewallCleanupPolicy =
    FirewallCleanupPolicy::SessionOwnedJournalEntriesOnly;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyAssetAuthorizationPolicy {
    pub testing: bool,
    pub learning: bool,
    pub copying: bool,
    pub packaging: bool,
    pub commercial_use: bool,
}

/// User-confirmed authorization boundary. Pack builders and release tooling
/// must preserve the non-commercial restriction in generated metadata.
pub const LEGACY_ASSET_AUTHORIZATION: LegacyAssetAuthorizationPolicy =
    LegacyAssetAuthorizationPolicy {
        testing: true,
        learning: true,
        copying: true,
        packaging: true,
        commercial_use: false,
    };

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirstReleaseLoadGate {
    pub online_devices: u16,
    pub concurrent_rtsp_clients: u16,
    pub video_bitrate_bps: u32,
    pub duration_minutes: u16,
    pub maximum_average_cpu_percent: u8,
    pub maximum_rss_bytes: u64,
}

pub const FIRST_RELEASE_LOAD_GATE: FirstReleaseLoadGate = FirstReleaseLoadGate {
    online_devices: 500,
    concurrent_rtsp_clients: 100,
    video_bitrate_bps: 2_000_000,
    duration_minutes: 60,
    maximum_average_cpu_percent: 80,
    maximum_rss_bytes: 4 * 1024 * 1024 * 1024,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_simulator::assets::validation::{
        SUPPORTED_ENGINE_API, SUPPORTED_SCHEMA_VERSION,
    };

    #[test]
    fn release_versions_match_the_asset_contract() {
        assert_eq!(FIRST_RELEASE_APP_VERSION, "1.2.0");
        assert_eq!(FIRST_RELEASE_ENGINE_API, SUPPORTED_ENGINE_API);
        assert_eq!(FIRST_RELEASE_SCHEMA_VERSION, SUPPORTED_SCHEMA_VERSION);
    }

    #[test]
    fn security_defaults_preserve_non_commercial_asset_authorization() {
        assert!(MANAGE_FIREWALL_BY_DEFAULT);
        assert!(REQUIRE_FIREWALL_START_CONFIRMATION);
        assert_eq!(
            FIREWALL_CLEANUP_POLICY,
            FirewallCleanupPolicy::SessionOwnedJournalEntriesOnly
        );
        assert!(LEGACY_ASSET_AUTHORIZATION.testing);
        assert!(LEGACY_ASSET_AUTHORIZATION.learning);
        assert!(LEGACY_ASSET_AUTHORIZATION.copying);
        assert!(LEGACY_ASSET_AUTHORIZATION.packaging);
        assert!(!LEGACY_ASSET_AUTHORIZATION.commercial_use);
        assert_eq!(
            ASSET_SERVER_AUTHENTICATION,
            AssetServerAuthenticationPolicy::None
        );
    }

    #[test]
    fn load_and_user_image_limits_are_explicit() {
        assert_eq!(FIRST_RELEASE_LOAD_GATE.online_devices, 500);
        assert_eq!(FIRST_RELEASE_LOAD_GATE.concurrent_rtsp_clients, 100);
        assert_eq!(FIRST_RELEASE_LOAD_GATE.video_bitrate_bps, 2_000_000);
        assert_eq!(USER_IMAGE_MAX_BYTES, 20 * 1024 * 1024);
        assert_eq!(USER_IMAGE_MAX_DIMENSION, 8192);
    }
}
