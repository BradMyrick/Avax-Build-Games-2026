//! EIP-712 match intents — the gasless stake commitment a player signs when
//! joining a staked queue. The intent binds (player, game, ruleset, stake,
//! deadline) cryptographically before any funds move: the matchmaker only
//! pairs intents it has verified, and the deadline kills replays.
//!
//! domain  = AMPMatchIntent / 1 / chainId / verifyingContract = settlement
//! struct  = MatchIntent(address player,string gameId,string rulesetId,uint256 stakeWei,uint256 deadline)
//!
//! Byte-layout mirrors the hand-rolled encodings in attest.rs and the
//! Solidity/ethers conventions (strings hash to keccak of their bytes).

use alloy_primitives::{Address, B256, keccak256};

pub const INTENT_DOMAIN_NAME: &str = "AMPMatchIntent";
pub const INTENT_DOMAIN_VERSION: &str = "1";
pub const INTENT_TYPEHASH: &[u8] =
    b"MatchIntent(address player,string gameId,string rulesetId,uint256 stakeWei,uint256 deadline)";

pub struct MatchIntent<'a> {
    pub player: Address,
    pub game_id: &'a str,
    pub ruleset_id: &'a str,
    pub stake_wei: u64,
    /// Unix seconds; the server rejects intents past this moment.
    pub deadline: u64,
}

fn word_u64(v: u64) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&v.to_be_bytes());
    w
}

fn word_address(a: Address) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(a.as_slice());
    w
}

pub fn intent_digest(chain_id: u64, settlement: Address, intent: &MatchIntent) -> B256 {
    // domain separator
    let mut d = Vec::with_capacity(32 * 5);
    d.extend_from_slice(
        keccak256(
            b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
        )
        .as_slice(),
    );
    d.extend_from_slice(keccak256(INTENT_DOMAIN_NAME.as_bytes()).as_slice());
    d.extend_from_slice(keccak256(INTENT_DOMAIN_VERSION.as_bytes()).as_slice());
    d.extend_from_slice(&word_u64(chain_id));
    d.extend_from_slice(&word_address(settlement));
    let domain_sep = keccak256(&d);

    // struct hash: typehash ‖ addr ‖ keccak(gameId) ‖ keccak(rulesetId) ‖ stake ‖ deadline
    let mut sh = Vec::with_capacity(32 * 6);
    sh.extend_from_slice(keccak256(INTENT_TYPEHASH).as_slice());
    sh.extend_from_slice(&word_address(intent.player));
    sh.extend_from_slice(keccak256(intent.game_id.as_bytes()).as_slice());
    sh.extend_from_slice(keccak256(intent.ruleset_id.as_bytes()).as_slice());
    sh.extend_from_slice(&word_u64(intent.stake_wei));
    sh.extend_from_slice(&word_u64(intent.deadline));
    let struct_hash = keccak256(&sh);

    let mut out = Vec::with_capacity(66);
    out.extend_from_slice(&[0x19, 0x01]);
    out.extend_from_slice(domain_sep.as_slice());
    out.extend_from_slice(struct_hash.as_slice());
    keccak256(&out)
}

/// Recover the signing wallet from an EIP-712 intent signature (65-byte hex).
pub fn recover_intent_signer(
    chain_id: u64,
    settlement: Address,
    intent: &MatchIntent,
    signature_hex: &str,
) -> anyhow::Result<Address> {
    let digest = intent_digest(chain_id, settlement, intent);
    let sig_bytes = hex::decode(signature_hex.trim_start_matches("0x"))?;
    if sig_bytes.len() != 65 {
        anyhow::bail!("intent signature must be 65 bytes, got {}", sig_bytes.len());
    }
    let arr: [u8; 65] = sig_bytes.as_slice().try_into().unwrap();
    let sig = alloy_primitives::Signature::from_raw_array(&arr)
        .map_err(|e| anyhow::anyhow!("invalid signature: {e}"))?;
    sig.recover_address_from_prehash(&digest)
        .map_err(|e| anyhow::anyhow!("recovery failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_signer::SignerSync;
    use alloy_signer_local::PrivateKeySigner;

    #[test]
    fn intent_round_trip_recovers_signer() {
        let signer = PrivateKeySigner::random();
        let settlement: Address = "0x000000000000000000000000000000000000dEaD"
            .parse()
            .unwrap();
        let intent = MatchIntent {
            player: signer.address(),
            game_id: "amp-tactics",
            ruleset_id: "ranked-1v1",
            stake_wei: 1_000_000_000_000,
            deadline: 1_800_000_000,
        };
        let digest = intent_digest(43113, settlement, &intent);
        let sig = signer.sign_hash_sync(&digest).unwrap();
        let mut bytes = sig.as_bytes().to_vec();
        if bytes[64] < 27 {
            bytes[64] += 27;
        }
        let recovered =
            recover_intent_signer(43113, settlement, &intent, &hex::encode(bytes)).unwrap();
        assert_eq!(recovered, signer.address());
    }

    #[test]
    fn intent_is_input_sensitive() {
        let settlement: Address = "0x000000000000000000000000000000000000dEaD"
            .parse()
            .unwrap();
        let base = MatchIntent {
            player: Address::ZERO,
            game_id: "g",
            ruleset_id: "r",
            stake_wei: 1,
            deadline: 100,
        };
        let other = MatchIntent {
            player: Address::ZERO,
            game_id: "g",
            ruleset_id: "r",
            stake_wei: 2,
            deadline: 100,
        };
        let d1 = intent_digest(1, settlement, &base);
        let d2 = intent_digest(2, settlement, &base); // different chain
        let d3 = intent_digest(1, settlement, &other);
        assert_ne!(d1, d2);
        assert_ne!(d1, d3);
        assert_eq!(d1, intent_digest(1, settlement, &base));
    }

    #[test]
    fn bad_signature_length_rejected() {
        let settlement: Address = "0x000000000000000000000000000000000000dEaD"
            .parse()
            .unwrap();
        let intent = MatchIntent {
            player: Address::ZERO,
            game_id: "g",
            ruleset_id: "r",
            stake_wei: 1,
            deadline: 100,
        };
        assert!(recover_intent_signer(1, settlement, &intent, "00").is_err());
        // Garbage-but-well-formed signatures recover to an UNRELATED address
        // (alloy accepts v>=35 as EIP-155 style) — the wallet equality check
        // is the security gate, so assert it never yields the intent player.
        if let Ok(recovered) = recover_intent_signer(1, settlement, &intent, &"ab".repeat(65)) {
            assert_ne!(recovered, intent.player);
        }
    }
}
