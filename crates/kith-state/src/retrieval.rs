//! Retrieval interface — keyword + future vector search over events.
//! The vector index (ADR-005) will be added when an embedding library is wired.
//! For now, keyword-based retrieval provides the interface and tests.

use kith_common::event::{Event, EventScope};

/// A retrieval result with a relevance score.
#[derive(Debug, Clone)]
pub struct RetrievalResult {
    pub event: Event,
    pub score: f64,
    pub match_reason: String,
}

/// Keyword-based retrieval over a set of events.
/// This is the structured half of hybrid retrieval (ADR-005).
/// Vector search will supplement this when embeddings are available.
pub struct KeywordRetriever;

impl KeywordRetriever {
    /// Search events by keyword matching on detail, event_type, path, and metadata.
    pub fn search(
        events: &[Event],
        query: &str,
        scope: &EventScope,
        limit: usize,
    ) -> Vec<RetrievalResult> {
        let query_lower = query.to_lowercase();
        let terms: Vec<&str> = query_lower.split_whitespace().collect();

        let mut results: Vec<RetrievalResult> = events
            .iter()
            .filter(|e| match scope {
                EventScope::Ops => true,
                EventScope::Public => e.scope == EventScope::Public,
            })
            .filter_map(|e| {
                let (score, reason) = score_event(e, &terms);
                if score > 0.0 {
                    Some(RetrievalResult {
                        event: e.clone(),
                        score,
                        match_reason: reason,
                    })
                } else {
                    None
                }
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        results
    }
}

/// Score an event against search terms. Returns (score, reason).
fn score_event(event: &Event, terms: &[&str]) -> (f64, String) {
    let mut score = 0.0;
    let mut reasons = Vec::new();

    let detail_lower = event.detail.to_lowercase();
    let type_lower = event.event_type.to_lowercase();
    let path_lower = event.path.as_deref().unwrap_or("").to_lowercase();
    let metadata_str = event.metadata.to_string().to_lowercase();

    for term in terms {
        if type_lower.contains(term) {
            score += 3.0;
            reasons.push(format!("event_type match: {term}"));
        }
        if path_lower.contains(term) {
            score += 2.0;
            reasons.push(format!("path match: {term}"));
        }
        if detail_lower.contains(term) {
            score += 1.0;
            reasons.push(format!("detail match: {term}"));
        }
        if metadata_str.contains(term) {
            score += 1.0;
            reasons.push(format!("metadata match: {term}"));
        }
    }

    (score, reasons.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kith_common::event::{Event, EventCategory};

    fn drift_event(machine: &str, path: &str) -> Event {
        Event::new(machine, EventCategory::Drift, "drift.file_changed", &format!("modified {path}"))
            .with_path(path)
            .with_scope(EventScope::Public)
    }

    fn exec_event(machine: &str, command: &str) -> Event {
        Event::new(machine, EventCategory::Exec, "exec.command", command)
            .with_metadata(serde_json::json!({"command": command}))
            .with_scope(EventScope::Ops)
    }

    #[test]
    fn search_by_path() {
        let events = vec![
            drift_event("s1", "/etc/nginx/conf.d/api.conf"),
            drift_event("s1", "/etc/postgresql/pg_hba.conf"),
        ];

        let results = KeywordRetriever::search(&events, "nginx", &EventScope::Ops, 10);
        assert_eq!(results.len(), 1);
        assert!(results[0].event.path.as_deref().unwrap().contains("nginx"));
        assert!(results[0].score > 0.0);
    }

    #[test]
    fn search_by_detail() {
        let events = vec![
            exec_event("s1", "docker ps"),
            exec_event("s1", "systemctl restart nginx"),
        ];

        let results = KeywordRetriever::search(&events, "docker", &EventScope::Ops, 10);
        assert_eq!(results.len(), 1);
        assert!(results[0].event.detail.contains("docker"));
    }

    #[test]
    fn search_by_event_type() {
        let events = vec![
            drift_event("s1", "/a"),
            Event::new("s1", EventCategory::Drift, "drift.service_stopped", "nginx stopped")
                .with_scope(EventScope::Public),
        ];

        let results = KeywordRetriever::search(&events, "service_stopped", &EventScope::Ops, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event.event_type, "drift.service_stopped");
    }

    #[test]
    fn search_respects_scope() {
        let events = vec![
            drift_event("s1", "/etc/config"),  // Public
            exec_event("s1", "docker ps"),      // Ops
        ];

        // Public scope should only see the drift event
        let public = KeywordRetriever::search(&events, "docker", &EventScope::Public, 10);
        assert!(public.is_empty()); // docker event is Ops-scoped

        // Ops scope sees everything
        let ops = KeywordRetriever::search(&events, "docker", &EventScope::Ops, 10);
        assert_eq!(ops.len(), 1);
    }

    #[test]
    fn search_limits_results() {
        let events: Vec<Event> = (0..20)
            .map(|i| drift_event("s1", &format!("/etc/config-{i}")))
            .collect();

        let results = KeywordRetriever::search(&events, "config", &EventScope::Ops, 5);
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn search_ranks_by_score() {
        let events = vec![
            // Matches in event_type (3 pts) + detail (1 pt) = 4
            Event::new("s1", EventCategory::Drift, "drift.nginx_changed", "nginx config changed")
                .with_scope(EventScope::Public),
            // Matches only in detail (1 pt)
            Event::new("s1", EventCategory::Exec, "exec.command", "restarted nginx")
                .with_scope(EventScope::Ops),
        ];

        let results = KeywordRetriever::search(&events, "nginx", &EventScope::Ops, 10);
        assert_eq!(results.len(), 2);
        assert!(results[0].score >= results[1].score, "higher score should be first");
    }

    #[test]
    fn search_no_matches() {
        let events = vec![drift_event("s1", "/etc/config")];
        let results = KeywordRetriever::search(&events, "nonexistent", &EventScope::Ops, 10);
        assert!(results.is_empty());
    }

    #[test]
    fn search_multi_term() {
        let events = vec![
            drift_event("s1", "/etc/nginx/conf.d/api.conf"),
            drift_event("s1", "/etc/postgresql/pg_hba.conf"),
        ];

        // "nginx config" — should match the nginx event on both terms
        let results = KeywordRetriever::search(&events, "nginx config", &EventScope::Ops, 10);
        assert!(!results.is_empty());
        // nginx event should score higher (matches on more terms)
        assert!(results[0].event.path.as_deref().unwrap().contains("nginx"));
    }

    #[test]
    fn search_case_insensitive() {
        let events = vec![drift_event("s1", "/etc/Nginx/CONF")];
        let results = KeywordRetriever::search(&events, "nginx", &EventScope::Ops, 10);
        assert_eq!(results.len(), 1);
    }
}
