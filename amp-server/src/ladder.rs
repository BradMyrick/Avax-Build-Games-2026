//! EIP-712 MultiplayerLadder digest — mirrors `AMPMultiplayer.sol`'s
//! typehash byte-for-byte. Every player signs this over their claimed
//! final placement array; a K-of-N concordant quorum settles the match.
//!
//! domain  = AMPMultiplayer / 1 / chainId / verifyingContract
//! struct  = MultiplayerLadder(bytes32 matchId, bytes32 gameId,
//!                              address[] rankedPlacements, bytes32 transcriptHash,
//!                              uint256 sessionNonce)

use alloy_primitives::{Address, B256, keccak256};

pub const LADDER_DOMAIN_NAME: &str = "AMPMultiplayer";
pub const LADDER_DOMAIN_VERSION: &str = "1";
pub const LADDER_TYPEHASH: &[u8] = b"MultiplayerLadder(bytes32 matchId,bytes32 gameId,address[] rankedPlacements,bytes32 transcriptHash,uint256 sessionNonce)";

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

/// keccak256 of the concatenated 32-byte-padded addresses (EIP-712 array
/// encoding).
fn address_array_root(addresses: &[Address]) -> B256 {
    let mut buf = Vec::with_capacity(addresses.len() * 32);
    for a in addresses {
        buf.extend_from_slice(&word_address(*a));
    }
    keccak256(&buf)
}

pub fn ladder_domain_separator(chain_id: u64, contract: Address) -> B256 {
    let mut d = Vec::with_capacity(32 * 5);
    d.extend_from_slice(
        keccak256(
            b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
        )
        .as_slice(),
    );
    d.extend_from_slice(keccak256(LADDER_DOMAIN_NAME.as_bytes()).as_slice());
    d.extend_from_slice(keccak256(LADDER_DOMAIN_VERSION.as_bytes()).as_slice());
    d.extend_from_slice(&word_u64(chain_id));
    d.extend_from_slice(&word_address(contract));
    keccak256(&d)
}

/// Full EIP-712 digest for a MultiplayerLadder attestation.
pub fn ladder_digest(
    chain_id: u64,
    contract: Address,
    on_chain_match_id: u64,
    game_id: u64,
    ranked: &[Address],
    transcript_hash: B256,
    session_nonce: u64,
) -> B256 {
    let domain_sep = ladder_domain_separator(chain_id, contract);
    let ranked_root = address_array_root(ranked);

    let mut sh = Vec::with_capacity(32 * 5);
    sh.extend_from_slice(keccak256(LADDER_TYPEHASH).as_slice());
    sh.extend_from_slice(&word_u64(on_chain_match_id));
    sh.extend_from_slice(&word_u64(game_id));
    sh.extend_from_slice(ranked_root.as_slice());
    sh.extend_from_slice(transcript_hash.as_slice());
    sh.extend_from_slice(&word_u64(session_nonce));
    let struct_hash = keccak256(&sh);

    let mut out = Vec::with_capacity(66);
    out.extend_from_slice(&[0x19, 0x01]);
    out.extend_from_slice(domain_sep.as_slice());
    out.extend_from_slice(struct_hash.as_slice());
    keccak256(&out)
}

/// Recover the signing wallet from a 65-byte ladder signature.
#[allow(clippy::too_many_arguments)]
pub fn recover_ladder_signer(
    chain_id: u64,
    contract: Address,
    on_chain_match_id: u64,
    game_id: u64,
    ranked: &[Address],
    transcript_hash: B256,
    session_nonce: u64,
    signature_hex: &str,
) -> anyhow::Result<Address> {
    let digest = ladder_digest(
        chain_id,
        contract,
        on_chain_match_id,
        game_id,
        ranked,
        transcript_hash,
        session_nonce,
    );
    let sig_bytes = hex::decode(signature_hex.trim_start_matches("0x"))?;
    if sig_bytes.len() != 65 {
        anyhow::bail!("ladder signature must be 65 bytes, got {}", sig_bytes.len());
    }
    let arr: [u8; 65] = sig_bytes.as_slice().try_into().unwrap();
    let sig = alloy_primitives::Signature::from_raw_array(&arr)
        .map_err(|e| anyhow::anyhow!("invalid signature: {e}"))?;
    sig.recover_address_from_prehash(&digest)
        .map_err(|e| anyhow::anyhow!("recovery failed: {e}"))
}

/// keccak of the ranked placements + transcript hash — the on-chain
/// `settledLadderHash` comparison value.
#[allow(dead_code)] // used by the settlement pipeline once M3.5 lands
pub fn ladder_hash(ranked: &[Address], transcript_hash: B256) -> B256 {
    let mut buf = Vec::with_capacity(32 + ranked.len() * 32);
    for a in ranked {
        buf.extend_from_slice(&word_address(*a));
    }
    buf.extend_from_slice(transcript_hash.as_slice());
    keccak256(&buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_signer::SignerSync;
    use alloy_signer_local::PrivateKeySigner;

    fn contract() -> Address {
        "0xcabf7b626172fE55d54f03c346563671AbcC77f7"
            .parse()
            .unwrap()
    }

    #[test]
    fn ladder_round_trip() {
        let signer = PrivateKeySigner::random();
        let ranked = [
            "0x0000000000000000000000000000000000000001"
                .parse()
                .unwrap(),
            "0x0000000000000000000000000000000000000002"
                .parse()
                .unwrap(),
        ];
        let th = keccak256(b"test transcript");
        let digest = ladder_digest(43113, contract(), 42, 7, &ranked, th, 99);
        let sig = signer.sign_hash_sync(&digest).unwrap();
        let mut bytes = sig.as_bytes().to_vec();
        if bytes[64] < 27 {
            bytes[64] += 27;
        }
        let recovered = recover_ladder_signer(
            43113,
            contract(),
            42,
            7,
            &ranked,
            th,
            99,
            &hex::encode(bytes),
        )
        .unwrap();
        assert_eq!(recovered, signer.address());
    }

    #[test]
    fn ladder_digest_is_input_sensitive() {
        let ranked: Vec<Address> = (0..4)
            .map(|i| Address::from_word(B256::from(u256_padding(i))))
            .collect();
        let th = keccak256(b"t");
        let a = ladder_digest(1, contract(), 1, 1, &ranked, th, 1);
        let b = ladder_digest(2, contract(), 1, 1, &ranked, th, 1);
        let c = ladder_digest(1, contract(), 1, 1, &ranked, keccak256(b"x"), 1);
        assert_ne!(a, b);
        assert_ne!(a, c);
    }

    fn u256_padding(v: u64) -> [u8; 32] {
        let mut w = [0u8; 32];
        w[24..].copy_from_slice(&v.to_be_bytes());
        w
    }
}
