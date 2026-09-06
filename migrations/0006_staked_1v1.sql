-- 1v1 baseline: staked intents, escrow lifecycle, RT settlement routing.

-- Signed EIP-712 match intents captured at staked queue-join time.
ALTER TABLE amp_queue_tickets
    ADD COLUMN IF NOT EXISTS intent_deadline TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS intent_sig TEXT;

ALTER TABLE amp_matches
    -- when agreement was reached (drives the RT fallback clock)
    ADD COLUMN IF NOT EXISTS agreed_at TIMESTAMPTZ,
    -- deadline for direct player settlement before the relayer takes over
    ADD COLUMN IF NOT EXISTS settle_deadline TIMESTAMPTZ,
    -- registry game id the escrow match was created under
    ADD COLUMN IF NOT EXISTS escrow_game_id BIGINT;

CREATE INDEX IF NOT EXISTS amp_matches_settle_sweep_idx
    ON amp_matches (state, settle_deadline);
