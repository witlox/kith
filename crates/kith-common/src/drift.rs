//! Drift detection types. 4-category model (simpler than pact's 7).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriftCategory {
    Files,
    Services,
    Network,
    Packages,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DriftVector {
    pub files: f64,
    pub services: f64,
    pub network: f64,
    pub packages: f64,
}

impl DriftVector {
    /// Weighted squared magnitude. Sum of squared weighted dimensions.
    /// No sqrt — consistent with pact, cheaper, ordering preserved.
    pub fn magnitude_sq(&self, weights: &DriftWeights) -> f64 {
        (self.files * weights.files).powi(2)
            + (self.services * weights.services).powi(2)
            + (self.network * weights.network).powi(2)
            + (self.packages * weights.packages).powi(2)
    }

    /// Reset all dimensions to zero.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Increment the dimension for a given category.
    pub fn increment(&mut self, category: &DriftCategory) {
        match category {
            DriftCategory::Files => self.files += 1.0,
            DriftCategory::Services => self.services += 1.0,
            DriftCategory::Network => self.network += 1.0,
            DriftCategory::Packages => self.packages += 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftWeights {
    pub files: f64,
    pub services: f64,
    pub network: f64,
    pub packages: f64,
}

impl Default for DriftWeights {
    fn default() -> Self {
        Self {
            files: 1.0,
            services: 2.0,
            network: 1.5,
            packages: 1.0,
        }
    }
}

/// Simple glob matching for blacklist patterns.
/// Supports `**` (match anything under prefix) and exact match.
pub fn matches_blacklist(pattern: &str, path: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix("/**") {
        return path.starts_with(prefix);
    }
    pattern == path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_drift_vector_is_zero() {
        let v = DriftVector::default();
        assert_eq!(v.files, 0.0);
        assert_eq!(v.services, 0.0);
        assert_eq!(v.network, 0.0);
        assert_eq!(v.packages, 0.0);
    }

    #[test]
    fn magnitude_sq_with_default_weights() {
        let mut v = DriftVector::default();
        v.files = 2.0;
        v.services = 1.0;
        let w = DriftWeights::default();
        // (2*1)^2 + (1*2)^2 + 0 + 0 = 4 + 4 = 8
        let mag = v.magnitude_sq(&w);
        assert!((mag - 8.0).abs() < f64::EPSILON, "expected 8.0, got {mag}");
    }

    #[test]
    fn magnitude_sq_zero_weight_ignores_dimension() {
        let mut v = DriftVector::default();
        v.files = 100.0;
        let w = DriftWeights {
            files: 0.0,
            services: 1.0,
            network: 1.0,
            packages: 1.0,
        };
        assert!((v.magnitude_sq(&w)).abs() < f64::EPSILON);
    }

    #[test]
    fn increment_updates_correct_dimension() {
        let mut v = DriftVector::default();
        v.increment(&DriftCategory::Files);
        v.increment(&DriftCategory::Files);
        v.increment(&DriftCategory::Services);
        assert_eq!(v.files, 2.0);
        assert_eq!(v.services, 1.0);
        assert_eq!(v.network, 0.0);
        assert_eq!(v.packages, 0.0);
    }

    #[test]
    fn reset_clears_all_dimensions() {
        let mut v = DriftVector {
            files: 5.0,
            services: 3.0,
            network: 1.0,
            packages: 2.0,
        };
        v.reset();
        assert_eq!(v, DriftVector::default());
    }

    #[test]
    fn blacklist_glob_matching() {
        assert!(matches_blacklist("/tmp/**", "/tmp/foo"));
        assert!(matches_blacklist("/tmp/**", "/tmp/foo/bar/baz"));
        assert!(!matches_blacklist("/tmp/**", "/var/tmp/foo"));
        assert!(matches_blacklist("/var/log/**", "/var/log/syslog"));
        assert!(matches_blacklist("/etc/foo", "/etc/foo"));
        assert!(!matches_blacklist("/etc/foo", "/etc/bar"));
    }

    #[test]
    fn blacklist_filters_default_noisy_paths() {
        let patterns = [
            "/tmp/**",
            "/var/log/**",
            "/proc/**",
            "/sys/**",
            "/dev/**",
            "/run/user/**",
        ];
        let noisy = vec![
            "/tmp/scratch",
            "/var/log/syslog",
            "/proc/cpuinfo",
            "/sys/class/net",
            "/dev/null",
            "/run/user/1000/test",
        ];
        for path in noisy {
            let matched = patterns.iter().any(|p| matches_blacklist(p, path));
            assert!(matched, "{path} should be blacklisted");
        }

        let clean = vec!["/etc/nginx/conf.d/api.conf", "/home/pim/.bashrc"];
        for path in clean {
            let matched = patterns.iter().any(|p| matches_blacklist(p, path));
            assert!(!matched, "{path} should NOT be blacklisted");
        }
    }
}
