//! Wallet login: EIP-191 challenge-response. No gas, no transaction — the
//! player signs one message, we recover the address, we mint a session token.
//!
//! The challenge is wallet-bound and single-use, stored in Postgres so a
//! restart cannot invalidate in-flight logins.

use alloy_primitives::{B256, Signature, eip191_hash_message};
use anyhow::{Result, bail};
use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use chrono::{Duration, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::config::Config;
use crate::error::ApiError;
use crate::store::Store;

pub const CHALLENGE_PREFIX: &str = "AMP_AUTH:v1";
const CHALLENGE_TTL_SECS: i64 = 300;
const MAX_OUTSTANDING_PER_WALLET: i64 = 5;

#[derive(Clone)]
pub struct AuthService {
    store: Store,
    session_ttl_hours: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Player {
    pub wallet: String,
    pub region: String,
    pub language: String,
}

impl AuthService {
    pub fn new(store: Store, session_ttl_hours: i64) -> Self {
        Self {
            store,
            session_ttl_hours,
        }
    }

    pub async fn create_challenge(
        &self,
        wallet: &str,
    ) -> Result<(String, chrono::DateTime<Utc>), ApiError> {
        let wallet = normalize_wallet(wallet)?;
        let outstanding = self
            .store
            .outstanding_challenges(&wallet)
            .await
            .map_err(ApiError::Database)?;
        if outstanding >= MAX_OUTSTANDING_PER_WALLET {
            return Err(ApiError::BadRequest(
                "too many outstanding challenges; finish or wait for one to expire".into(),
            ));
        }
        let nonce = uuid::Uuid::new_v4().to_string();
        let message = format!("{CHALLENGE_PREFIX}:{nonce}");
        let expires_at = Utc::now() + Duration::seconds(CHALLENGE_TTL_SECS);
        self.store
            .insert_challenge(&nonce, &wallet, expires_at)
            .await
            .map_err(ApiError::Database)?;
        Ok((message, expires_at))
    }

    /// Verify an EIP-191 signature over a challenge we issued for this wallet.
    /// On success: consume the challenge, upsert the player, mint a session.
    pub async fn verify_login(
        &self,
        wallet: &str,
        signature_hex: &str,
        challenge: &str,
        region: Option<&str>,
    ) -> Result<(String, chrono::DateTime<Utc>, Player), ApiError> {
        let wallet = normalize_wallet(wallet)?;
        let nonce = challenge_nonce(challenge)
            .ok_or_else(|| ApiError::BadRequest("malformed challenge".into()))?;

        let recovered = recover_eip191(challenge.as_bytes(), signature_hex)
            .map_err(|e| ApiError::BadRequest(format!("bad signature: {e}")))?;
        if format!("{recovered:#x}").to_lowercase() != wallet {
            return Err(ApiError::BadRequest(
                "signature does not match wallet".into(),
            ));
        }

        let consumed = self
            .store
            .consume_challenge(&nonce, &wallet)
            .await
            .map_err(ApiError::Database)?;
        if !consumed {
            return Err(ApiError::BadRequest(
                "challenge unknown, expired, or already used".into(),
            ));
        }

        let region = region.map(str::to_string).unwrap_or_else(|| "na".into());
        self.store
            .upsert_player(&wallet, &region, "en")
            .await
            .map_err(ApiError::Database)?;

        let mut token_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut token_bytes);
        let token = format!("amp_{}", hex::encode(token_bytes));
        let expires_at = Utc::now() + Duration::hours(self.session_ttl_hours);
        self.store
            .insert_session(&hash_token(&token), &wallet, expires_at)
            .await
            .map_err(ApiError::Database)?;

        Ok((
            token,
            expires_at,
            Player {
                wallet,
                region,
                language: "en".into(),
            },
        ))
    }

    pub async fn session_wallet(&self, token: &str) -> Result<String, ApiError> {
        let wallet = self
            .store
            .session_wallet(&hash_token(token))
            .await
            .map_err(ApiError::Database)?
            .ok_or(ApiError::Unauthorized)?;
        Ok(wallet)
    }
}

/// Axum extractor: `Authorization: Bearer amp_...` → wallet string.
pub struct Authed(pub String);

impl<S> FromRequestParts<S> for Authed
where
    S: Send + Sync,
    AuthService: axum::extract::FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth = AuthService::from_ref(state);
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or(ApiError::Unauthorized)?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or(ApiError::Unauthorized)?
            .trim();
        if !token.starts_with("amp_") {
            return Err(ApiError::Unauthorized);
        }
        let wallet = auth.session_wallet(token).await?;
        parts.extensions.insert(wallet.clone());
        Ok(Authed(wallet))
    }
}

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub fn normalize_wallet(input: &str) -> Result<String, ApiError> {
    let w = input.trim();
    if !w.starts_with("0x") || w.len() != 42 {
        return Err(ApiError::BadRequest(
            "wallet must be a 0x-prefixed 20-byte address".into(),
        ));
    }
    Ok(w.to_lowercase())
}

fn challenge_nonce(challenge: &str) -> Option<String> {
    challenge
        .strip_prefix(CHALLENGE_PREFIX)?
        .strip_prefix(':')
        .map(str::to_string)
}

/// Recover the signer address from an EIP-191 `personal_sign` signature over
/// `message`. Signature is 65-byte hex (r || s || v).
pub fn recover_eip191(message: &[u8], signature_hex: &str) -> Result<alloy_primitives::Address> {
    let sig_bytes = hex::decode(signature_hex.trim_start_matches("0x"))?;
    if sig_bytes.len() != 65 {
        bail!("signature must be 65 bytes, got {}", sig_bytes.len());
    }
    let arr: [u8; 65] = sig_bytes.as_slice().try_into().unwrap();
    let sig =
        Signature::from_raw_array(&arr).map_err(|e| anyhow::anyhow!("invalid signature: {e}"))?;
    let prehash: B256 = eip191_hash_message(message);
    sig.recover_address_from_prehash(&prehash)
        .map_err(|e| anyhow::anyhow!("recovery failed: {e}"))
}

// Keep Config referenced for future per-deployment auth knobs without
// breaking the constructor signature used in main.rs.
#[allow(dead_code)]
fn _config_type_check(_cfg: &Config) {}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::keccak256;
    use alloy_signer::SignerSync;
    use alloy_signer_local::PrivateKeySigner;

    fn test_wallet() -> PrivateKeySigner {
        PrivateKeySigner::from_slice(keccak256(b"amp-server-auth-test-wallet").as_slice()).unwrap()
    }

    fn sign_eip191(wallet: &PrivateKeySigner, msg: &[u8]) -> String {
        let h: B256 = eip191_hash_message(msg);
        let sig = wallet.sign_hash_sync(&h).unwrap();
        let bytes = sig.as_bytes();
        let mut out = bytes.to_vec();
        // alloy yields v in {0,1}; personal_sign tooling expects {27,28}
        if out[64] < 27 {
            out[64] += 27;
        }
        hex::encode(out)
    }

    #[test]
    fn recover_round_trip() {
        let wallet = test_wallet();
        let msg = format!("{CHALLENGE_PREFIX}:abc123");
        let sig = sign_eip191(&wallet, msg.as_bytes());
        let recovered = recover_eip191(msg.as_bytes(), &sig).unwrap();
        assert_eq!(
            format!("{recovered:#x}").to_lowercase(),
            format!("{:#x}", wallet.address())
        );
    }

    #[test]
    fn recover_rejects_short_sig() {
        assert!(recover_eip191(b"msg", "00").is_err());
        assert!(recover_eip191(b"msg", &hex::encode([0u8; 40])).is_err());
    }

    #[test]
    fn recover_rejects_garbage() {
        assert!(recover_eip191(b"msg", &"ff".repeat(65)).is_err());
    }

    #[test]
    fn nonce_extraction() {
        assert_eq!(
            challenge_nonce("AMP_AUTH:v1:n-123").as_deref(),
            Some("n-123")
        );
        assert!(challenge_nonce("WRONG:n-123").is_none());
        assert!(challenge_nonce("AMP_AUTH:v1").is_none());
    }

    #[test]
    fn normalize_rejects_bad_wallets() {
        assert!(normalize_wallet("0x1234").is_err());
        assert!(normalize_wallet("nope").is_err());
        assert_eq!(
            normalize_wallet("0xABCDEF0123456789ABCDEF0123456789ABCDEF01").unwrap(),
            "0xabcdef0123456789abcdef0123456789abcdef01"
        );
    }
}
