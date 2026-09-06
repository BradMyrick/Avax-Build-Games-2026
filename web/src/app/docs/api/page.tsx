import Link from "next/link";
import { Code2, Terminal, Webhook, FileJson, Shield, Users } from "lucide-react";

export const metadata = {
  title: "AMP API Reference — REST, WebSocket & EIP-712",
  description: "Complete API reference for the AMP matchmaker: REST endpoints, WebSocket events, EIP-712 typed data, and the embeddable Rust core.",
};

function Endpoint({
  method,
  path,
  auth,
  body,
  response,
  desc,
}: {
  method: string;
  path: string;
  auth?: boolean;
  body?: string;
  response?: string;
  desc: string;
}) {
  const methodColor =
    method === "GET" ? "text-green-400 border-green-400/30 bg-green-400/5" :
    method === "POST" ? "text-brand-cyan border-brand-cyan/30 bg-brand-cyan/5" :
    method === "DELETE" ? "text-red-400 border-red-400/30 bg-red-400/5" :
    "text-yellow-400 border-yellow-400/30 bg-yellow-400/5";

  return (
    <div className="glass-panel rounded-2xl border border-white/10 p-5 mb-4">
      <div className="flex items-center gap-3 mb-2 flex-wrap">
        <span className={`text-xs font-black font-mono px-2.5 py-1 rounded border ${methodColor}`}>
          {method}
        </span>
        <code className="text-sm font-mono text-white">{path}</code>
        {auth && (
          <span className="text-[10px] font-bold uppercase tracking-wider text-yellow-400/70 border border-yellow-400/20 rounded-full px-2 py-0.5">
            Auth
          </span>
        )}
      </div>
      <p className="text-sm text-zinc-400 mb-3">{desc}</p>
      {body && (
        <div className="mb-2">
          <div className="text-[10px] font-black uppercase tracking-widest text-zinc-500 mb-1">
            Body
          </div>
          <pre className="bg-black/60 border border-white/10 rounded-lg px-3 py-2 text-xs font-mono text-zinc-300 overflow-x-auto">
            <code>{body}</code>
          </pre>
        </div>
      )}
      {response && (
        <div>
          <div className="text-[10px] font-black uppercase tracking-widest text-zinc-500 mb-1">
            Response
          </div>
          <pre className="bg-black/60 border border-white/10 rounded-lg px-3 py-2 text-xs font-mono text-zinc-300 overflow-x-auto">
            <code>{response}</code>
          </pre>
        </div>
      )}
    </div>
  );
}

function WsEvent({ type, desc, payload }: { type: string; desc: string; payload: string }) {
  return (
    <div className="glass-panel rounded-2xl border border-white/10 p-5 mb-4">
      <div className="flex items-center gap-3 mb-2">
        <span className="text-xs font-black font-mono px-2.5 py-1 rounded border text-purple-400 border-purple-400/30 bg-purple-400/5">
          WS
        </span>
        <code className="text-sm font-mono text-white">{type}</code>
      </div>
      <p className="text-sm text-zinc-400 mb-3">{desc}</p>
      <pre className="bg-black/60 border border-white/10 rounded-lg px-3 py-2 text-xs font-mono text-zinc-300 overflow-x-auto">
        <code>{payload}</code>
      </pre>
    </div>
  );
}

export default function ApiReferencePage() {
  return (
    <div className="min-h-screen bg-black text-white antialiased">
      <div className="absolute top-0 -left-1/4 w-[150%] h-[300px] bg-brand-cyan/10 blur-[120px] rounded-full pointer-events-none" />

      <div className="relative z-10 max-w-4xl mx-auto px-6 py-16">
        <div className="mb-12">
          <Link href="/docs" className="text-sm text-zinc-500 hover:text-brand-cyan transition-colors">
            ← Docs
          </Link>
          <div className="inline-flex items-center gap-2 rounded-full border border-brand-cyan/30 bg-brand-cyan/5 px-4 py-1.5 text-xs font-bold uppercase tracking-widest text-brand-cyan mt-6 mb-4">
            <Code2 className="w-3.5 h-3.5" />
            API Reference
          </div>
          <h1 className="text-4xl font-black uppercase tracking-tight mb-4">
            Every Endpoint, <span className="text-brand-cyan">Documented</span>
          </h1>
          <p className="text-lg text-zinc-400 leading-relaxed">
            REST for actions, WebSocket for real-time events, EIP-712 for
            signatures. Base URL:{" "}
            <code className="text-brand-cyan text-sm">
              https://amp.playwithamp.xyz
            </code>
          </p>
        </div>

        {/* Auth section */}
        <h2 className="text-xl font-black uppercase tracking-tight mb-4 flex items-center gap-2">
          <Terminal className="w-5 h-5 text-brand-cyan" />
          Authentication
        </h2>
        <p className="text-sm text-zinc-400 mb-6">
          All authenticated endpoints use{" "}
          <code className="text-brand-cyan">Authorization: Bearer {"<token>"}</code>.
          Tokens expire after 7 days. Login is a single free EIP-191 signature.
        </p>

        <Endpoint
          method="POST"
          path="/v1/auth/challenge"
          body={`{ "wallet": "0x..." }`}
          response={`{ "challenge": "Sign in to AMP Arena\\n\\nThis signature is free...", "expiresAt": "..." }`}
          desc="Request a sign-in challenge for a wallet. The challenge is human-readable and expires in 5 minutes."
        />
        <Endpoint
          method="POST"
          path="/v1/auth/verify"
          body={`{ "wallet": "0x...", "signature": "0x...", "challenge": "Sign in to AMP Arena..." }`}
          response={`{ "token": "amp_...", "expiresAt": "...", "player": { "wallet": "0x..." } }`}
          desc="Exchange the EIP-191 signature for a session token. Single-use challenge."
        />

        {/* Queue section */}
        <h2 className="text-xl font-black uppercase tracking-tight mb-4 mt-10 flex items-center gap-2">
          <Users className="w-5 h-5 text-brand-cyan" />
          Queue & Matchmaking
        </h2>
        <Endpoint
          method="GET"
          path="/v1/games"
          response={`{ "games": [{ "id": "amp-tactics", "name": "AMP Tactics", "queueDepth": 12 }], "stakingEnabled": false, "chainId": 43113 }`}
          desc="List available games with live queue depth. No auth required."
        />
        <Endpoint
          method="POST"
          path="/v1/queue/join"
          auth
          body={`{ "gameId": "amp-tactics", "rulesetId": "ranked-1v1" }`}
          response={`{ "ticketId": "...", "queueDepth": 12, "skillWindow": 350, "rating": 1500 }`}
          desc="Join a ranked queue. Idempotent — joining twice returns the existing ticket."
        />
        <Endpoint
          method="POST"
          path="/v1/queue/leave"
          auth
          response={`{ "left": true }`}
          desc="Leave the queue. Always succeeds."
        />
        <Endpoint
          method="GET"
          path="/v1/queue/status"
          auth
          response={`{ "queued": true, "depth": 12, "waitedMs": 45000, "skillWindow": 710 }`}
          desc="Live queue status. The skill window widens the longer you wait."
        />

        {/* Match section */}
        <h2 className="text-xl font-black uppercase tracking-tight mb-4 mt-10 flex items-center gap-2">
          <FileJson className="w-5 h-5 text-brand-cyan" />
          Matches
        </h2>
        <Endpoint
          method="POST"
          path="/v1/matches/{id}/report"
          auth
          body={`{ "result": "win", "transcriptHash": "0x...", "signature": "0x..." }`}
          response={`{ "matchId": "...", "state": "agreed", "outcome": "win_b" }`}
          desc="Report your match result. Both players must agree. EIP-191 signature required for staked matches."
        />
        <Endpoint
          method="GET"
          path="/v1/matches/{id}"
          auth
          response={`{ "matchId": "...", "state": "live", "you": { "ratingSnapshot": {...} }, "opponent": { "wallet": "0x...", "ratingSnapshot": {...} } }`}
          desc="Get match details. Participants only."
        />
        <Endpoint
          method="GET"
          path="/v1/matches/history"
          auth
          response={`{ "matches": [{ "matchId": "...", "outcome": "win_b", "state": "agreed" }] }`}
          desc="Match history for the authenticated player."
        />

        {/* Multiplayer section */}
        <h2 className="text-xl font-black uppercase tracking-tight mb-4 mt-10 flex items-center gap-2">
          <Users className="w-5 h-5 text-brand-cyan" />
          N-Player Multiplayer
        </h2>
        <Endpoint
          method="POST"
          path="/v1/parties"
          auth
          body={`{ "gameId": "amp-tactics", "rulesetId": "ranked-1v1" }`}
          response={`{ "partyId": "...", "inviteCode": "AB3D5F", "leader": "0x..." }`}
          desc="Create a party. Returns a 6-character invite code for members to join."
        />
        <Endpoint
          method="POST"
          path="/v1/parties/join"
          auth
          body={`{ "inviteCode": "AB3D5F" }`}
          response={`{ "partyId": "...", "members": 3, "state": "open" }`}
          desc="Join a party by invite code. Max 16 members."
        />
        <Endpoint
          method="POST"
          path="/v1/multi/commit"
          auth
          body={`{ "gameId": "amp-tactics", "commitHash": "0x...", "stakeWei": 1000000000000000, "lobbySize": 8 }`}
          response={`{ "committed": true, "committedCount": 3, "ready": false }`}
          desc="Commit a blinded FFA queue entry: keccak256(address ‖ stake ‖ salt). Prevents lobby-targeting collusion."
        />
        <Endpoint
          method="POST"
          path="/v1/multi/reveal"
          auth
          body={`{ "gameId": "amp-tactics", "salt": "my-secret-salt" }`}
          response={`{ "revealed": true, "revealedCount": 8 }`}
          desc="Reveal your salt. Lobby forms when enough players reveal."
        />
        <Endpoint
          method="POST"
          path="/v1/multi/{id}/report"
          auth
          body={`{ "ranked": [["0x...", 1], ["0x...", 2]], "transcriptHash": "0x...", "sessionNonce": 42, "signature": "0x..." }`}
          response={`{ "matchId": "...", "state": "quorum", "concordant": 6, "quorumNeeded": 6 }`}
          desc="Submit an EIP-712 MultiplayerLadder report. Settlement fires when K = ⌊2N/3⌋+1 concordant reports arrive."
        />

        {/* WebSocket section */}
        <h2 className="text-xl font-black uppercase tracking-tight mb-4 mt-10 flex items-center gap-2">
          <Webhook className="w-5 h-5 text-brand-cyan" />
          WebSocket Events
        </h2>
        <p className="text-sm text-zinc-400 mb-6">
          Connect to{" "}
          <code className="text-brand-cyan">wss://amp.playwithamp.xyz/v1/ws?token=...</code>{" "}
          for real-time events. Automatic reconnection recommended.
        </p>

        <WsEvent
          type="hello"
          desc="Sent on connection. Confirms your identity."
          payload={`{ "type": "hello", "data": { "wallet": "0x..." } }`}
        />
        <WsEvent
          type="queue_status"
          desc="Live queue status while waiting. Pushed every tick."
          payload={`{ "type": "queue_status", "data": { "depth": 12, "waitedMs": 45000, "skillWindow": 710 } }`}
        />
        <WsEvent
          type="match_found"
          desc="Your match is ready. Includes opponent card and expiry."
          payload={`{ "type": "match_found", "data": { "matchId": "...", "opponent": { "wallet": "0x...", "rating": 1520, "region": "na" }, "expiresAt": "..." } }`}
        />
        <WsEvent
          type="match_result"
          desc="Match settled. Includes personalized rating delta."
          payload={`{ "type": "match_result", "data": { "matchId": "...", "outcome": "win_b", "won": true, "you": { "ratingBefore": 1500, "ratingAfter": 1552, "deviationAfter": 180 }, "attestation": { ... } } }`}
        />
        <WsEvent
          type="multi_lobby_formed"
          desc="N-player lobby is ready from revealed commits."
          payload={`{ "type": "multi_lobby_formed", "data": { "matchId": "...", "lobbySize": 8, "stakeWei": 1000000, "sessionNonce": 42 } }`}
        />
        <WsEvent
          type="multi_result"
          desc="N-player match settled with rating updates."
          payload={`{ "type": "multi_result", "data": { "matchId": "...", "outcome": { "ratingBefore": 1500, "ratingAfter": 1552, "delta": 52 } } }`}
        />

        {/* EIP-712 section */}
        <h2 className="text-xl font-black uppercase tracking-tight mb-4 mt-10 flex items-center gap-2">
          <Shield className="w-5 h-5 text-brand-cyan" />
          EIP-712 Typed Data
        </h2>
        <p className="text-sm text-zinc-400 mb-6">
          Match results and multiplayer ladders are signed with EIP-712 typed
          data. The domain separator is deterministic across chains.
        </p>

        <div className="glass-panel rounded-2xl border border-white/10 p-6 mb-4">
          <div className="text-xs font-black uppercase tracking-widest text-brand-cyan mb-3">
            Match Result (1v1)
          </div>
          <pre className="bg-black/60 border border-white/10 rounded-lg px-4 py-3 text-xs font-mono text-zinc-300 overflow-x-auto">
            <code>{`Domain: { name: "AMPSettlement", version: "1", chainId: 43113 }
Type: AsyncResult(uint256 matchId, uint8 outcome, bytes32 transcriptHash)`}</code>
          </pre>
        </div>

        <div className="glass-panel rounded-2xl border border-white/10 p-6 mb-4">
          <div className="text-xs font-black uppercase tracking-widest text-brand-cyan mb-3">
            Multiplayer Ladder (N-player)
          </div>
          <pre className="bg-black/60 border border-white/10 rounded-lg px-4 py-3 text-xs font-mono text-zinc-300 overflow-x-auto">
            <code>{`Domain: { name: "AMPMultiplayer", version: "1", chainId: 43113 }
Type: MultiplayerLadder(
  bytes32 matchId,
  bytes32 gameId,
  address[] rankedPlacements,  // rank 1 = winner
  bytes32 transcriptHash,
  uint256 sessionNonce
)`}</code>
          </pre>
        </div>

        {/* Rust core */}
        <h2 className="text-xl font-black uppercase tracking-tight mb-4 mt-10 flex items-center gap-2">
          <Terminal className="w-5 h-5 text-brand-cyan" />
          Embeddable Rust Core
        </h2>
        <p className="text-sm text-zinc-400 mb-4">
          Want the algorithms without the service? The core is a pure Rust
          crate with zero dependencies beyond serde:
        </p>
        <div className="glass-panel rounded-2xl border border-white/10 p-6 mb-4">
          <pre className="bg-black/60 border border-white/10 rounded-lg px-4 py-3 text-xs font-mono text-zinc-300 overflow-x-auto">
            <code>{`[dependencies]
amp-match-core = "0.1"

// Glicko-2 field update (order-independent, fuzz-gated)
use amp_match_core::glicko2_update_vs_many;
let (r, rd, vol) = glicko2_update_vs_many(1500.0, 350.0, 0.06, &opponents, &scores);

// Lobby shuffle (bias-free Fisher-Yates via blockhash)
use amp_match_core::shuffle_by_blockhash;
let shuffled = shuffle_by_blockhash(candidates, &blockhash);

// Anti-collusion commitments
use amp_match_core::ticket_commit;
let hash = ticket_commit(&address, stake_wei, &salt);`}</code>
          </pre>
        </div>

        <div className="mt-12 text-center">
          <Link
            href="/docs/contracts"
            className="text-sm text-brand-cyan hover:underline"
          >
            View deployed contracts & addresses →
          </Link>
        </div>
      </div>
    </div>
  );
}
