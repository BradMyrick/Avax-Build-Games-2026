"use client";

/**
 * Client SDK for amp-server — the matchmaker.
 *
 * Login is one EIP-191 signature (no gas, no transaction). The session token
 * is stored locally and used for the REST API and the WebSocket.
 */

const SERVER_URL =
  process.env.NEXT_PUBLIC_AMP_SERVER_URL || "http://localhost:8080";

export const AMP_SERVER_URL = SERVER_URL;

/**
 * True when the page is served from a public host but the SDK still points
 * at localhost — i.e. NEXT_PUBLIC_AMP_SERVER_URL was not set (or not set
 * before the build that shipped). NEXT_PUBLIC_* vars are INLINED AT BUILD
 * TIME, so adding one in Vercel requires a redeploy.
 */
export function matchmakerMisconfigured(): boolean {
  if (typeof window === "undefined") return false;
  const site = window.location.hostname;
  const localSite =
    site === "localhost" || site === "127.0.0.1" || site.endsWith(".local");
  const localServer =
    SERVER_URL.includes("://localhost") || SERVER_URL.includes("://127.0.0.1");
  return !localSite && localServer;
}

const TOKEN_KEY = "amp_session_token";
const WALLET_KEY = "amp_session_wallet";

export interface Session {
  token: string;
  wallet: string;
  expiresAt: string;
}

export interface PlayerRating {
  gameId: string;
  rulesetId: string;
  rating: number;
  deviation: number;
  wins: number;
  losses: number;
  draws: number;
}

export interface QueueEvent {
  type: string;
  data: Record<string, unknown> & { wait?: never };
}

class ApiError extends Error {
  constructor(
    public code: string,
    message: string,
    public status: number,
  ) {
    super(message);
  }
}

export function storedSession(): Session | null {
  if (typeof window === "undefined") return null;
  const token = localStorage.getItem(TOKEN_KEY);
  const wallet = localStorage.getItem(WALLET_KEY);
  if (!token || !wallet) return null;
  return { token, wallet, expiresAt: "" };
}

export function clearSession() {
  localStorage.removeItem(TOKEN_KEY);
  localStorage.removeItem(WALLET_KEY);
}

async function api<T>(
  path: string,
  opts: { method?: string; body?: unknown; auth?: boolean } = {},
): Promise<T> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (opts.auth !== false) {
    const s = storedSession();
    if (s) headers.Authorization = `Bearer ${s.token}`;
  }
  let res: Response;
  try {
    res = await fetch(`${SERVER_URL}${path}`, {
      method: opts.method || "GET",
      headers,
      body: opts.body !== undefined ? JSON.stringify(opts.body) : undefined,
    });
  } catch {
    console.error(
      `[AMP] Matchmaker unreachable at ${SERVER_URL}.` +
        (matchmakerMisconfigured()
          ? " Build is missing NEXT_PUBLIC_AMP_SERVER_URL (set it and rebuild)."
          : ""),
    );
    throw new ApiError(
      "network",
      "Matchmaking is temporarily unavailable. Please try again shortly.",
      0,
    );
  }
  const json = await res.json().catch(() => ({}));
  if (!res.ok) {
    throw new ApiError(
      json.error || "unknown",
      json.message || res.statusText,
      res.status,
    );
  }
  return json as T;
}

/** Wallet login: challenge → personal_sign → verify → session token. */
export async function loginWithWallet(): Promise<Session> {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const eth = (window as any).ethereum;
  if (typeof window === "undefined" || !eth) {
    throw new Error("No wallet found — install MetaMask or use a wallet browser");
  }
  const accounts: string[] = await eth.request({ method: "eth_requestAccounts" });
  const wallet = accounts[0];

  const { challenge } = await api<{ challenge: string }>(
    "/v1/auth/challenge",
    { method: "POST", body: { wallet }, auth: false },
  );
  const signature: string = await eth.request({
    method: "personal_sign",
    params: [toUtf8Hex(challenge), wallet],
  });
  const verified = await api<{ token: string; expiresAt: string }>(
    "/v1/auth/verify",
    { method: "POST", body: { wallet, signature, challenge }, auth: false },
  );

  localStorage.setItem(TOKEN_KEY, verified.token);
  localStorage.setItem(WALLET_KEY, wallet.toLowerCase());
  return { token: verified.token, wallet, expiresAt: verified.expiresAt };
}

function toUtf8Hex(s: string): string {
  const bytes = new TextEncoder().encode(s);
  return (
    "0x" +
    Array.from(bytes)
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("")
  );
}

export interface GameInfo {
  id: string;
  name: string;
  rulesets: { id: string; name: string; queueDepth: number }[];
  nextQueueWindowUtc?: string;
}

export function fetchGames() {
  return api<{ games: GameInfo[] }>("/v1/games", { auth: false });
}

export function fetchMe() {
  return api<{
    wallet: string;
    ratings: PlayerRating[];
    liveMatchId: string | null;
    queueTicket: { ticketId: string; gameId: string; rulesetId: string } | null;
  }>("/v1/me");
}

export function joinQueue(gameId: string, rulesetId: string) {
  return api<{
    ticketId: string;
    alreadyQueued: boolean;
    queueDepth: number;
    waitedMs: number;
    skillWindow: number;
    rating?: number;
  }>("/v1/queue/join", { method: "POST", body: { gameId, rulesetId } });
}

export function leaveQueue() {
  return api<{ left: boolean }>("/v1/queue/leave", { method: "POST" });
}

export function fetchMatch(matchId: string) {
  return api<Record<string, unknown>>(`/v1/matches/${matchId}`);
}

/**
 * Report your match result. The report is signed with your wallet
 * (EIP-191 over "AMP_REPORT:v1:{matchId}:{result}") whenever a wallet is
 * available — non-repudiable evidence that makes staked settlement
 * possible without trusting the operator. Required for staked matches.
 */
export async function reportOutcome(
  matchId: string,
  result: "win" | "loss" | "draw",
  transcriptHash?: string,
): Promise<{ matchId: string; state: string; note?: string; bot?: boolean }> {
  const body: Record<string, unknown> = { result };
  if (transcriptHash) body.transcriptHash = transcriptHash;

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const eth = typeof window !== "undefined" ? (window as any).ethereum : null;
  const s = storedSession();
  if (eth && s) {
    try {
      const message = `AMP_REPORT:v1:${matchId}:${result}`;
      body.signature = await eth.request({
        method: "personal_sign",
        params: [toUtf8Hex(message), s.wallet],
      });
    } catch {
      // User declined the signature — send unsigned (free matches still
      // settle; staked matches will be rejected server-side).
    }
  }
  return api(`/v1/matches/${matchId}/report`, { method: "POST", body });
}

export function fetchHistory(limit = 20) {
  return api<{ matches: Record<string, unknown>[] }>(
    `/v1/matches/history?limit=${limit}`,
  );
}

/** Live events: queue status, match found, results. Reconnects with backoff. */
export function connectWs(
  onEvent: (type: string, data: Record<string, unknown>) => void,
  onOpen?: () => void,
  onClose?: () => void,
): () => void {
  let ws: WebSocket | null = null;
  let closed = false;
  let attempt = 0;

  const open = () => {
    const s = storedSession();
    if (!s || closed) return;
    const url = SERVER_URL.replace(/^http/, "ws") + `/v1/ws?token=${encodeURIComponent(s.token)}`;
    ws = new WebSocket(url);
    ws.onopen = () => {
      attempt = 0;
      onOpen?.();
    };
    ws.onmessage = (ev) => {
      try {
        const msg = JSON.parse(ev.data as string);
        onEvent(msg.type, msg.data || {});
      } catch {
        /* ignore malformed */
      }
    };
    ws.onclose = () => {
      onClose?.();
      if (!closed) {
        attempt += 1;
        setTimeout(open, Math.min(1000 * 2 ** attempt, 15000));
      }
    };
  };
  open();

  return () => {
    closed = true;
    ws?.close();
  };
}

// ---- N-Player Multiplayer API ---------------------------------------------

export interface PartyInfo {
  partyId: string;
  leader: string;
  members: { wallet: string; region: string; acceptedAt: string }[];
  state: string;
  inviteCode: string;
  gameId: string;
  rulesetId: string;
}

export interface MultiMatch {
  matchId: string;
  state: string;
  lobbySize: number;
  stakePerPlayer: number;
  bondPerPlayer: number;
  players: { wallet: string; index: number; rating: number; region: string }[];
  ladder?: [string, number][];
  signerCount: number;
  quorumNeeded: number;
  reportCount: number;
  quorumUntil?: string;
}

export function createParty(gameId: string, rulesetId: string) {
  return api<{ partyId: string; inviteCode: string; leader: string; state: string }>(
    "/v1/parties",
    { method: "POST", body: { game_id: gameId, ruleset_id: rulesetId } },
  );
}

export function joinParty(inviteCode: string, region?: string) {
  return api<{ partyId: string; members: number; state: string }>("/v1/parties/join", {
    method: "POST",
    body: { invite_code: inviteCode, region },
  });
}

export function getParty(partyId: string) {
  return api<PartyInfo>(`/v1/parties/${partyId}`);
}

export function lockParty(partyId: string) {
  return api<{ partyId: string; state: string; lockMessage: string }>(
    `/v1/parties/${partyId}/lock`,
    { method: "POST" },
  );
}

export function disbandParty(partyId: string) {
  return api<{ disbanded: boolean }>(`/v1/parties/${partyId}/disband`, { method: "POST" });
}

export function multiCommit(
  gameId: string,
  commitHash: string,
  stakeWei: number,
  lobbySize: number,
) {
  return api<{ committed: boolean; committedCount: number; ready: boolean }>(
    "/v1/multi/commit",
    { method: "POST", body: { gameId, commitHash, stakeWei, lobbySize } },
  );
}

export function multiReveal(gameId: string, rulesetId: string, salt: string) {
  return api<{ revealed: boolean; revealedCount: number }>("/v1/multi/reveal", {
    method: "POST",
    body: { gameId, rulesetId, salt },
  });
}

export function getMultiMatch(matchId: string) {
  return api<MultiMatch>(`/v1/multi/${matchId}`);
}

export function multiReport(
  matchId: string,
  ranked: [string, number][],
  transcriptHash: string,
  sessionNonce: number,
  signature: string,
) {
  return api<{
    matchId: string;
    state: string;
    concordant?: number;
    quorumNeeded: number;
  }>(`/v1/multi/${matchId}/report`, {
    method: "POST",
    body: { ranked, transcriptHash, sessionNonce, signature },
  });
}

export function multiClaim(matchId: string) {
  return api<{ matchId: string; state: string; message: string }>(
    `/v1/multi/${matchId}/claim`,
    { method: "POST" },
  );
}

/** Compute the commit hash: keccak256(address ‖ stake ‖ salt) — uses
 *  the browser's keccak via ethers. */
export async function computeCommitHash(
  wallet: string,
  stakeWei: number,
  salt: string,
): Promise<string> {
  const { ethers } = await import("ethers");
  const addr = ethers.getBytes(wallet);
  const stake = ethers.toBeHex(BigInt(stakeWei), 8);
  const saltBytes = ethers.toUtf8Bytes(salt);
  const concatenated = ethers.concat([addr, ethers.getBytes(stake), saltBytes]);
  return ethers.keccak256(concatenated);
}

/** Sign a MultiplayerLadder EIP-712 message. */
export async function signLadder(params: {
  wallet: string;
  matchId: string;
  gameId: string;
  ranked: string[];
  transcriptHash: string;
  sessionNonce: number;
  chainId: number;
  contractAddress: string;
}): Promise<string> {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const eth = (window as any).ethereum;
  if (!eth) throw new Error("No wallet found");

  const msgParams = JSON.stringify({
    domain: {
      name: "AMPMultiplayer",
      version: "1",
      chainId: params.chainId,
      verifyingContract: params.contractAddress,
    },
    message: {
      matchId: params.matchId,
      gameId: params.gameId,
      rankedPlacements: params.ranked,
      transcriptHash: params.transcriptHash,
      sessionNonce: params.sessionNonce,
    },
    primaryType: "MultiplayerLadder",
    types: {
      EIP712Domain: [
        { name: "name", type: "string" },
        { name: "version", type: "string" },
        { name: "chainId", type: "uint256" },
        { name: "verifyingContract", type: "address" },
      ],
      MultiplayerLadder: [
        { name: "matchId", type: "bytes32" },
        { name: "gameId", type: "bytes32" },
        { name: "rankedPlacements", type: "address[]" },
        { name: "transcriptHash", type: "bytes32" },
        { name: "sessionNonce", type: "uint256" },
      ],
    },
  });

  return eth.request({
    method: "eth_signTypedData_v4",
    params: [params.wallet, msgParams],
  });
}
