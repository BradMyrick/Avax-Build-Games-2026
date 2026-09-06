"use client";

import { useEffect, useRef, useState } from "react";
import Link from "next/link";
import {
  Users,
  Copy,
  Check,
  Lock,
  Zap,
  Swords,
  Trophy,
  RefreshCw,
  ArrowLeft,
  UserPlus,
  Shield,
  ChevronUp,
  ChevronDown,
  CircleStop,
  Scale,
} from "lucide-react";
import {
  computeCommitHash,
  connectWs,
  createParty,
  disbandParty,
  getMultiMatch,
  getParty,
  joinParty,
  lockParty,
  multiClaim,
  multiCommit,
  multiReveal,
  multiReport,
  signLadder,
  storedSession,
} from "@/lib/amp";

type MultiPhase =
  | "loading"
  | "loggedOut"
  | "idle"
  | "partyOpen"
  | "partyLocked"
  | "committed"
  | "revealing"
  | "lobby"
  | "reporting"
  | "reported"
  | "result"
  | "disputed";

interface PartyView {
  id: string;
  leader: string;
  inviteCode: string;
  members: { wallet: string; region: string }[];
  state: string;
}

interface LobbyView {
  matchId: string;
  lobbySize: number;
  players: { wallet: string; index: number; rating: number; region: string }[];
  stakeWei: number;
  bondWei: number;
  sessionNonce: number;
}

interface ResultView {
  matchId: string;
  ratingBefore: number;
  ratingAfter: number;
  delta: number;
}

export default function MultiArenaPage() {
  const [phase, setPhase] = useState<MultiPhase>("loading");
  const [wallet, setWallet] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [party, setParty] = useState<PartyView | null>(null);
  const [joinCode, setJoinCode] = useState("");
  const [lobby, setLobby] = useState<LobbyView | null>(null);
  const [result, setResult] = useState<ResultView | null>(null);
  const [ranked, setRanked] = useState<string[]>([]);
  const [copied, setCopied] = useState(false);
  const [commitSalt, setCommitSalt] = useState<string>("");
  const [lobbySize, setLobbySize] = useState(8);
  const [stakeWei, setStakeWei] = useState(0);
  const disconnectWs = useRef<(() => void) | null>(null);

  const short = (w: string) => `${w.slice(0, 6)}…${w.slice(-4)}`;

  // Boot: check session
  useEffect(() => {
    const s = storedSession();
    if (s) {
      setWallet(s.wallet);
      setPhase("idle");
    } else {
      setPhase("loggedOut");
    }
  }, []);

  // WebSocket for real-time events
  useEffect(() => {
    if (!wallet) return;
    const disconnect = connectWs((type, data) => {
      const d = data as Record<string, unknown>;
      if (type === "multi_lobby_formed") {
        const matchId = d.matchId as string;
        setLobby({
          matchId,
          lobbySize: d.lobbySize as number,
          players: [],
          stakeWei: d.stakeWei as number,
          bondWei: d.bondWei as number,
          sessionNonce: d.sessionNonce as number,
        });
        setPhase("lobby");
        getMultiMatch(matchId)
          .then((m) => {
            setLobby({
              matchId: m.matchId,
              lobbySize: m.lobbySize,
              players: m.players.map((p) => ({
                wallet: p.wallet,
                index: p.index,
                rating: p.rating,
                region: p.region,
              })),
              stakeWei: m.stakePerPlayer,
              bondWei: m.bondPerPlayer,
              sessionNonce: d.sessionNonce as number,
            });
            setRanked(m.players.map((p) => p.wallet));
          })
          .catch(() => {});
      } else if (type === "multi_result") {
        const outcome = d.outcome as { ratingBefore?: number; ratingAfter?: number; delta?: number } | undefined;
        setResult({
          matchId: d.matchId as string,
          ratingBefore: outcome?.ratingBefore ?? 0,
          ratingAfter: outcome?.ratingAfter ?? 0,
          delta: outcome?.delta ?? 0,
        });
        setPhase("result");
      } else if (type === "multi_cancelled") {
        setPhase("idle");
        setError("Match cancelled — lobby expired.");
      }
    });
    disconnectWs.current = disconnect;
    return () => disconnect();
  }, [wallet]);

  // Actions
  const handleConnect = async () => {
    setBusy(true);
    setError(null);
    try {
      const { loginWithWallet } = await import("@/lib/amp");
      const s = await loginWithWallet();
      setWallet(s.wallet);
      setPhase("idle");
    } catch (e) {
      setError(e instanceof Error ? e.message : "login failed");
    } finally {
      setBusy(false);
    }
  };

  const handleCreateParty = async () => {
    setBusy(true);
    setError(null);
    try {
      const p = await createParty("amp-tactics", "ranked-1v1");
      setParty({
        id: p.partyId,
        leader: p.leader,
        inviteCode: p.inviteCode,
        members: [{ wallet: p.leader, region: "na" }],
        state: "open",
      });
      setPhase("partyOpen");
    } catch (e) {
      setError(e instanceof Error ? e.message : "failed to create party");
    } finally {
      setBusy(false);
    }
  };

  const handleJoinParty = async () => {
    if (!joinCode.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const p = await joinParty(joinCode.trim().toUpperCase());
      const full = await getParty(p.partyId);
      setParty({
        id: full.partyId,
        leader: full.leader,
        inviteCode: full.inviteCode,
        members: full.members.map((m) => ({ wallet: m.wallet, region: m.region })),
        state: full.state,
      });
      setPhase("partyOpen");
    } catch (e) {
      setError(e instanceof Error ? e.message : "failed to join");
    } finally {
      setBusy(false);
    }
  };

  const handleLockParty = async () => {
    if (!party) return;
    setBusy(true);
    try {
      await lockParty(party.id);
      setParty({ ...party, state: "locked" });
      setPhase("partyLocked");
    } catch (e) {
      setError(e instanceof Error ? e.message : "failed to lock");
    } finally {
      setBusy(false);
    }
  };

  const handleDisband = async () => {
    if (!party) return;
    setBusy(true);
    try {
      await disbandParty(party.id);
      setParty(null);
      setPhase("idle");
    } finally {
      setBusy(false);
    }
  };

  const handleCommit = async () => {
    if (!wallet) return;
    setBusy(true);
    setError(null);
    try {
      // Generate a random salt
      const salt = Array.from(crypto.getRandomValues(new Uint8Array(16)))
        .map((b) => b.toString(16).padStart(2, "0"))
        .join("");
      setCommitSalt(salt);

      const hash = await computeCommitHash(wallet, stakeWei, salt);
      const res = await multiCommit("amp-tactics", hash, stakeWei, lobbySize);
      setPhase("committed");
      if (res.ready) {
        setError(`Lobby ready! ${res.committedCount} players committed. Reveal now.`);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "commit failed");
    } finally {
      setBusy(false);
    }
  };

  const handleReveal = async () => {
    if (!commitSalt) return;
    setBusy(true);
    setError(null);
    try {
      const res = await multiReveal("amp-tactics", "ranked-1v1", commitSalt);
      if (res.revealed) {
        setPhase("revealing");
        setError(`Revealed! ${res.revealedCount}/${lobbySize} players revealed.`);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "reveal failed");
    } finally {
      setBusy(false);
    }
  };

  const movePlayer = (from: number, to: number) => {
    if (from === to || from < 0 || to < 0 || from >= ranked.length || to >= ranked.length) return;
    const next = [...ranked];
    const [item] = next.splice(from, 1);
    next.splice(to, 0, item);
    setRanked(next);
  };

  const handleReport = async () => {
    if (!lobby || !wallet || ranked.length === 0) return;
    setBusy(true);
    setError(null);
    try {
      // Sign the ladder with EIP-712
      const sig = await signLadder({
        wallet,
        matchId: `0x${lobby.matchId.replace(/-/g, "").padEnd(64, "0").slice(0, 64)}`,
        gameId: `0x${"1".padStart(64, "0")}`,
        ranked,
        transcriptHash: `0x${"0".repeat(64)}`,
        sessionNonce: lobby.sessionNonce,
        chainId: 43113,
        contractAddress: "0xcabf7b626172fE55d54f03c346563671AbcC77f7",
      });

      const rankedTuples: [string, number][] = ranked.map((w, i) => [w, i + 1]);
      const res = await multiReport(
        lobby.matchId,
        rankedTuples,
        `0x${"0".repeat(64)}`,
        lobby.sessionNonce,
        sig,
      );
      if (res.state === "quorum") {
        setPhase("reported");
      } else {
        setPhase("reported");
        setError(`Reported! ${res.quorumNeeded} needed for quorum.`);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : "report failed");
    } finally {
      setBusy(false);
    }
  };

  const handleClaim = async () => {
    if (!lobby) return;
    setBusy(true);
    try {
      await multiClaim(lobby.matchId);
    } catch (e) {
      setError(e instanceof Error ? e.message : "claim failed");
    } finally {
      setBusy(false);
    }
  };

  const copyInvite = async () => {
    if (!party) return;
    await navigator.clipboard.writeText(party.inviteCode);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const isLeader = wallet && party && party.leader === wallet;

  return (
    <div className="min-h-screen bg-black text-white antialiased">
      <div className="absolute top-0 -left-1/4 w-[150%] h-[400px] bg-brand-cyan/10 blur-[120px] rounded-full pointer-events-none" />
      <div className="absolute bottom-0 -right-1/4 w-[150%] h-[400px] bg-brand-red/10 blur-[120px] rounded-full pointer-events-none" />

      <div className="relative z-10 max-w-3xl mx-auto px-6 py-12">
        {/* Header */}
        <div className="flex items-center justify-between mb-10">
          <Link href="/arena" className="flex items-center gap-2 text-sm text-zinc-500 hover:text-brand-cyan transition-colors">
            <ArrowLeft className="w-4 h-4" />
            1v1 Arena
          </Link>
          <div className="flex items-center gap-3">
            <span className="text-xl font-black tracking-widest uppercase">N-Player</span>
            <span className="text-[10px] text-zinc-400 font-medium tracking-widest uppercase border border-brand-cyan/30 rounded-full px-2 py-0.5 text-brand-cyan">
              Beta
            </span>
          </div>
        </div>

        {error && (
          <div className="mb-6 rounded-2xl border border-brand-red/30 bg-brand-red/10 px-5 py-4 text-sm text-red-200">
            {error}
          </div>
        )}

        {/* Loading */}
        {phase === "loading" && (
          <div className="flex flex-col items-center py-24 text-zinc-400">
            <RefreshCw className="w-8 h-8 animate-spin mb-4" />
            Loading…
          </div>
        )}

        {/* Logged out */}
        {phase === "loggedOut" && (
          <div className="glass-panel rounded-3xl border border-brand-cyan/20 p-10 text-center">
            <Users className="w-12 h-12 text-brand-cyan mx-auto mb-6" />
            <h1 className="text-3xl font-black uppercase tracking-tight mb-4">
              One signature. No gas.
            </h1>
            <p className="text-zinc-400 mb-8 max-w-md mx-auto">
              Connect your wallet to enter N-player matchmaking. Free play
              requires one gasless signature.
            </p>
            <button
              onClick={handleConnect}
              disabled={busy}
              className="bg-gradient-to-r from-brand-cyan/20 to-transparent hover:from-brand-cyan/30 text-white px-8 py-4 rounded-2xl font-bold text-lg transition-all hover:scale-105 active:scale-95 border border-brand-cyan/30 hover:border-brand-cyan/60 shadow-[0_0_25px_rgba(0,229,255,0.2)] disabled:opacity-50"
            >
              {busy ? "Check your wallet…" : "Connect wallet"}
            </button>
          </div>
        )}

        {/* Idle: choose party or solo queue */}
        {phase === "idle" && (
          <div className="space-y-6">
            <div className="glass-panel rounded-3xl border border-white/10 p-8">
              <h2 className="text-xl font-bold mb-4 flex items-center gap-2">
                <Users className="w-5 h-5 text-brand-cyan" />
                Play with friends
              </h2>
              <div className="space-y-4">
                <button
                  onClick={handleCreateParty}
                  disabled={busy}
                  className="w-full rounded-2xl border border-brand-cyan/40 bg-brand-cyan/10 hover:bg-brand-cyan/20 py-4 font-bold transition-colors disabled:opacity-50"
                >
                  Create a party
                </button>
                <div className="flex gap-2">
                  <input
                    value={joinCode}
                    onChange={(e) => setJoinCode(e.target.value.toUpperCase())}
                    placeholder="Invite code"
                    maxLength={6}
                    className="flex-1 rounded-2xl border border-white/15 bg-white/5 px-4 py-4 font-mono text-lg tracking-widest placeholder:text-zinc-600 focus:border-brand-cyan/50 outline-none"
                  />
                  <button
                    onClick={handleJoinParty}
                    disabled={busy || joinCode.length !== 6}
                    className="rounded-2xl border border-white/15 hover:border-white/30 bg-white/5 px-6 font-bold transition-colors disabled:opacity-30"
                  >
                    <UserPlus className="w-5 h-5" />
                  </button>
                </div>
              </div>
            </div>

            <div className="glass-panel rounded-3xl border border-white/10 p-8">
              <h2 className="text-xl font-bold mb-4 flex items-center gap-2">
                <Zap className="w-5 h-5 text-brand-cyan" />
                Queue solo (FFA)
              </h2>
              <div className="grid grid-cols-2 gap-4 mb-4">
                <div>
                  <label className="text-xs text-zinc-500 uppercase tracking-widest block mb-2">
                    Lobby size
                  </label>
                  <select
                    value={lobbySize}
                    onChange={(e) => setLobbySize(Number(e.target.value))}
                    className="w-full rounded-xl border border-white/15 bg-white/5 px-4 py-3 outline-none focus:border-brand-cyan/50"
                  >
                    <option value={4}>4 players</option>
                    <option value={8}>8 players</option>
                    <option value={16}>16 players</option>
                  </select>
                </div>
                <div>
                  <label className="text-xs text-zinc-500 uppercase tracking-widest block mb-2">
                    Stake (AVAX)
                  </label>
                  <select
                    value={stakeWei}
                    onChange={(e) => setStakeWei(Number(e.target.value))}
                    className="w-full rounded-xl border border-white/15 bg-white/5 px-4 py-3 outline-none focus:border-brand-cyan/50"
                  >
                    <option value={0}>Free play</option>
                    <option value={1000000000000000}>0.001 AVAX</option>
                    <option value={10000000000000000}>0.01 AVAX</option>
                    <option value={100000000000000000}>0.1 AVAX</option>
                  </select>
                </div>
              </div>
              <button
                onClick={handleCommit}
                disabled={busy}
                className="w-full bg-gradient-to-r from-brand-cyan/20 to-transparent hover:from-brand-cyan/30 text-white px-8 py-4 rounded-2xl font-black text-lg uppercase tracking-wide transition-all hover:scale-[1.02] active:scale-95 border border-brand-cyan/30 hover:border-brand-cyan/60 shadow-[0_0_25px_rgba(0,229,255,0.15)] disabled:opacity-50"
              >
                <Shield className="inline w-5 h-5 mr-2 -mt-1" />
                Commit to queue
              </button>
              <p className="mt-3 text-xs text-zinc-500 text-center">
                Commit-reveal prevents lobby targeting. Your identity is hidden
                until the lobby forms.
              </p>
            </div>
          </div>
        )}

        {/* Party open: invite + wait */}
        {phase === "partyOpen" && party && (
          <div className="glass-panel rounded-3xl border border-brand-cyan/20 p-8">
            <div className="flex items-center justify-between mb-6">
              <h2 className="text-xl font-bold flex items-center gap-2">
                <Users className="w-5 h-5 text-brand-cyan" />
                Party ({party.members.length}/16)
              </h2>
              <span className="text-xs text-zinc-500 border border-white/10 rounded-full px-3 py-1">
                {party.state}
              </span>
            </div>

            <div className="flex items-center gap-3 mb-6">
              <code className="text-2xl font-black font-mono tracking-[0.3em] text-brand-cyan bg-black/40 border border-brand-cyan/20 rounded-2xl px-6 py-3">
                {party.inviteCode}
              </code>
              <button
                onClick={copyInvite}
                className="p-3 rounded-xl border border-white/15 hover:border-brand-cyan/40 transition-colors"
                title="Copy invite code"
              >
                {copied ? <Check className="w-5 h-5 text-green-400" /> : <Copy className="w-5 h-5 text-zinc-400" />}
              </button>
            </div>

            <div className="space-y-2 mb-6">
              {party.members.map((m) => (
                <div key={m.wallet} className="flex items-center justify-between rounded-xl bg-white/5 px-4 py-3">
                  <span className="font-mono text-sm text-zinc-300">{short(m.wallet)}</span>
                  {m.wallet === party.leader ? (
                    <span className="text-xs text-yellow-400 font-bold uppercase tracking-wider">Leader</span>
                  ) : (
                    <span className="text-xs text-zinc-500">{m.region}</span>
                  )}
                </div>
              ))}
            </div>

            <div className="flex gap-3">
              {isLeader && (
                <button
                  onClick={handleLockParty}
                  disabled={busy || party.members.length < 1}
                  className="flex-1 rounded-2xl border border-brand-cyan/40 bg-brand-cyan/10 hover:bg-brand-cyan/20 py-3 font-bold flex items-center justify-center gap-2 transition-colors"
                >
                  <Lock className="w-4 h-4" />
                  Lock & Queue
                </button>
              )}
              <button
                onClick={handleDisband}
                disabled={busy}
                className="rounded-2xl border border-white/15 hover:border-brand-red/50 hover:text-red-200 text-zinc-300 px-5 py-3 transition-colors"
              >
                <CircleStop className="w-4 h-4" />
              </button>
            </div>
          </div>
        )}

        {/* Party locked: ready to queue */}
        {phase === "partyLocked" && party && (
          <div className="glass-panel rounded-3xl border border-green-400/30 bg-green-400/5 p-8 text-center">
            <Lock className="w-10 h-10 text-green-400 mx-auto mb-4" />
            <h2 className="text-xl font-bold mb-2">Party locked</h2>
            <p className="text-sm text-zinc-400 mb-6">
              {party.members.length} player{party.members.length > 1 ? "s" : ""} ready.
              The leader can now commit to the FFA queue.
            </p>
            {isLeader && (
              <button
                onClick={handleCommit}
                disabled={busy}
                className="bg-gradient-to-r from-brand-cyan/20 to-transparent hover:from-brand-cyan/30 px-8 py-4 rounded-2xl font-bold border border-brand-cyan/30 hover:border-brand-cyan/60 transition-all disabled:opacity-50"
              >
                <Shield className="inline w-5 h-5 mr-2 -mt-1" />
                Commit to queue
              </button>
            )}
          </div>
        )}

        {/* Committed: waiting for lobby threshold */}
        {phase === "committed" && (
          <div className="glass-panel rounded-3xl border border-yellow-400/30 bg-yellow-400/5 p-8 text-center">
            <Shield className="w-10 h-10 text-yellow-400 mx-auto mb-4" />
            <h2 className="text-xl font-bold mb-2">Committed</h2>
            <p className="text-sm text-zinc-400 mb-6">
              Your blinded commitment is in the pool. When {lobbySize} players
              commit, everyone reveals their salt and the lobby forms.
            </p>
            <button
              onClick={handleReveal}
              disabled={busy || !commitSalt}
              className="rounded-2xl border border-yellow-400/40 bg-yellow-400/10 hover:bg-yellow-400/20 px-8 py-4 font-bold transition-colors disabled:opacity-50"
            >
              Reveal salt
            </button>
          </div>
        )}

        {/* Revealing: waiting for lobby */}
        {phase === "revealing" && (
          <div className="glass-panel rounded-3xl border border-brand-cyan/20 p-10 text-center">
            <div className="w-16 h-16 mx-auto mb-6 rounded-full border-2 border-brand-cyan/30 border-t-brand-cyan animate-spin" />
            <h2 className="text-xl font-bold mb-2">Forming lobby…</h2>
            <p className="text-sm text-zinc-400">
              Waiting for all players to reveal. The lobby shuffles once
              everyone has revealed.
            </p>
          </div>
        )}

        {/* Lobby: match found */}
        {phase === "lobby" && lobby && (
          <div className="glass-panel rounded-3xl border border-brand-cyan/30 p-8">
            <div className="text-center mb-6">
              <Swords className="w-12 h-12 text-brand-cyan mx-auto mb-4" />
              <h2 className="text-2xl font-black uppercase tracking-tight">Lobby Formed</h2>
              <p className="text-sm text-zinc-400 mt-1">
                {lobby.lobbySize} players · Stake {lobby.stakeWei > 0 ? `${lobby.stakeWei / 1e18} AVAX` : "Free"}
              </p>
            </div>

            <div className="grid grid-cols-2 md:grid-cols-4 gap-2 mb-6">
              {lobby.players.map((p) => (
                <div key={p.wallet} className="rounded-xl bg-white/5 border border-white/10 p-3 text-center">
                  <div className="text-sm font-mono text-zinc-300">{short(p.wallet)}</div>
                  <div className="text-xs text-brand-cyan font-bold">{Math.round(p.rating)}</div>
                </div>
              ))}
            </div>

            <p className="text-sm text-zinc-400 text-center mb-6">
              Play your match, then rank every player below. When{" "}
              {Math.floor((2 * lobby.lobbySize) / 3) + 1} players submit
              matching ladders, the match settles.
            </p>

            <button
              onClick={() => setPhase("reporting")}
              className="w-full bg-gradient-to-r from-brand-cyan/20 to-transparent hover:from-brand-cyan/30 py-4 rounded-2xl font-bold border border-brand-cyan/30 transition-all"
            >
              Start ranking →
            </button>
          </div>
        )}

        {/* Reporting: drag-order the ladder */}
        {phase === "reporting" && lobby && (
          <div className="glass-panel rounded-3xl border border-brand-cyan/30 p-8">
            <h2 className="text-xl font-bold mb-2 flex items-center gap-2">
              <Trophy className="w-5 h-5 text-yellow-400" />
              Rank the players
            </h2>
            <p className="text-sm text-zinc-400 mb-6">
              Rank 1 = winner. Reorder with the arrows, then sign and submit.
            </p>

            <div className="space-y-2 mb-6">
              {ranked.map((w, i) => (
                <div
                  key={w}
                  className={`flex items-center justify-between rounded-xl px-4 py-3 border ${
                    i === 0
                      ? "border-yellow-400/40 bg-yellow-400/10"
                      : i < 3
                        ? "border-brand-cyan/20 bg-brand-cyan/5"
                        : "border-white/10 bg-white/5"
                  }`}
                >
                  <div className="flex items-center gap-3">
                    <span
                      className={`w-8 h-8 rounded-full flex items-center justify-center font-black text-sm ${
                        i === 0 ? "bg-yellow-400/20 text-yellow-400" : "bg-white/10 text-zinc-400"
                      }`}
                    >
                      {i + 1}
                    </span>
                    <span className="font-mono text-sm text-zinc-300">{short(w)}</span>
                    {w === wallet && (
                      <span className="text-[10px] text-brand-cyan font-bold uppercase">You</span>
                    )}
                  </div>
                  <div className="flex gap-1">
                    <button
                      onClick={() => movePlayer(i, i - 1)}
                      disabled={i === 0}
                      className="p-1.5 rounded-lg border border-white/10 hover:border-white/30 disabled:opacity-20"
                    >
                      <ChevronUp className="w-4 h-4" />
                    </button>
                    <button
                      onClick={() => movePlayer(i, i + 1)}
                      disabled={i === ranked.length - 1}
                      className="p-1.5 rounded-lg border border-white/10 hover:border-white/30 disabled:opacity-20"
                    >
                      <ChevronDown className="w-4 h-4" />
                    </button>
                  </div>
                </div>
              ))}
            </div>

            <button
              onClick={handleReport}
              disabled={busy}
              className="w-full bg-gradient-to-r from-brand-cyan/20 to-transparent hover:from-brand-cyan/30 py-4 rounded-2xl font-bold border border-brand-cyan/30 transition-all disabled:opacity-50"
            >
              {busy ? "Check your wallet…" : "Sign & submit ladder"}
            </button>
            <p className="mt-3 text-xs text-zinc-500 text-center">
              Your wallet signs the EIP-712 MultiplayerLadder — non-repudiable
              evidence for settlement.
            </p>
          </div>
        )}

        {/* Reported: waiting for quorum */}
        {phase === "reported" && (
          <div className="glass-panel rounded-3xl border border-white/10 p-10 text-center">
            <RefreshCw className="w-10 h-10 text-brand-cyan mx-auto mb-6 animate-spin" />
            <h2 className="text-xl font-bold mb-2">Ladder submitted</h2>
            <p className="text-sm text-zinc-400 mb-6">
              Waiting for {Math.floor((2 * (lobby?.lobbySize ?? 8)) / 3) + 1}{" "}
              concordant reports to reach quorum.
            </p>
            {lobby && (
              <button
                onClick={handleClaim}
                disabled={busy}
                className="rounded-2xl border border-brand-cyan/40 bg-brand-cyan/10 hover:bg-brand-cyan/20 px-8 py-3 font-bold transition-colors disabled:opacity-50"
              >
                Trigger settlement
              </button>
            )}
          </div>
        )}

        {/* Result */}
        {phase === "result" && result && (
          <div className="glass-panel rounded-3xl border border-brand-cyan/30 p-8 text-center">
            {result.delta > 0 ? (
              <Trophy className="w-12 h-12 text-yellow-400 mx-auto mb-4" />
            ) : result.delta < 0 ? (
              <Scale className="w-12 h-12 text-zinc-500 mx-auto mb-4" />
            ) : (
              <Shield className="w-12 h-12 text-brand-cyan mx-auto mb-4" />
            )}
            <h2 className="text-2xl font-black uppercase tracking-tight mb-4">
              {result.delta > 0 ? "Rating up" : result.delta < 0 ? "Rating down" : "No change"}
            </h2>
            <div className="text-4xl font-black font-mono mb-2">
              {Math.round(result.ratingBefore)}
              <span className="text-zinc-500 mx-2">→</span>
              <span className={result.delta >= 0 ? "text-brand-cyan" : "text-brand-red"}>
                {Math.round(result.ratingAfter)}
              </span>
            </div>
            <div className={`text-sm font-bold ${result.delta >= 0 ? "text-brand-cyan" : "text-brand-red"}`}>
              {result.delta >= 0 ? "+" : ""}{result.delta.toFixed(1)}
            </div>
            <button
              onClick={() => { setResult(null); setLobby(null); setPhase("idle"); }}
              className="mt-6 text-sm text-brand-cyan hover:underline"
            >
              Play again →
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
