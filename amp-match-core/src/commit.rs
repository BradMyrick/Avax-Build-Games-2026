//! Anti-collusion primitives: blinded ticket commitments and the
//! deterministic lobby shuffle.
//!
//! **Ticket commitment** (§1.3 of the v2 spec):
//! `H = keccak256(address ‖ stake ‖ salt)` — a staked FFA queue entry is
//! invisible to coordinators until the reveal phase.
//!
//! **Lobby shuffle:** when a bucket holds more eligible candidates than the
//! lobby needs, assignment is a Fisher-Yates shuffle seeded by the latest
//! Avalanche block hash — unpredictable before the fact, verifiable after.
//! Pre-coordinated groups cannot deliberately land in the same lobby
//! because no one can predict the blockhash the shuffle will use.
//!
//! This module is the reason the crate grew its second dependency
//! (`tiny-keccak`, audited, no_std): commitments must be keccak256 to match
//! the EVM. Still zero server / RPC / async machinery.

use tiny_keccak::{Hasher, Keccak};

/// keccak256 of the concatenated bytes (the EVM's hash).
pub fn keccak256(bytes: &[u8]) -> [u8; 32] {
    let mut k = Keccak::v256();
    k.update(bytes);
    let mut out = [0u8; 32];
    k.finalize(&mut out);
    out
}

/// Blinded queue-entry commitment. `salt` must be 32 random bytes known
/// only to the committer until reveal.
pub fn ticket_commit(address: &[u8; 20], stake_wei: u64, salt: &[u8; 32]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(60);
    buf.extend_from_slice(address);
    buf.extend_from_slice(&stake_wei.to_be_bytes());
    buf.extend_from_slice(salt);
    keccak256(&buf)
}

/// Deterministic, unbiased shuffle seeded by a blockhash.
///
/// Keystream: `block_i = keccak256(blockhash ‖ u64_be(i))`, consumed as
/// u64 words. Bounded indices use Lemire's widening-multiply method with
/// rejection — no modulo bias.
pub fn shuffle_by_blockhash<T>(mut items: Vec<T>, blockhash: &[u8; 32]) -> Vec<T> {
    if items.len() < 2 {
        return items;
    }
    let mut rng = BlockhashRng::new(blockhash);
    let mut i = items.len() - 1;
    while i > 0 {
        let j = rng.below(i as u64 + 1) as usize;
        items.swap(i, j);
        i -= 1;
    }
    items
}

struct BlockhashRng<'a> {
    blockhash: &'a [u8; 32],
    block: [u8; 32],
    counter: u64,
    words: [u64; 4],
    pos: usize,
}

impl<'a> BlockhashRng<'a> {
    fn new(blockhash: &'a [u8; 32]) -> Self {
        Self {
            blockhash,
            block: [0u8; 32],
            counter: 0,
            words: [0u64; 4],
            pos: 4,
        }
    }

    fn refill(&mut self) {
        let mut buf = [0u8; 40];
        buf[..32].copy_from_slice(self.blockhash);
        buf[32..].copy_from_slice(&self.counter.to_be_bytes());
        self.block = keccak256(&buf);
        for (c, w) in (0..32usize).step_by(8).zip(self.words.iter_mut()) {
            let mut b = [0u8; 8];
            b.copy_from_slice(&self.block[c..c + 8]);
            *w = u64::from_be_bytes(b);
        }
        self.pos = 0;
        self.counter += 1;
    }

    fn next_u64(&mut self) -> u64 {
        if self.pos >= self.words.len() {
            self.refill();
        }
        let w = self.words[self.pos];
        self.pos += 1;
        w
    }

    /// Unbiased `x < bound` (Lemire 2019: widening multiply, reject when the
    /// low limb falls in the partial-tail zone; threshold = 2⁶⁴ mod bound).
    fn below(&mut self, bound: u64) -> u64 {
        if bound < 2 {
            return 0;
        }
        let threshold = bound.wrapping_neg() % bound; // == 2^64 mod bound
        loop {
            let x = self.next_u64();
            let m = (x as u128) * (bound as u128);
            let lo = m as u64;
            if lo < threshold {
                continue; // rare rejection keeps the distribution flat
            }
            return (m >> 64) as u64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keccak_matches_known_vectors() {
        // Standard keccak-256 (EVM) vectors — guards against any tiny-keccak
        // feature/config mistake.
        let empty = keccak256(b"");
        assert_eq!(
            hex(&empty),
            "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"
        );
        let abc = keccak256(b"abc");
        assert_eq!(
            hex(&abc),
            "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45"
        );
    }

    #[test]
    fn commitment_binds_all_inputs() {
        let a = [1u8; 20];
        let salt = [7u8; 32];
        let h = ticket_commit(&a, 1000, &salt);
        assert_ne!(ticket_commit(&[2u8; 20], 1000, &salt), h, "address bound");
        assert_ne!(ticket_commit(&a, 1001, &salt), h, "stake bound");
        assert_ne!(ticket_commit(&a, 1000, &[8u8; 32]), h, "salt bound");
        assert_eq!(ticket_commit(&a, 1000, &salt), h, "deterministic");
    }

    #[test]
    fn shuffle_is_deterministic_per_blockhash() {
        let items: Vec<u32> = (0..64).collect();
        let bh = [42u8; 32];
        let a = shuffle_by_blockhash(items.clone(), &bh);
        let b = shuffle_by_blockhash(items.clone(), &bh);
        assert_eq!(a, b, "same blockhash ⇒ same permutation");
        let other = shuffle_by_blockhash(items, &[43u8; 32]);
        assert_ne!(a, other, "different blockhash ⇒ different permutation");
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let items: Vec<u32> = (0..256).collect();
        let mut out = shuffle_by_blockhash(items, &[9u8; 32]);
        out.sort_unstable();
        let identity: Vec<u32> = (0..256).collect();
        assert_eq!(out, identity, "no element lost or duplicated");
    }

    #[test]
    fn shuffle_moves_the_first_element() {
        let items: Vec<u32> = (0..32).collect();
        let out = shuffle_by_blockhash(items, &[1u8; 32]);
        assert_ne!(out[0], 0, "identity layout is (2^-31 · 31) unlikely");
    }

    #[test]
    fn trivial_shuffles_pass_through() {
        let empty: Vec<u32> = vec![];
        assert!(shuffle_by_blockhash(empty, &[0u8; 32]).is_empty());
        assert_eq!(shuffle_by_blockhash(vec![1], &[0u8; 32]), vec![1]);
    }

    #[test]
    fn below_is_within_bounds_and_spread() {
        let mut rng = BlockhashRng::new(&[5u8; 32]);
        let mut hits = [0usize; 7];
        for _ in 0..7000 {
            let x = rng.below(7) as usize;
            assert!(x < 7);
            hits[x] += 1;
        }
        // Every bucket in a 7000/7 = 1000 ± 20% band — enough to catch
        // catastrophic bias without being flaky.
        for (i, h) in hits.iter().enumerate() {
            assert!(*h > 800 && *h < 1200, "bucket {i} hit {h} times — biased");
        }
        assert_eq!(rng.below(1), 0);
        assert_eq!(rng.below(0), 0);
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
}
