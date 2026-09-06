-- M3: N-player party sessions, commit-reveal queueing, multiplayer matches,
-- ladder reports with quorum tracking, and death certificates.

-- Parties: a leader invites members; the composition is leader-signed.
CREATE TABLE IF NOT EXISTS amp_parties (
    id          UUID PRIMARY KEY,
    leader      TEXT NOT NULL REFERENCES amp_players(wallet),
    members     JSONB NOT NULL,           -- [{wallet, region, acceptedAt}]
    game_id     TEXT NOT NULL,
    ruleset_id  TEXT NOT NULL,
    state       TEXT NOT NULL DEFAULT 'open',  -- open | locked | disbanded
    invite_code TEXT UNIQUE NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    locked_at   TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS amp_parties_leader_idx ON amp_parties (leader, state);
CREATE INDEX IF NOT EXISTS amp_parties_invite_idx ON amp_parties (invite_code) WHERE state = 'open';

-- Commit-reveal phase for staked FFA queues.
CREATE TABLE IF NOT EXISTS amp_commits (
    wallet      TEXT NOT NULL REFERENCES amp_players(wallet),
    game_id     TEXT NOT NULL,
    ruleset_id  TEXT NOT NULL,
    commit_hash TEXT NOT NULL,            -- keccak256(addr ‖ stake ‖ salt)
    stake_wei   BIGINT NOT NULL,
    state       TEXT NOT NULL DEFAULT 'committed',  -- committed | revealed | expired
    salt        TEXT,                     -- set at reveal
    revealed_at TIMESTAMPTZ,
    blockhash   TEXT,                     -- set at lobby formation
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (wallet, game_id, ruleset_id)
);

-- N-player matches (parallel to the existing amp_matches for 1v1).
CREATE TABLE IF NOT EXISTS amp_multi_matches (
    id                UUID PRIMARY KEY,
    game_id           TEXT NOT NULL,
    ruleset_id        TEXT NOT NULL,
    lobby_size        INT NOT NULL,
    payout_profile_id SMALLINT NOT NULL DEFAULT 1,
    stake_per_player  BIGINT NOT NULL,
    bond_per_player   BIGINT NOT NULL,
    on_chain_match_id BIGINT,             -- AMPMultiplayer lobby id (uint256)
    state             TEXT NOT NULL DEFAULT 'committing',
                      -- committing | revealing | escrow | live | quorum | grace | settled | disputed | cancelled
    players           JSONB NOT NULL,     -- [{wallet, index, rating, rd, region, party_id?}]
    signer_mask       BIGINT NOT NULL DEFAULT 0,
    ladder            JSONB,              -- [{wallet, rank}] — filled at settlement
    transcript_hash   TEXT,
    session_nonce     BIGINT,
    ladder_hash       TEXT,               -- keccak of ranked placements + transcript
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    ready_at          TIMESTAMPTZ,
    quorum_until      TIMESTAMPTZ,
    grace_until       TIMESTAMPTZ,
    settled_at        TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS amp_multi_state_idx ON amp_multi_matches (state, created_at);

-- Individual ladder reports (EIP-712 MultiplayerLadder signatures).
CREATE TABLE IF NOT EXISTS amp_ladder_reports (
    match_id       UUID NOT NULL REFERENCES amp_multi_matches(id),
    wallet         TEXT NOT NULL,
    ranked         JSONB NOT NULL,        -- [{wallet, rank}]
    transcript_hash TEXT NOT NULL,
    session_nonce  BIGINT NOT NULL,
    signature      TEXT NOT NULL,          -- 65-byte hex
    reported_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (match_id, wallet)
);

-- Death certificates: eliminated players sign and disconnect.
CREATE TABLE IF NOT EXISTS amp_exit_certs (
    match_id       UUID NOT NULL REFERENCES amp_multi_matches(id),
    wallet         TEXT NOT NULL,
    rank           INT NOT NULL,
    exit_frame     BIGINT NOT NULL,
    state_hash     TEXT NOT NULL,
    signature      TEXT NOT NULL,
    countersigned_by JSONB,               -- [wallet] survivors who verified
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (match_id, wallet)
);
