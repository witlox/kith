//! Error taxonomy for kith. All crates use KithError or InferenceError.
//! No panics in library code. No anyhow.

#[derive(Debug, thiserror::Error)]
pub enum KithError {
    #[error("authentication required")]
    Unauthenticated,

    #[error("policy denied: {reason}")]
    PolicyDenied { reason: String },

    #[error("credentials expired")]
    CredentialsExpired,

    #[error("invalid credential: {0}")]
    InvalidCredential(String),

    #[error("machine not found: {0}")]
    MachineNotFound(String),

    #[error("pending change not found: {0}")]
    PendingNotFound(String),

    #[error("commit window expired for {pending_id}")]
    CommitWindowExpired { pending_id: String },

    #[error("machine unreachable: {0}")]
    MachineUnreachable(String),

    #[error("drift detected on {machine}: {detail}")]
    DriftDetected { machine: String, detail: String },

    #[error("inference unavailable: {0}")]
    InferenceUnavailable(String),

    #[error("sync error: {0}")]
    SyncError(String),

    #[error("mesh error: {0}")]
    MeshError(String),

    #[error("containment not available: {0}")]
    ContainmentUnavailable(String),

    #[error("internal error: {0}")]
    Internal(String),
}

#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    #[error("backend unreachable: {0}")]
    Unreachable(String),

    #[error("authentication failed: {0}")]
    AuthFailed(String),

    #[error("rate limited, retry after {retry_after_ms}ms")]
    RateLimited { retry_after_ms: u64 },

    #[error("context window exceeded: {used} tokens used, {limit} limit")]
    ContextOverflow { used: u64, limit: u64 },

    #[error("malformed response: {0}")]
    MalformedResponse(String),

    #[error("timeout after {0:?}")]
    Timeout(std::time::Duration),

    #[error("backend error: {0}")]
    BackendError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kith_error_display_messages() {
        let e = KithError::PolicyDenied {
            reason: "viewer scope cannot execute".into(),
        };
        assert!(e.to_string().contains("viewer scope cannot execute"));

        let e = KithError::Unauthenticated;
        assert_eq!(e.to_string(), "authentication required");
    }

    #[test]
    fn inference_error_display_messages() {
        let e = InferenceError::RateLimited {
            retry_after_ms: 5000,
        };
        assert!(e.to_string().contains("5000"));

        let e = InferenceError::ContextOverflow {
            used: 200_000,
            limit: 128_000,
        };
        assert!(e.to_string().contains("200000"));
        assert!(e.to_string().contains("128000"));
    }
}
