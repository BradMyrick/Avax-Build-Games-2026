# AMP — Avalanche Matchmaking Protocol

### Real ranked matchmaking for multiplayer games, with verifiable settlement on Avalanche.

AMP is a multiplayer matchmaking protocol. Players log in with one wallet
signature (no gas), enter a skill-rated queue, get matched, and report the
result. Every settled match updates a Glicko-2 rating and — when it matters —
is attested with an EIP-712 signature and settled on-chain through escrow
contracts on Avalanche, with the protocol taking a fee in basis points.

**Try it:** [playwithamp.xyz](https://playwithamp.xyz) → *Play Ranked*.

---

## The architecture

The invariant that shapes everything: **matchmaking is hot mutable state,
settlement is cold verifiable state.** The queue runs in memory at a 100ms
tick; the money and attestations live on-chain.

```
web/ (Next.js)                    amp-server (Rust)                    contracts/ (Solidity)
  /arena — queue UI, wallet         HTTP/WS gateway (axum)                AMPRegistry + AMPSettlement
    login, live match view          amp-match-core (pure lib:               ├─ staked matches: escrow,
  /setup /cup /claim —                Glicko-2, rules, queue)                verifier-attested payouts,
    tournaments (kept working)      EIP-191 challenge-response              bps rake, dispute arbitration
        │                           EIP-712 outcome attestation           AMPTournamentCup (untouched)
        ▼                                  │                                └─ sponsor prize cups
  amp-server API / WS                     ▼
        │                          Postgres (Neon) ◀── relayer_jobs ── relayer (Rust, custody)
        ▼                          players, ratings, queue,             drains jobs, submits txs:
  one EIP-191 signature            matches, reports                      fund | finalize | settle_match
```

**Components:**

- **`amp-server/` (Rust)** — the matchmaker. Axum HTTP/WS gateway over an
  in-memory `MatchQueue`; wallet-bound EIP-191 challenge login; Glicko-2
  ratings; expanding skill window (tight matches early, any match
  eventually); two-player outcome reconciliation with dispute + timeout
  defaults; EIP-712 `AsyncResult` attestations.
- **`amp-match-core/` (Rust, zero-dep)** — the embeddable heart: Glicko-2,
  composable rules (skill/region/language/latency), the bucketed queue.
  Studios can embed it without running anything else.
- **`relayer/` (Rust)** — the only process holding a funded key. Drains
  `relayer_jobs` from Postgres and submits on Fuji: tournament
  fund/finalize + match `settle_match`.
- **`contracts/` (Foundry)** — `AMPRegistry` + `AMPSettlement` (staked
  matches, verifier-gated payouts, bps protocol fee, arbiter disputes) and
  `AMPTournamentCup` (sponsor prize pools) — all deployed and verified on
  Fuji, timelock-governed.
- **`web/` (Next.js)** — `/arena` player client (login → queue → match →
  report → rating) plus the tournament product surface (`/setup`, `/manage`,
  `/cup`, `/claim`).
- **`migrations/`** — one Postgres schema for everything.

## The player flow

1. **Login** — connect wallet, sign one challenge. No transaction, no gas.
2. **Queue** — pick a game + ruleset, see live queue depth and your rating.
   Leave anytime.
3. **Match found** — WebSocket push with opponent card (rating, region).
4. **Play, report** — both players submit `win/loss/draw`.
5. **Result** — instant: rating update + (if configured) an EIP-712
   attestation. Staked matches: escrow settles on-chain, winners claim.

Trust rules for reports: both agree → settled. Conflict → disputed,
operator arbitrates. Opponent silent past the deadline → reporter's result
stands. Nobody reports → cancelled, ratings untouched.

## The three hard problems — and what ships for each

**Cold-start liquidity.** Empty lobbies kill peer-to-peer matchmaking, so
AMP ships (1) **practice-bot fill** — wait past the threshold
(`AMP_BOT_AFTER_MS`, default 45s) and the house opponent offers an instant
unrated match, so a solo player is never stranded; (2) **prime-time queue
windows** (`AMP_QUEUE_WINDOWS_JSON`) that concentrate concurrent players
into scheduled blocks; (3) a **free-first funnel** — gasless login, free
ranked play — the widest possible top of the ladder.

**Oracle-free outcome attestation.** Every result report can be **signed by
the player's own wallet** (EIP-191 over `AMP_REPORT:v1:{matchId}:{result}`)
— non-repudiable evidence that makes settlement possible without trusting
the operator. Signed reports are *required* for staked matches. Players may
also submit **transcript hashes**; matching hashes strengthen agreement,
mismatched hashes force a dispute even when claims align. On-chain,
`AMPSettlement` supports `RT_HASH_AGREE` mode where the two player
submissions settle escrow directly — no verifier signature needed — with
verifier-attested `ASYNC_VERIFIER` mode as the high-stakes path.

**Fee vs. value.** A bare rake gets bypassed; AMP's rake (bps, taken only
on staked settlement) funds the ladder players can't self-host: one wallet
= one cross-game rating graph, portable EIP-712 skill attestations,
Glicko-2 matchmaking quality, griefing arbitration, escrowed prize cups,
and an embeddable core library (`amp-match-core`) for studios that want the
algorithms without running the service.

## Run it

### Database (Neon Postgres)
Set `DATABASE_URL` to the **pooled** connection string (the `-pooler`
hostname). Apply migrations: `cd web && npm run db:migrate`.

### amp-server (the matchmaker)
```bash
cd amp-server && cp .env.example .env   # set DATABASE_URL at minimum
cargo run --release                      # :8080 — free-play mode without a verifier key
```
Set `AMP_VERIFIER_KEY` to enable EIP-712 attestations; add
`AMP_SETTLEMENT_ADDRESS` + `AMP_STAKING_ENABLED=1` for staked settlement.

### web
```bash
cd web && npm install
NEXT_PUBLIC_AMP_SERVER_URL=http://localhost:8080 npm run dev
```

### relayer (custody — run separately, never on Vercel)
```bash
cd relayer && cargo run --release
# DATABASE_URL + AMP_RELAYER_KEY (funded Fuji EOA)
```

### contracts
```bash
cd contracts && forge test -vvv
```

## Repo layout

```
├── amp-server/        # Rust matchmaker: HTTP/WS, auth, queue, ratings, attestations
├── amp-match-core/    # embeddable Glicko-2 + rules + queue library (serde-only)
├── relayer/           # Rust custody relayer (job queue → sign → submit)
├── contracts/         # Foundry: Registry/Settlement + TournamentCup (Fuji)
├── web/               # Next.js: /arena player client + tournament product
├── migrations/        # Postgres schema
└── docs/              # docs.page source · docs/legacy/ = pre-pivot design docs
```

## Security model

- **Pull-payments only** on both settlement paths; no push transfers.
- **Relayer key isolation** — the funded key exists only in the relayer env.
- **Verifier attestations** — EIP-712 digests computed identically in
  Solidity, Rust (hand-rolled, test-verified recovery), and the browser.
- **Wallet-bound, single-use challenges**; session tokens stored hashed.
- **Idempotent reports** — first report per player stands (`ON CONFLICT`).
- Contracts: `ReentrancyGuard` + `Pausable` + `Ownable2Step`, value
  conservation fuzz-tested, timelock governance on economic parameters.

### P0 audit remediation — verified closed

The 2026-07 audit spec (`SECURITY_REMEDIATION.md`, preserved in git
history) is fully remediated and was removed from the tree:

| Item | Resolution |
|---|---|
| C1 payout tampering | Relayer jobs carry `{tournamentId}` only; winners independently re-derived in Rust (`relayer/src/bracket.rs`) with a 36-case TS-engine parity corpus, cross-checked against the web's derivation before signing |
| C2 double funding | PayPal orders gated by `funding_intents` idempotency; amounts verified server-side |
| C3 organizer spoofing | Bearer `manage_token` (constant-time compare) on bracket/finalize/init; token required on init upsert |
| H1 report forgery/replay | EIP-191 signature recovery + `report_nonces` single-use on `/report` |
| Abuse limits | Upstash sliding-window rate limits (5/30/60 per min) + 64KB body caps + array-length caps on every write route |
| L2/L3 job leakage | `GET /api/job/[id]` returns status fields only; row provisioning moved into the relayer |

## Ops notes

### Wallet phishing warnings on preview deployments

MetaMask (and PhishFort, its warning-list supplier) has flagged thousands of
abused `*.vercel.app` / `*.netlify.app` preview subdomains. Wallet prompts on
a PR preview URL can therefore show *"Continue at your own risk…"* even
though the site is clean — the warning targets the domain's reputation, not
the code. `playwithamp.xyz` itself is not on any list.

Rules of thumb:
- **Never share wallet-facing flows on preview URLs.** Test sign-in flows on
  the production custom domain only.
- If a specific subdomain got flagged (recycled preview names carry
  scammers' history), request delisting:
  - MetaMask: <https://support.metamask.io> (report a false positive) or
    file an issue on `MetaMask/eth-phishing-detect`.
  - PhishFort: open a PR against `phishfort/phishfort-lists/whitelists`.
- Keep signature prompts human-readable and truthful — the sign-in message
  states plainly that it is free and moves no funds
  (`auth.rs::challenge_message`, site name via `AMP_SITE_NAME`). Opaque
  `PREFIX:uuid` blobs are what drainer reports are made of.

## License

Apache License 2.0. See [LICENSE](LICENSE).
