//! Ed25519 credential model (ADR-006).
//! Keypair generation, signing, verification.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::error::KithError;

/// A signed credential carried in every gRPC request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub public_key: Vec<u8>,
    pub timestamp_unix_ms: i64,
    pub signature: Vec<u8>,
}

/// A user's keypair. The private key never leaves the user's machine.
pub struct Keypair {
    signing_key: SigningKey,
}

impl Keypair {
    /// Generate a new random keypair.
    pub fn generate() -> Self {
        let mut rng = rand::thread_rng();
        let signing_key = SigningKey::generate(&mut rng);
        Self { signing_key }
    }

    /// Reconstruct from an existing secret key (32 bytes).
    pub fn from_secret(bytes: &[u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(bytes);
        Self { signing_key }
    }

    /// Get the public key bytes (32 bytes).
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    /// Sign a request. The signed payload is: pubkey || timestamp || request_hash.
    pub fn sign(&self, timestamp_unix_ms: i64, request_hash: &[u8]) -> Credential {
        let pubkey = self.public_key_bytes();
        let mut payload = Vec::with_capacity(32 + 8 + request_hash.len());
        payload.extend_from_slice(&pubkey);
        payload.extend_from_slice(&timestamp_unix_ms.to_le_bytes());
        payload.extend_from_slice(request_hash);

        let signature = self.signing_key.sign(&payload);

        Credential {
            public_key: pubkey.to_vec(),
            timestamp_unix_ms,
            signature: signature.to_bytes().to_vec(),
        }
    }

    /// Get the secret key bytes for storage.
    pub fn secret_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }
}

/// Verify a credential. Checks signature validity and timestamp freshness.
/// Does NOT check policy — that's the daemon's job.
pub fn verify_credential(
    credential: &Credential,
    request_hash: &[u8],
    now_unix_ms: i64,
    max_skew_ms: i64,
) -> Result<[u8; 32], KithError> {
    // Parse public key
    let pubkey_bytes: [u8; 32] = credential
        .public_key
        .as_slice()
        .try_into()
        .map_err(|_| KithError::InvalidCredential("public key must be 32 bytes".into()))?;

    let verifying_key = VerifyingKey::from_bytes(&pubkey_bytes)
        .map_err(|e| KithError::InvalidCredential(format!("invalid public key: {e}")))?;

    // Parse signature
    let sig_bytes: [u8; 64] = credential
        .signature
        .as_slice()
        .try_into()
        .map_err(|_| KithError::InvalidCredential("signature must be 64 bytes".into()))?;

    let signature = Signature::from_bytes(&sig_bytes);

    // Reconstruct signed payload
    let mut payload = Vec::with_capacity(32 + 8 + request_hash.len());
    payload.extend_from_slice(&pubkey_bytes);
    payload.extend_from_slice(&credential.timestamp_unix_ms.to_le_bytes());
    payload.extend_from_slice(request_hash);

    // Verify signature
    verifying_key
        .verify(&payload, &signature)
        .map_err(|_| KithError::InvalidCredential("signature verification failed".into()))?;

    // Check timestamp freshness (±max_skew_ms)
    let skew = (now_unix_ms - credential.timestamp_unix_ms).abs();
    if skew > max_skew_ms {
        return Err(KithError::CredentialsExpired);
    }

    Ok(pubkey_bytes)
}

/// Format a public key as a hex string for display/config.
pub fn pubkey_to_hex(pubkey: &[u8; 32]) -> String {
    hex::encode(pubkey)
}

/// Parse a hex-encoded public key.
pub fn pubkey_from_hex(hex_str: &str) -> Result<[u8; 32], KithError> {
    let bytes = hex::decode(hex_str)
        .map_err(|e| KithError::InvalidCredential(format!("invalid hex: {e}")))?;
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| KithError::InvalidCredential("public key must be 32 bytes".into()))
}

// We need the hex crate — or we inline it. Let's inline to avoid the dependency.
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn decode(s: &str) -> Result<Vec<u8>, String> {
        if s.len() % 2 != 0 {
            return Err("odd length hex string".into());
        }
        (0..s.len())
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| format!("invalid hex at {i}: {e}"))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keypair_generate_and_sign_verify() {
        let kp = Keypair::generate();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let request_hash = b"test-request-hash";

        let cred = kp.sign(now_ms, request_hash);

        let result = verify_credential(&cred, request_hash, now_ms, 30_000);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), kp.public_key_bytes());
    }

    #[test]
    fn verify_rejects_tampered_signature() {
        let kp = Keypair::generate();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let request_hash = b"test-request-hash";

        let mut cred = kp.sign(now_ms, request_hash);
        // Tamper with signature
        if let Some(byte) = cred.signature.first_mut() {
            *byte ^= 0xFF;
        }

        let result = verify_credential(&cred, request_hash, now_ms, 30_000);
        assert!(result.is_err());
        assert!(matches!(result, Err(KithError::InvalidCredential(_))));
    }

    #[test]
    fn verify_rejects_wrong_request_hash() {
        let kp = Keypair::generate();
        let now_ms = chrono::Utc::now().timestamp_millis();

        let cred = kp.sign(now_ms, b"original-hash");
        let result = verify_credential(&cred, b"different-hash", now_ms, 30_000);
        assert!(result.is_err());
    }

    #[test]
    fn verify_rejects_expired_timestamp() {
        let kp = Keypair::generate();
        let old_ms = chrono::Utc::now().timestamp_millis() - 60_000; // 60s ago
        let request_hash = b"test";

        let cred = kp.sign(old_ms, request_hash);
        let now_ms = chrono::Utc::now().timestamp_millis();

        // 30s max skew — 60s old should fail
        let result = verify_credential(&cred, request_hash, now_ms, 30_000);
        assert!(matches!(result, Err(KithError::CredentialsExpired)));
    }

    #[test]
    fn verify_accepts_within_skew_window() {
        let kp = Keypair::generate();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let slightly_old = now_ms - 10_000; // 10s ago
        let request_hash = b"test";

        let cred = kp.sign(slightly_old, request_hash);
        // 30s max skew — 10s old should pass
        let result = verify_credential(&cred, request_hash, now_ms, 30_000);
        assert!(result.is_ok());
    }

    #[test]
    fn keypair_roundtrip_from_secret() {
        let kp1 = Keypair::generate();
        let secret = kp1.secret_bytes();
        let kp2 = Keypair::from_secret(&secret);
        assert_eq!(kp1.public_key_bytes(), kp2.public_key_bytes());
    }

    #[test]
    fn pubkey_hex_roundtrip() {
        let kp = Keypair::generate();
        let pubkey = kp.public_key_bytes();
        let hex_str = pubkey_to_hex(&pubkey);
        assert_eq!(hex_str.len(), 64); // 32 bytes = 64 hex chars
        let parsed = pubkey_from_hex(&hex_str).unwrap();
        assert_eq!(parsed, pubkey);
    }

    #[test]
    fn pubkey_from_hex_rejects_invalid() {
        assert!(pubkey_from_hex("not-hex").is_err());
        assert!(pubkey_from_hex("abcd").is_err()); // too short
    }
}
