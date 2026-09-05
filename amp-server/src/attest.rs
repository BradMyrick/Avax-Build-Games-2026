//! EIP-712 outcome attestations, byte-compatible with `AMPSettlement` on
//! Avalanche. The digest is hand-rolled (matching the relayer's style) so the
//! encoding is explicit and version-independent:
//!
//! domain  = EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)
//!         with name="AMPSettlement", version="1"
//! struct = AsyncResult(uint256 matchId,uint8 outcome,bytes32 transcriptHash)
//!
//! OutcomeCode (mirrors AMPTypes.sol): NONE=0, WIN_A=1, WIN_B=2, DRAW=3,
//! CANCELLED=4.

use alloy_primitives::{Address, B256, keccak256};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;

pub const EIP712_DOMAIN_TYPEHASH: &[u8] =
    b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)";
pub const ASYNC_RESULT_TYPEHASH: &[u8] =
    b"AsyncResult(uint256 matchId,uint8 outcome,bytes32 transcriptHash)";
pub const SETTLEMENT_NAME: &str = "AMPSettlement";
pub const SETTLEMENT_VERSION: &str = "1";

pub fn outcome_code(outcome: &str) -> Option<u8> {
    match outcome {
        "win_a" => Some(1),
        "win_b" => Some(2),
        "draw" => Some(3),
        "cancelled" => Some(4),
        _ => None,
    }
}

/// keccak256(abi.encode(typeHash, matchId, outcome, transcriptHash))
pub fn async_result_struct_hash(
    on_chain_match_id: u64,
    outcome: u8,
    transcript_hash: B256,
) -> B256 {
    let mut buf = Vec::with_capacity(32 * 4);
    buf.extend_from_slice(keccak256(ASYNC_RESULT_TYPEHASH).as_slice());
    buf.extend_from_slice(&word_u64(on_chain_match_id)[..]);
    buf.extend_from_slice(&word_u8(outcome)[..]);
    buf.extend_from_slice(transcript_hash.as_slice());
    keccak256(&buf)
}

pub fn domain_separator(chain_id: u64, settlement: Address) -> B256 {
    let mut buf = Vec::with_capacity(32 * 5);
    buf.extend_from_slice(keccak256(EIP712_DOMAIN_TYPEHASH).as_slice());
    buf.extend_from_slice(keccak256(SETTLEMENT_NAME.as_bytes()).as_slice());
    buf.extend_from_slice(keccak256(SETTLEMENT_VERSION.as_bytes()).as_slice());
    buf.extend_from_slice(&word_u64(chain_id)[..]);
    buf.extend_from_slice(&word_address(settlement)[..]);
    keccak256(&buf)
}

/// A uint value as a right-aligned 32-byte EVM word.
fn word_u64(v: u64) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&v.to_be_bytes());
    w
}

/// A uint8 enum value as a right-aligned 32-byte EVM word.
fn word_u8(v: u8) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[31] = v;
    w
}

/// An address as a left-padded 32-byte EVM word.
fn word_address(a: Address) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(a.as_slice());
    w
}

pub fn eip712_digest(
    chain_id: u64,
    settlement: Address,
    on_chain_match_id: u64,
    outcome: u8,
    transcript_hash: B256,
) -> B256 {
    let struct_hash = async_result_struct_hash(on_chain_match_id, outcome, transcript_hash);
    let mut buf = Vec::with_capacity(66);
    buf.extend_from_slice(&[0x19, 0x01]);
    buf.extend_from_slice(domain_separator(chain_id, settlement).as_slice());
    buf.extend_from_slice(struct_hash.as_slice());
    keccak256(&buf)
}

/// Sign the AsyncResult attestation with the verifier key. Returns (digest,
/// 65-byte signature with EIP-712 v semantics used by OZ ECDSA.recover).
pub fn sign_attestation(
    signer: &PrivateKeySigner,
    chain_id: u64,
    settlement: Address,
    on_chain_match_id: u64,
    outcome: u8,
    transcript_hash: B256,
) -> anyhow::Result<(B256, Vec<u8>)> {
    let digest = eip712_digest(
        chain_id,
        settlement,
        on_chain_match_id,
        outcome,
        transcript_hash,
    );
    let sig = signer.sign_hash_sync(&digest)?;
    let mut out = sig.as_bytes().to_vec();
    // Solidity ECDSA.recover expects v in {27, 28}.
    if out[64] < 27 {
        out[64] += 27;
    }
    Ok((digest, out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::Signature;
    use alloy_signer_local::PrivateKeySigner;

    #[test]
    fn outcome_codes_match_contract_enum() {
        assert_eq!(outcome_code("win_a"), Some(1));
        assert_eq!(outcome_code("win_b"), Some(2));
        assert_eq!(outcome_code("draw"), Some(3));
        assert_eq!(outcome_code("cancelled"), Some(4));
        assert_eq!(outcome_code("nope"), None);
    }

    #[test]
    fn signature_recovers_to_signer_over_hand_rolled_digest() {
        let signer = PrivateKeySigner::random();
        let settlement: Address = "0x000000000000000000000000000000000000dEaD"
            .parse()
            .unwrap();
        let transcript = keccak256(b"test transcript");
        let (digest, sig) =
            sign_attestation(&signer, 43113, settlement, 42, 2, transcript).unwrap();

        let arr: [u8; 65] = sig.as_slice().try_into().unwrap();
        let recovered = Signature::from_raw_array(&arr)
            .unwrap()
            .recover_address_from_prehash(&digest)
            .unwrap();
        assert_eq!(recovered, signer.address());
        // v must be Ethereum-style for OZ ECDSA.recover.
        assert!(sig[64] == 27 || sig[64] == 28);
    }

    #[test]
    fn digest_is_deterministic_and_input_sensitive() {
        let settlement: Address = "0x000000000000000000000000000000000000dEaD"
            .parse()
            .unwrap();
        let th = keccak256(b"t");
        let a = eip712_digest(43113, settlement, 1, 1, th);
        let b = eip712_digest(43113, settlement, 1, 1, th);
        let c = eip712_digest(43113, settlement, 1, 2, th);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
