# AMP v2 — N-Player Multiplayer: Design Record

Status: **in execution** (branch `n-player-multiplayer`). This document
records the approved plan, the resolved decisions, and every deliberate
deviation from the source requirements spec.

## Resolved decisions

| Decision | Choice | Rationale |
|---|---|---|
| Rating storage (§5.1) | **Off-chain + EIP-712 attestations** | Live on-chain (R, RD, σ) costs ~3 storage writes/player/match and turns every update into a tx. The sovereign record is the signed ladder attestation; a merkle-root anchor remains available later if demand appears. |
| Contract shape | **New `AMPMultiplayer.sol`** | Self-contained dual-deposit escrow + quorum settlement + dispute machine. v1 1v1 contracts stay live untouched (tournaments + 1v1 keep working during the build). |
| Commit-reveal (§1.3) | **Full commit-reveal for staked FFA** | Server-private queues + blockhash shuffle are necessary but not sufficient; blinding kills coordinator lobby-targeting outright. Free queues skip it (no stake, nothing to target). |
| Dispute verification (§4.2) | **Bonded operator verifier for v2** | Same trust model as the operator-first matchmaker, now with skin in the game. WASM input-replay is a v3 milestone. |

## Governing invariants

1. **Quorum intersection** (Lamport 1982, signed messages): with
   `N = 3f + 1` participants, `K = floor(2N/3) + 1 = 2f + 1`. Two
   conflicting quorums overlap in ≥ `f + 1` signers, at least one honest —
   equivocation is detectable without identifying who is honest. For
   `N ≤ 3`, `K = N` (unanimity; no Byzantine tolerance possible).
2. **Conservation**: `Σ deposits ≡ Σ payouts + Σ fees + Σ refunds + Σ slashed`
   to 1 wei in every terminal state. Fuzz-gated in forge.
3. **Pull-over-push**: settlement only increments `claimable[]`; zero
   external calls during resolution.
4. **Checks-effects-interactions** on every function touching an external
   address.
5. **Hot/cold split** (unchanged from v1): matchmaking is in-memory state,
   settlement is on-chain state; commit-reveal and quorum live at the
   boundary.

## Deliberate deviations from the spec text

1. **§5.2 party recalibration formula — ratio inverted.** The spec writes
   `ΔR_member = ΔR_party × (R_party / R_member)^γ`, which *amplifies*
   below-average members' gains — the opposite of its stated anti-boost
   intent. Implementation uses `(R_member / R_party)^γ`:
   lower-rated members gain *less* on a shared win (losses pass through
   unscaled — a smurf must not dodge losses either).
2. **§2.2 payout profile units.** `tierBps` sums to 10000 of the **net**
   pool (after protocol rake). The spec comment's "sum = 10000 minus
   protocolRakeBps" composes badly with the existing fee router; rake-first
   then tiers-of-net is the clean split.
3. **§3.3 death certificates are off-chain evidence.** Exit attestations
   shape the final ladder and bond accounting; the chain sees only the
   final `settleMultiplayer` call. Per-exit transactions would defeat the
   §3.4 gas bound.
4. **§6 gas gate scope.** The `< 250k` bound is asserted at N=8 per the
   spec. BR-64 (K = 43 signatures) gets a measured, documented bound in the
   same test — signature verification grows linearly, and the gate must
   follow reality.
5. **§1.1 AdjustedAverage.** Implemented as `mean + λ·σ_spread` (population
   standard deviation, λ clamped [0, 2], default 0.5) rather than a spread
   multiplier — the stdev form is what lobby systems actually price.

## Milestones

- **M1 — core library** (done): party λ·σ aggregation, ping-matrix region
  resolution (120ms gate), `try_match_teams` (exact-size packing + rating
  window), `drain_battle_royale` (wait-priority, decayed windows),
  `placement_vectors` (tie-aware), `recalibrate_party_deltas` (γ),
  `commit.rs` (keccak256 commitments, blockhash Fisher-Yates with Lemire
  debiasing). Fuzz gates: 100k party graphs; permutation bit-equality
  (which caught and forced the canonicalized-summation fix in
  `glicko2_update_vs_many` — float addition is not associative; the
  accumulation order is now sorted by input bit patterns).
- **M2 — `AMPMultiplayer.sol` (done, deployed + sourcify-verified to Fuji
  as 0xcabf7b626172fE55d54f03c346563671AbcC77f7, manifest
  `contracts/deployment-fuji-v2.json`)**: dual-deposit escrow, payout profiles,
  `settleMultiplayer` (bitmask + packed sigs, early popcount < K revert),
  quorum/grace/challenge state machine, bonded-verifier verdicts, slashing
  (non-signer bonds 50/50 relayer/rank-1; challenge losses 70/30
  valid-group/treasury). Gates all green: N=8 settlement at 152,962 gas
  (< 250k), DoS-with-reverting-receiver isolated, conservation fuzz (256
  runs × 128k calls) exact, terminal-drain equality. Larger sizes
  documented: N=16 → 195,844; BR-64 → 404,021.
  
  Two architecture notes forced by the gates: (1) **prove-your-payout** —
  settlement only records the ladder hash + packed economics snapshot and
  credits the three fee recipients; every player claims by resubmitting the
  ladder (hash-verified) and paying their own claim gas, keeping
  settlement's storage writes constant in N. Payout profiles are therefore
  IMMUTABLE (claims recompute tiers). (2) the first dispute-payout draft
  double-spent non-loser stakes (refunds + ladder prizes) — the
  conservation fuzz caught it; the corrected model funds ladder prizes
  from stakes, refunds only bonds to non-losers, and routes loser
  stake-value through the valid ladder's tiers.
- **M3 — amp-server**: party sessions (leader-signed composition),
  commit-reveal queue phases, blockhash-seeded shuffle (one RPC fetch per
  formation), N-way quorum collector (120s window), death-certificate
  intake, `settle_multi` relayer jobs, rating pipeline + γ recalibration.
- **M4 — web**: party UI, lobby/escrow UX, quorum screen, exit-sign
  prompts, multi-tier claims.
- **M5 — hardening**: load (64-player BR formation), chaos (mass
  disconnect mid-quorum, malicious ladders), soak, release.

## Dependency note

`amp-match-core` grew its second dependency — `tiny-keccak` (audited,
no_std, keccak feature only) — because v2 ticket commitments must be
keccak256 to match the EVM. The no-server/no-RPC/no-async contract is
unchanged.
