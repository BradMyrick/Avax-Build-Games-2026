-- AMP matchmaking state: players, auth sessions, ratings, queue tickets,
-- live matches, and outcome reports. The matchmaker's hot loop runs in
-- memory; these tables are the durable source of truth across restarts.

CREATE TABLE IF NOT EXISTS amp_players (
    wallet       TEXT PRIMARY KEY,                -- lowercase 0x address
    region       TEXT NOT NULL DEFAULT 'na',
    language     TEXT NOT NULL DEFAULT 'en',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS amp_auth_challenges (
    nonce      TEXT PRIMARY KEY,
    wallet     TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    used       BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE INDEX IF NOT EXISTS amp_auth_challenges_wallet_idx
    ON amp_auth_challenges (wallet, expires_at);

CREATE TABLE IF NOT EXISTS amp_sessions (
    token_hash TEXT PRIMARY KEY,                  -- sha256 hex of the bearer token
    wallet     TEXT NOT NULL REFERENCES amp_players(wallet),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    revoked    BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE INDEX IF NOT EXISTS amp_sessions_wallet_idx ON amp_sessions (wallet);

CREATE TABLE IF NOT EXISTS amp_ratings (
    wallet           TEXT NOT NULL REFERENCES amp_players(wallet),
    game_id          TEXT NOT NULL,
    ruleset_id       TEXT NOT NULL,
    rating           DOUBLE PRECISION NOT NULL DEFAULT 1500,
    rating_deviation DOUBLE PRECISION NOT NULL DEFAULT 350,
    volatility       DOUBLE PRECISION NOT NULL DEFAULT 0.06,
    wins             BIGINT NOT NULL DEFAULT 0,
    losses           BIGINT NOT NULL DEFAULT 0,
    draws            BIGINT NOT NULL DEFAULT 0,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (wallet, game_id, ruleset_id)
);

CREATE TABLE IF NOT EXISTS amp_queue_tickets (
    id         UUID PRIMARY KEY,
    wallet     TEXT NOT NULL REFERENCES amp_players(wallet),
    game_id    TEXT NOT NULL,
    ruleset_id TEXT NOT NULL,
    stake_wei  BIGINT NOT NULL DEFAULT 0,
    region     TEXT NOT NULL DEFAULT 'na',
    status     TEXT NOT NULL DEFAULT 'queued',   -- queued | matched | cancelled
    match_id   UUID,
    joined_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS amp_queue_tickets_status_idx
    ON amp_queue_tickets (status, game_id, ruleset_id);

CREATE TABLE IF NOT EXISTS amp_matches (
    id                UUID PRIMARY KEY,
    game_id           TEXT NOT NULL,
    ruleset_id        TEXT NOT NULL,
    stake_wei         BIGINT NOT NULL DEFAULT 0,
    state             TEXT NOT NULL DEFAULT 'live',  -- live | agreed | settling | settled | disputed | cancelled | expired
    player_a          TEXT NOT NULL REFERENCES amp_players(wallet),
    player_b          TEXT NOT NULL REFERENCES amp_players(wallet),
    rating_a_snapshot JSONB NOT NULL,               -- {rating, deviation, volatility}
    rating_b_snapshot JSONB NOT NULL,
    winner            TEXT,                         -- wallet, or NULL on draw/cancel
    outcome           TEXT,                         -- win_a | win_b | draw | cancelled
    attestation       JSONB,                        -- EIP-712 skill record + settlement sig
    on_chain_match_id BIGINT,                       -- AMPRegistry match id when staked
    connect_info      JSONB,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at        TIMESTAMPTZ NOT NULL,
    settled_at        TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS amp_matches_state_idx ON amp_matches (state, expires_at);
CREATE INDEX IF NOT EXISTS amp_matches_player_idx ON amp_matches (player_a, player_b);

CREATE TABLE IF NOT EXISTS amp_match_reports (
    match_id       UUID NOT NULL REFERENCES amp_matches(id),
    wallet         TEXT NOT NULL,
    result         TEXT NOT NULL,                  -- reporter-relative: win | loss | draw
    transcript_hash TEXT,
    submitted_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (match_id, wallet)
);
