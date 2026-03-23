//! Shared domain types: CapabilityReport, PeerInfo, etc.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityReport {
    pub machine: String,
    pub os: OsInfo,
    pub resources: ResourceInfo,
    pub software: Vec<SoftwareInfo>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsInfo {
    pub name: String,
    pub version: String,
    pub arch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceInfo {
    pub cpu_cores: u32,
    pub memory_bytes: u64,
    pub disk_free_bytes: u64,
    pub disk_total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoftwareInfo {
    pub name: String,
    pub version: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingChange {
    pub id: String,
    pub command: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_report_serialization() {
        let report = CapabilityReport {
            machine: "staging-1".into(),
            os: OsInfo {
                name: "Linux".into(),
                version: "6.1.0".into(),
                arch: "x86_64".into(),
            },
            resources: ResourceInfo {
                cpu_cores: 8,
                memory_bytes: 16_000_000_000,
                disk_free_bytes: 50_000_000_000,
                disk_total_bytes: 100_000_000_000,
            },
            software: vec![SoftwareInfo {
                name: "docker".into(),
                version: "24.0.7".into(),
                path: "/usr/bin/docker".into(),
            }],
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&report).unwrap();
        let parsed: CapabilityReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.machine, "staging-1");
        assert_eq!(parsed.resources.cpu_cores, 8);
        assert_eq!(parsed.software.len(), 1);
    }
}
