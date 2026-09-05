-- Cold-start + oracle-free attestation additions.

-- Bot-fill matches: created against the house opponent when a player waits
-- past the configured threshold. Bot matches settle instantly from the
-- player's single report and never touch ratings, attestations, or stakes.
ALTER TABLE amp_matches
    ADD COLUMN IF NOT EXISTS bot_match BOOLEAN NOT NULL DEFAULT false;

-- Player-signed outcome reports (EIP-191 over
-- "AMP_REPORT:v1:{matchId}:{result}"). Non-repudiable evidence usable for
-- on-chain RT_HASH_AGREE settlement without any operator signature.
-- Required for staked matches.
ALTER TABLE amp_match_reports
    ADD COLUMN IF NOT EXISTS signature TEXT;
