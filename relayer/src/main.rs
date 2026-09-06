//! AMP custodial relayer — the only process that holds the funded key.
//!
//! Polls `relayer_jobs` in Postgres, signs + submits `createTournament` and
//! `finalizeTournament` on Fuji, writes back the result. The web app never
//! touches the key; it only enqueues jobs.

use std::env;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use ethers::contract::abigen;
use ethers::core::k256::{FieldBytes, ecdsa::SigningKey};
use ethers::core::types::{Address, U256};
use ethers::middleware::SignerMiddleware;
use ethers::providers::{Http, Middleware, Provider};
use ethers::signers::{LocalWallet, Signer};
use ethers::utils::keccak256;
use serde::Deserialize;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use std::sync::Arc;
use tracing::{error, info};

abigen!(
    AMPTournamentCup,
    r#"[
        function createTournament(uint16[] payoutBps, address verifier, uint64 finalizeDeadline) payable returns (uint256)
        function finalizeTournament(uint256 tournamentId, address[] winners, bytes signature)
        function nextTournamentId() view returns (uint256)
    ]"#,
);

abigen!(
    AMPSettlement,
    r#"[
        {
            "type": "function",
            "name": "submitAsyncResult",
            "inputs": [
                { "name": "matchId", "type": "uint256", "internalType": "uint256" },
                {
                    "name": "result",
                    "type": "tuple",
                    "internalType": "struct AMPTypes.AsyncResult",
                    "components": [
                        { "name": "matchId", "type": "uint256", "internalType": "uint256" },
                        { "name": "outcome", "type": "uint8", "internalType": "uint8" },
                        { "name": "transcriptHash", "type": "bytes32", "internalType": "bytes32" },
                        { "name": "signature", "type": "bytes", "internalType": "bytes" }
                    ]
                }
            ],
            "outputs": [],
            "stateMutability": "nonpayable"
        }
    ]"#,
);

mod bracket;

mod config;

use config::Config;

#[derive(Debug, Deserialize)]
struct Job {
    id: i64,
    kind: String,
    payload: Value,
}

#[derive(Debug, Deserialize)]
struct FundPayload {
    #[serde(rename = "payoutBps")]
    payout_bps: Vec<u16>,
    #[serde(rename = "winnerWallets", default)]
    winner_wallets: Vec<String>,
    #[serde(rename = "fundedAvax")]
    funded_avax: f64,
    #[serde(default)]
    mode: Option<String>,
    #[serde(rename = "finalizeDays", default = "default_finalize_days")]
    finalize_days: i64,
    #[serde(rename = "manageToken", default)]
    manage_token: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    players: Option<Value>,
}

fn default_finalize_days() -> i64 {
    7
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load relayer/.env for local dev (no-op in prod where env is injected).
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cfg = Config::from_env()?;
    info!(
        chain_id = cfg.chain_id,
        rpc = %cfg.rpc_url,
        cup = %cfg.cup_address,
        settlement = %cfg.settlement_address,
        "AMP relayer configuration"
    );

    let db_url = env::var("DATABASE_URL").context("DATABASE_URL not set")?;
    let key_str = env::var("AMP_RELAYER_KEY").context("AMP_RELAYER_KEY not set")?;
    let wallet: LocalWallet = key_str
        .parse::<LocalWallet>()
        .context("invalid AMP_RELAYER_KEY")?
        .with_chain_id(cfg.chain_id);
    let relayer_addr = wallet.address();
    info!(?relayer_addr, "AMP relayer started");

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&db_url)
        .await
        .context("connecting to Postgres")?;

    let provider = Arc::new(SignerMiddleware::new(
        Provider::<Http>::try_from(cfg.rpc_url.as_str())?,
        wallet.clone(),
    ));
    let balance = provider.get_balance(relayer_addr, None).await?;
    info!(?balance, "relayer balance");

    let cfg = Arc::new(cfg);
    loop {
        match poll_once(&pool, &provider, &key_str, &cfg).await {
            Ok(true) => {}
            Ok(false) => tokio::time::sleep(Duration::from_millis(cfg.poll_idle_ms)).await,
            Err(e) => {
                error!(error = %e, "poll error; backing off");
                tokio::time::sleep(Duration::from_millis(cfg.poll_error_ms)).await;
            }
        }
    }
}

type SignerProvider = SignerMiddleware<Provider<Http>, LocalWallet>;

async fn poll_once(
    pool: &PgPool,
    provider: &Arc<SignerProvider>,
    key_str: &str,
    cfg: &Arc<Config>,
) -> Result<bool> {
    let row = sqlx::query(
        r#"SELECT id, kind, payload::text as payload FROM relayer_jobs
           WHERE status = 'pending'
           ORDER BY id ASC
           FOR UPDATE SKIP LOCKED
           LIMIT 1"#,
    )
    .fetch_optional(pool)
    .await?;

    let Some(rec) = row else { return Ok(false) };
    let job = Job {
        id: rec.get("id"),
        kind: rec.get("kind"),
        payload: serde_json::from_str(
            &rec.get::<Option<String>, _>("payload").unwrap_or_default(),
        )?,
    };

    info!(job_id = job.id, kind = %job.kind, "processing job");

    let result = match job.kind.as_str() {
        "fund" => fund_job(provider, &job, key_str, pool, cfg).await,
        "finalize" => finalize_job(provider, &job, key_str, pool, cfg).await,
        "settle_match" => settle_match_job(provider, &job, pool, cfg).await,
        "settle_multi" => settle_multi_job(provider, &job, pool, cfg).await,
        other => Err(anyhow!("unknown job kind: {other}")),
    };

    match result {
        Ok((tournament_id, tx_hash)) => {
            let tx_hash = tx_hash.unwrap_or_default();
            sqlx::query(
                r#"UPDATE relayer_jobs
                   SET status = 'done', tournament_id = $2, tx_hash = $3, completed_at = now()
                   WHERE id = $1"#,
            )
            .bind(job.id)
            .bind(tournament_id)
            .bind(tx_hash.as_str())
            .execute(pool)
            .await?;
            info!(job_id = job.id, ?tournament_id, %tx_hash, "job done");
        }
        Err(e) => {
            let msg = format!("{e:#}");
            error!(job_id = job.id, error = %msg, "job failed");
            sqlx::query(
                r#"UPDATE relayer_jobs
                   SET status = 'failed', error = $2, completed_at = now()
                   WHERE id = $1"#,
            )
            .bind(job.id)
            .bind(msg.as_str())
            .execute(pool)
            .await?;
        }
    }
    Ok(true)
}

async fn fund_job(
    provider: &Arc<SignerProvider>,
    job: &Job,
    key_str: &str,
    pool: &PgPool,
    cfg: &Arc<Config>,
) -> Result<(Option<i64>, Option<String>)> {
    let p: FundPayload = serde_json::from_value(job.payload.clone())?;

    let cup: Address = cfg.cup_address.parse()?;
    let contract = AMPTournamentCup::new(cup, Arc::clone(provider));

    let deadline = U256::from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs()
            + (p.finalize_days * 86400) as u64,
    );
    let value = ethers::utils::parse_ether(p.funded_avax.to_string())?;
    let verifier = provider.signer().address();

    let create_call = contract
        .create_tournament(p.payout_bps.clone(), verifier, deadline.as_u64())
        .value(value)
        .gas(500_000);
    let pending = create_call.send().await.context("send createTournament")?;
    let receipt = pending.await?.context("createTournament reverted")?;
    let create_tx_hash = format!("{:?}", receipt.transaction_hash);

    let next = contract.next_tournament_id().call().await?;
    let tournament_id = next - U256::one();
    let tid_i64 = tournament_id.as_u64() as i64;

    // Instant mode: finalize immediately with sponsor-provided winners.
    if p.mode.as_deref() == Some("instant") && !p.winner_wallets.is_empty() {
        let winners = parse_addresses(&p.winner_wallets)?;
        let sig = finalize_signature(
            key_str,
            cfg.chain_id,
            &cfg.cup_address,
            tournament_id.as_u64(),
            &winners,
        )?;
        let fin_call = contract
            .finalize_tournament(tournament_id, winners, sig.into())
            .gas(400_000);
        let receipt = fin_call.send().await?.await?.context("finalize reverted")?;
        return Ok((
            Some(tid_i64),
            Some(format!("{:?}", receipt.transaction_hash)),
        ));
    }

    // Bracket mode: provision DB rows (P0-5) — the relayer is the authority.
    if p.mode.as_deref() == Some("bracket") {
        let sponsor_hex = format!("{:?}", verifier);
        let prize_wei = value.to_string();
        let payout_json = serde_json::to_string(&p.payout_bps)?;
        let paypal_id = job
            .payload
            .get("paypalOrderId")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        sqlx::query(
            r#"INSERT INTO tournaments (tournament_id, sponsor, prize_pool_wei, token, payout_bps, winner_wallets, state, mode, manage_token, paypal_order_id, tx_hash, created_at)
               VALUES ($1,$2,$3,$4,$5::jsonb,$6::jsonb,$7,$8,$9,$10,$11,now())
               ON CONFLICT (tournament_id) DO UPDATE SET tx_hash = EXCLUDED.tx_hash"#,
        )
        .bind(tid_i64)
        .bind(&sponsor_hex)
        .bind(&prize_wei)
        .bind("0x0000000000000000000000000000000000000000")
        .bind(&payout_json)
        .bind("[]")
        .bind("OPEN")
        .bind("bracket")
        .bind(p.manage_token.as_deref())
        .bind(paypal_id)
        .bind(&create_tx_hash)
        .execute(pool)
        .await?;

        if let Some(players) = &p.players {
            let bracket_state = serde_json::json!({
                "format": p.format.as_deref().unwrap_or("single_elimination"),
                "players": players,
                "results": [],
            });
            sqlx::query(
                r#"INSERT INTO brackets (tournament_id, state, updated_at) VALUES ($1, $2::jsonb, now())
                   ON CONFLICT (tournament_id) DO UPDATE SET state = EXCLUDED.state"#,
            )
            .bind(tid_i64)
            .bind(bracket_state.to_string())
            .execute(pool)
            .await?;
        }
    }

    Ok((Some(tid_i64), Some(create_tx_hash)))
}

async fn finalize_job(
    provider: &Arc<SignerProvider>,
    job: &Job,
    key_str: &str,
    pool: &PgPool,
    cfg: &Arc<Config>,
) -> Result<(Option<i64>, Option<String>)> {
    // P0-1: the job carries ONLY { tournamentId }. The relayer re-derives the
    // winner order from the durable bracket rows and cross-checks against the
    // web's computedWinners — a payout address never rides the job payload,
    // and the web's derivation is never trusted blindly.
    #[derive(Deserialize)]
    struct FinPayload {
        #[serde(rename = "tournamentId")]
        tournament_id: i64,
    }
    let p: FinPayload = serde_json::from_value(job.payload.clone())?;
    let tid = p.tournament_id;

    // Guards: tournament exists, is OPEN, and payout structure is known.
    let trow = sqlx::query(
        "SELECT state, payout_bps::text AS payout FROM tournaments WHERE tournament_id = $1",
    )
    .bind(tid)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow!("tournament {tid} not found"))?;
    let t_state: String = trow.get("state");
    let payout_json: String = trow.get("payout");
    if t_state != "OPEN" {
        return Err(anyhow!(
            "tournament {tid} is {t_state}, not OPEN (already finalized?)"
        ));
    }
    let payout_bps: Vec<u16> = serde_json::from_str(&payout_json)
        .with_context(|| format!("bad payout_bps for tournament {tid}"))?;
    if payout_bps.is_empty() {
        return Err(anyhow!("tournament {tid} has no placements"));
    }

    let row = sqlx::query("SELECT state::text FROM brackets WHERE tournament_id = $1")
        .bind(tid)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow!("bracket not found for tournament {tid}"))?;
    let state_text: String = row.get("state");
    let state: Value = serde_json::from_str(&state_text)?;

    // Only single-elim is ported to the relayer; other formats must not
    // finalize through this path until their derivation is ported too.
    let format = state["format"].as_str().unwrap_or("single_elimination");
    if format != "single_elimination" {
        return Err(anyhow!(
            "format {format} winner derivation not ported to relayer; refusing"
        ));
    }

    let players: Vec<bracket::PlayerRow> = serde_json::from_value(state["players"].clone())
        .with_context(|| format!("bad players on bracket for {tid}"))?;
    let results: Vec<bracket::ResultRow> = serde_json::from_value(state["results"].clone())
        .with_context(|| format!("bad results on bracket for {tid}"))?;

    let mut winners = bracket::derive_single_elim_winners(&players, &results)
        .with_context(|| format!("winner derivation failed for tournament {tid}"))?;
    if winners.len() < payout_bps.len() {
        return Err(anyhow!(
            "derived {} winners but tournament pays {} placements",
            winners.len(),
            payout_bps.len()
        ));
    }
    winners.truncate(payout_bps.len());

    // Parity cross-check against the web's derivation (defense against a
    // port bug on either side): mismatch → refuse to sign.
    if let Some(web_winners) = state["computedWinners"].as_array() {
        let web: Vec<String> = web_winners
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        if !web.is_empty() && web != winners {
            return Err(anyhow!(
                "derived winners disagree with web computedWinners for {tid}: refusing to sign"
            ));
        }
    }

    let addrs = parse_addresses(&winners)?;
    let tid_u256 = U256::from(tid);
    let cup: Address = cfg.cup_address.parse()?;
    let contract = AMPTournamentCup::new(cup, Arc::clone(provider));
    let sig = finalize_signature(key_str, cfg.chain_id, &cfg.cup_address, tid as u64, &addrs)?;
    let fin_call = contract
        .finalize_tournament(tid_u256, addrs, sig.into())
        .gas(400_000);
    let receipt = fin_call.send().await?.await?.context("finalize reverted")?;
    Ok((Some(tid), Some(format!("{:?}", receipt.transaction_hash))))
}

/// Settle a staked match: the amp-server (verifier) has already EIP-712-signed
/// the AsyncResult; the relayer's job is pure submission — recover the tx,
/// submit, confirm, and flip the match row to `settled`.
/// Settle an N-player multiplayer match: the amp-server has already
/// collected and verified a K-of-N concordant quorum of EIP-712 ladder
/// signatures; the relayer's job is pure submission to AMPMultiplayer.
async fn settle_multi_job(
    provider: &Arc<SignerProvider>,
    job: &Job,
    pool: &PgPool,
    cfg: &Arc<Config>,
) -> Result<(Option<i64>, Option<String>)> {
    #[derive(Deserialize)]
    struct SettleMultiPayload {
        #[serde(rename = "matchUuid")]
        match_uuid: String,
        #[serde(rename = "onChainMatchId")]
        on_chain_match_id: i64,
        #[serde(rename = "rankedPlacements")]
        ranked_placements: Vec<String>,
        #[serde(rename = "transcriptHash")]
        transcript_hash: String,
        #[serde(rename = "sessionNonce")]
        session_nonce: u64,
        #[serde(rename = "signerBitmask")]
        signer_bitmask: String,
        #[serde(rename = "packedSignatures")]
        packed_signatures: String,
    }
    let p: SettleMultiPayload = serde_json::from_value(job.payload.clone())?;

    let contract_addr: Address = cfg
        .multiplayer_address
        .as_deref()
        .and_then(|a| a.parse().ok())
        .ok_or_else(|| anyhow!("AMP_MULTIPLAYER_ADDRESS not configured"))?;

    // Parse the ranked placements.
    let ranked: Vec<Address> = p
        .ranked_placements
        .iter()
        .map(|s| s.parse::<Address>())
        .collect::<Result<Vec<_>, _>>()
        .context("bad ranked placements")?;

    // Parse the signer bitmask (hex string like "0x3f").
    let signer_mask = u64::from_str_radix(p.signer_bitmask.trim_start_matches("0x"), 16)
        .context("bad signer bitmask")?;

    // Parse packed signatures.
    let sig_bytes = hex::decode(p.packed_signatures.trim_start_matches("0x"))
        .context("bad packed signatures")?;
    if sig_bytes.len() % 65 != 0 {
        return Err(anyhow!(
            "packed signatures length {} not multiple of 65",
            sig_bytes.len()
        ));
    }

    let tx_hash = submit_multiplayer_settlement(
        provider,
        &contract_addr,
        p.on_chain_match_id as u64,
        &ranked,
        &p.transcript_hash,
        p.session_nonce,
        signer_mask,
        &sig_bytes,
    )
    .await?;

    // Update the server's match row.
    sqlx::query(
        "UPDATE amp_multi_matches SET state = 'settled', settled_at = now() WHERE id = $1::uuid",
    )
    .bind(&p.match_uuid)
    .execute(pool)
    .await?;

    Ok((Some(p.on_chain_match_id), Some(tx_hash)))
}

#[allow(clippy::too_many_arguments)]
async fn submit_multiplayer_settlement(
    _provider: &Arc<SignerProvider>,
    _contract_addr: &Address,
    _on_chain_match_id: u64,
    _ranked: &[Address],
    _transcript_hash: &str,
    _session_nonce: u64,
    _signer_mask: u64,
    _sig_bytes: &[u8],
) -> Result<String> {
    // The actual on-chain call to AMPMultiplayer.settleMultiplayer.
    // For now, log the intent and return a placeholder — the full
    // alloy-based submission lands with the AMPMultiplayer abigen in the
    // relayer once the contract is wired to the same deployment as the
    // server's config.
    tracing::info!(
        match_id = _on_chain_match_id,
        signers = _signer_mask.count_ones(),
        ranked_count = _ranked.len(),
        sig_count = _sig_bytes.len() / 65,
        "settle_multi: ready for on-chain submission (abigen wiring pending)"
    );
    Err(anyhow!(
        "settle_multi on-chain submission pending AMPMultiplayer abigen wiring — contract deployed but relayer binding not yet generated"
    ))
}

async fn settle_match_job(
    provider: &Arc<SignerProvider>,
    job: &Job,
    pool: &PgPool,
    cfg: &Arc<Config>,
) -> Result<(Option<i64>, Option<String>)> {
    #[derive(Deserialize)]
    struct SettlePayload {
        #[serde(rename = "matchUuid")]
        match_uuid: String,
        #[serde(rename = "onChainMatchId")]
        on_chain_match_id: i64,
        #[serde(rename = "outcomeCode")]
        outcome_code: u8,
        #[serde(rename = "transcriptHash")]
        transcript_hash: String,
        signature: String,
    }
    let p: SettlePayload = serde_json::from_value(job.payload.clone())?;

    let sig_bytes = hex::decode(p.signature.trim_start_matches("0x")).with_context(|| {
        format!(
            "bad signature hex ({} bytes expected 65)",
            p.signature.len()
        )
    })?;
    if sig_bytes.len() != 65 {
        return Err(anyhow!(
            "signature must be 65 bytes, got {}",
            sig_bytes.len()
        ));
    }
    let th_hex = p.transcript_hash.trim_start_matches("0x");
    let th_bytes = hex::decode(th_hex).context("bad transcriptHash hex")?;
    let mut transcript: [u8; 32] = [0u8; 32];
    if th_bytes.len() == 32 {
        transcript.copy_from_slice(&th_bytes);
    } else if !th_bytes.is_empty() {
        return Err(anyhow!(
            "transcriptHash must be 32 bytes, got {}",
            th_bytes.len()
        ));
    }

    let settlement: Address = cfg.settlement_address.parse()?;
    let contract = AMPSettlement::new(settlement, Arc::clone(provider));

    let result = AsyncResult {
        match_id: U256::from(p.on_chain_match_id as u64),
        outcome: p.outcome_code,
        transcript_hash: transcript,
        signature: sig_bytes.into(),
    };
    let match_id_u256 = U256::from(p.on_chain_match_id as u64);

    let call = contract
        .submit_async_result(match_id_u256, result)
        .gas(300_000);
    let receipt = call
        .send()
        .await?
        .await?
        .context("submitAsyncResult reverted")?;
    let tx_hash = format!("{:?}", receipt.transaction_hash);

    // Flip the server's match row to settled so players see it immediately.
    sqlx::query("UPDATE amp_matches SET state = 'settled', settled_at = now() WHERE id = $1::uuid")
        .bind(&p.match_uuid)
        .execute(pool)
        .await?;

    Ok((Some(p.on_chain_match_id), Some(tx_hash)))
}

/// Hand-rolled EIP-712 TournamentResult signature, byte-identical to the contract
/// and the browser (ethers signTypedData). Kept explicit so the digest encoding
/// is unambiguous and version-independent.
fn finalize_signature(
    key_str: &str,
    chain_id: u64,
    cup_address_hex: &str,
    tournament_id: u64,
    winners: &[Address],
) -> Result<Vec<u8>> {
    let digest = eip712_finalize_digest(chain_id, cup_address_hex, tournament_id, winners);
    let secret_hex = key_str.trim_start_matches("0x");
    let secret = hex::decode(secret_hex).context("bad key hex")?;
    let arr: [u8; 32] = secret
        .as_slice()
        .try_into()
        .map_err(|_| anyhow!("relayer key must be 32 bytes"))?;
    let fb: FieldBytes = arr.into();
    let signing_key = SigningKey::from_bytes(&fb).context("invalid key bytes")?;
    let (sig, rid) = signing_key
        .sign_prehash_recoverable(&digest)
        .context("sign prehash")?;
    let r = sig.r().to_bytes();
    let s = sig.s().to_bytes();
    let mut out = Vec::with_capacity(65);
    out.extend_from_slice(&r);
    out.extend_from_slice(&s);
    out.push(27 + rid.to_byte());
    Ok(out)
}

/// EIP-712 digest for TournamentResult(uint256 tournamentId, address[] winners).
fn eip712_finalize_digest(
    chain_id: u64,
    cup_address_hex: &str,
    tournament_id: u64,
    winners: &[Address],
) -> [u8; 32] {
    let cup: Address = cup_address_hex.parse().unwrap();

    // Domain separator
    let mut bs = Vec::new();
    bs.extend_from_slice(&keccak256(
        b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
    ));
    bs.extend_from_slice(&keccak256(b"AMPTournamentCup"));
    bs.extend_from_slice(&keccak256(b"1"));
    bs.extend_from_slice(&u256_bytes(U256::from(chain_id)));
    bs.extend_from_slice(&addr_bytes(&cup));
    let domain_sep = keccak256(&bs);

    let type_hash = keccak256(b"TournamentResult(uint256 tournamentId,address[] winners)");

    // winners root: keccak of concatenated 32-byte-padded addresses (EIP-712 array encoding)
    let mut w = Vec::new();
    for a in winners {
        w.extend_from_slice(&addr_bytes(a));
    }
    let winners_root = keccak256(&w);

    // struct hash
    let mut sh = Vec::new();
    sh.extend_from_slice(&type_hash);
    sh.extend_from_slice(&u256_bytes(U256::from(tournament_id)));
    sh.extend_from_slice(&winners_root);
    let struct_hash = keccak256(&sh);

    let mut d = vec![0x19, 0x01];
    d.extend_from_slice(&domain_sep);
    d.extend_from_slice(&struct_hash);
    keccak256(&d)
}

fn u256_bytes(v: U256) -> [u8; 32] {
    let mut buf = [0u8; 32];
    v.to_big_endian(&mut buf);
    buf
}

fn addr_bytes(a: &Address) -> [u8; 32] {
    let mut buf = [0u8; 32];
    buf[12..].copy_from_slice(a.as_bytes());
    buf
}

fn parse_addresses(strs: &[String]) -> Result<Vec<Address>> {
    strs.iter()
        .map(|s| {
            s.parse::<Address>()
                .with_context(|| format!("bad address: {s}"))
        })
        .collect()
}
