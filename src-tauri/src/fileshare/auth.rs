use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use ipnet::IpNet;
use uuid::Uuid;

use super::model::{
    FileSharePermission, FileSharePermissionSet, IpFilterMode, UserRootPermissions,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpRule {
    Exact(IpAddr),
    Network(IpNet),
}

#[derive(Debug, Clone)]
pub enum SessionSubject {
    Guest { username: String },
    Account { username: String },
}

impl SessionSubject {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn username(&self) -> &str {
        match self {
            Self::Guest { username } | Self::Account { username } => username,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_guest(&self) -> bool {
        matches!(self, Self::Guest { .. })
    }
}

#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub subject: SessionSubject,
    pub expires_at: Instant,
    pub client_ip: String,
    pub last_seen_at: Instant,
    ttl: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPrincipal {
    pub username: String,
    pub is_guest: bool,
    pub permissions: FileSharePermissionSet,
    pub root_permissions: Vec<UserRootPermissions>,
}

impl ResolvedPrincipal {
    pub fn permissions_for_root(&self, root_id: &str) -> Option<FileSharePermissionSet> {
        self.root_permissions
            .iter()
            .find(|r| r.root_id == root_id)
            .map(|r| r.permissions.clone())
    }
}

#[derive(Default)]
pub struct SessionStore {
    sessions: HashMap<String, SessionRecord>,
}

pub fn parse_ip_rules(rules: &[String]) -> Result<Vec<IpRule>, String> {
    rules
        .iter()
        .map(|rule| parse_ip_rule(rule))
        .collect::<Result<Vec<_>, _>>()
}

pub fn is_ip_allowed(mode: IpFilterMode, rules: &[IpRule], ip: IpAddr) -> bool {
    let matched = rules.iter().any(|rule| rule.matches(ip));

    match mode {
        IpFilterMode::Off => true,
        IpFilterMode::Whitelist => !rules.is_empty() && matched,
        IpFilterMode::Blacklist => !matched,
    }
}

impl SessionStore {
    pub fn create(&mut self, subject: SessionSubject, ttl: Duration, client_ip: String) -> String {
        let now = Instant::now();
        let token = Uuid::new_v4().simple().to_string();
        self.sessions.insert(
            token.clone(),
            SessionRecord {
                subject,
                expires_at: now + ttl,
                client_ip,
                last_seen_at: now,
                ttl,
            },
        );
        token
    }

    pub fn validate(&mut self, token: &str, client_ip: &str) -> Option<SessionRecord> {
        let now = Instant::now();
        self.sessions.retain(|_, record| record.expires_at > now);

        let record = self.sessions.get_mut(token)?;
        if record.client_ip != client_ip {
            return None;
        }

        record.last_seen_at = now;
        record.expires_at = now + record.ttl;
        Some(record.clone())
    }

    pub fn logout(&mut self, token: &str) {
        self.sessions.remove(token);
    }
}

pub fn require_permission(
    principal: &ResolvedPrincipal,
    permission: FileSharePermission,
) -> Result<(), StatusCode> {
    if principal.permissions.allows(permission) {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

fn parse_ip_rule(rule: &str) -> Result<IpRule, String> {
    let trimmed = rule.trim();
    if trimmed.is_empty() {
        return Err("IP rule cannot be empty".to_string());
    }

    if trimmed.contains('/') {
        trimmed
            .parse::<IpNet>()
            .map(IpRule::Network)
            .map_err(|e| format!("Invalid CIDR rule '{}': {}", trimmed, e))
    } else {
        trimmed
            .parse::<IpAddr>()
            .map(IpRule::Exact)
            .map_err(|e| format!("Invalid IP rule '{}': {}", trimmed, e))
    }
}

impl IpRule {
    fn matches(&self, ip: IpAddr) -> bool {
        match self {
            IpRule::Exact(value) => *value == ip,
            IpRule::Network(value) => value.contains(&ip),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::fileshare::model::FileSharePermissionSet;

    #[test]
    fn whitelist_mode_rejects_ip_outside_rules() {
        let rules = parse_ip_rules(&["192.168.0.0/24".into()]).expect("rules should parse");
        let allowed = is_ip_allowed(IpFilterMode::Whitelist, &rules, "10.0.0.5".parse().unwrap());
        assert!(!allowed);
    }

    #[test]
    fn session_expires_after_ttl() {
        let mut store = SessionStore::default();
        let token = store.create(
            SessionSubject::Guest {
                username: "guest".into(),
            },
            Duration::from_secs(1),
            "192.168.0.8".into(),
        );
        std::thread::sleep(Duration::from_millis(1200));
        assert!(store.validate(&token, "192.168.0.8").is_none());
    }

    #[test]
    fn session_validates_matching_ip_before_expiry() {
        let mut store = SessionStore::default();
        let token = store.create(
            SessionSubject::Guest {
                username: "guest".into(),
            },
            Duration::from_secs(60),
            "192.168.0.8".into(),
        );

        let session = store
            .validate(&token, "192.168.0.8")
            .expect("session should still be active");

        assert_eq!(session.subject.username(), "guest");
        assert!(session.subject.is_guest());
        assert_eq!(session.client_ip, "192.168.0.8");
    }

    #[test]
    fn require_permission_rejects_forbidden_actions() {
        let principal = ResolvedPrincipal {
            username: "guest".to_string(),
            is_guest: true,
            permissions: FileSharePermissionSet::read_only(),
            root_permissions: Vec::new(),
        };

        let result = require_permission(&principal, FileSharePermission::Delete);

        assert_eq!(result, Err(StatusCode::FORBIDDEN));
    }
}
