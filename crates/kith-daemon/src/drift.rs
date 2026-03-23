//! Drift evaluator — processes observer events, maintains drift vector.
//! Borrows pact's pattern: blacklist filtering, weighted magnitude, reset on commit.

use kith_common::drift::{matches_blacklist, DriftCategory, DriftVector, DriftWeights};

/// An event from the state observer (inotify, process poll, etc.)
#[derive(Debug, Clone)]
pub struct ObserverEvent {
    pub category: DriftCategory,
    pub path: String,
    pub detail: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

pub struct DriftEvaluator {
    blacklist: Vec<String>,
    weights: DriftWeights,
    current: DriftVector,
}

impl DriftEvaluator {
    pub fn new(blacklist: Vec<String>, weights: DriftWeights) -> Self {
        Self {
            blacklist,
            weights,
            current: DriftVector::default(),
        }
    }

    /// Process an observer event. Returns true if drift was updated (not blacklisted).
    pub fn process_event(&mut self, event: &ObserverEvent) -> bool {
        if self.is_blacklisted(&event.path) {
            return false;
        }
        self.current.increment(&event.category);
        true
    }

    /// Current weighted squared magnitude.
    pub fn magnitude_sq(&self) -> f64 {
        self.current.magnitude_sq(&self.weights)
    }

    /// Current drift vector.
    pub fn drift_vector(&self) -> &DriftVector {
        &self.current
    }

    /// Reset drift (after commit).
    pub fn reset(&mut self) {
        self.current.reset();
    }

    /// Update weights.
    pub fn set_weights(&mut self, weights: DriftWeights) {
        self.weights = weights;
    }

    fn is_blacklisted(&self, path: &str) -> bool {
        self.blacklist.iter().any(|p| matches_blacklist(p, path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_blacklist() -> Vec<String> {
        vec![
            "/tmp/**".into(),
            "/var/log/**".into(),
            "/proc/**".into(),
            "/sys/**".into(),
            "/dev/**".into(),
            "/run/user/**".into(),
        ]
    }

    fn event(category: DriftCategory, path: &str) -> ObserverEvent {
        ObserverEvent {
            category,
            path: path.into(),
            detail: String::new(),
            timestamp: chrono::Utc::now(),
        }
    }

    #[test]
    fn blacklisted_events_ignored() {
        let mut eval = DriftEvaluator::new(default_blacklist(), DriftWeights::default());
        assert!(!eval.process_event(&event(DriftCategory::Files, "/tmp/scratch")));
        assert!(!eval.process_event(&event(DriftCategory::Files, "/proc/cpuinfo")));
        assert!(!eval.process_event(&event(DriftCategory::Files, "/var/log/syslog")));
        assert_eq!(eval.magnitude_sq(), 0.0);
    }

    #[test]
    fn non_blacklisted_events_counted() {
        let mut eval = DriftEvaluator::new(default_blacklist(), DriftWeights::default());
        assert!(eval.process_event(&event(DriftCategory::Files, "/etc/nginx/conf.d/api.conf")));
        assert!(eval.process_event(&event(DriftCategory::Services, "nginx")));
        assert_eq!(eval.drift_vector().files, 1.0);
        assert_eq!(eval.drift_vector().services, 1.0);
    }

    #[test]
    fn magnitude_matches_spec() {
        // From drift-detection.feature: 2 files + 1 service, default weights -> magnitude_sq = 8.0
        let mut eval = DriftEvaluator::new(vec![], DriftWeights::default());
        eval.process_event(&event(DriftCategory::Files, "/a"));
        eval.process_event(&event(DriftCategory::Files, "/b"));
        eval.process_event(&event(DriftCategory::Services, "svc"));
        // (2*1)^2 + (1*2)^2 = 4 + 4 = 8
        assert!((eval.magnitude_sq() - 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn reset_clears_drift() {
        let mut eval = DriftEvaluator::new(vec![], DriftWeights::default());
        eval.process_event(&event(DriftCategory::Files, "/etc/config"));
        assert!(eval.magnitude_sq() > 0.0);
        eval.reset();
        assert_eq!(eval.magnitude_sq(), 0.0);
    }

    #[test]
    fn custom_blacklist() {
        let mut eval = DriftEvaluator::new(vec!["/scratch/**".into()], DriftWeights::default());
        assert!(!eval.process_event(&event(DriftCategory::Files, "/scratch/job123/output")));
        assert!(eval.process_event(&event(DriftCategory::Files, "/etc/config")));
    }
}
