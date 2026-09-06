"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import Link from "next/link";
import {
  Gamepad2,
  LogOut,
  RefreshCw,
  Swords,
  Trophy,
  Users,
  Zap,
  CircleStop,
  Wallet,
  ShieldCheck,
  Frown,
  Scale,
} from "lucide-react";
import {
  AMP_SERVER_URL,
  connectWs,
  fetchGames,
  matchmakerMisconfigured,
  fetchHistory,
  fetchMe,
  joinQueue,
  leaveQueue,
  loginWithWallet,
  clearSession,
  storedSession,
  reportOutcome,
  type GameInfo,
  type PlayerRating,
} from "@/lib/amp";

type Phase =
  | "loading"
  | "loggedOut"
  | "idle"
  | "queued"
  | "matchFound"
  | "reported"
  | "result"
  | "disputed";

interface MatchFoundView {
  matchId: string;
  opponent: { wallet: string; rating: number; region: string };
  yourRating: number;
  bot?: boolean;
}

interface ResultView {
  matchId: string;
  outcome: string;
  won: boolean;
  you: { ratingBefore: number; ratingAfter: number; deviationAfter: number };
  attested: boolean;
}

export default function ArenaPage() {
  const [phase, setPhase] = useState<Phase>("loading");
  const [wallet, setWallet] = useState<string | null>(null);
  const [ratings, setRatings] = useState<PlayerRating[]>([]);
  const [games, setGames] = useState<GameInfo[]>([]);
  const [selected, setSelected] = useState<{ gameId: string; rulesetId: string } | null>(null);
  const [queueStats, setQueueStats] = useState({ depth: 0, waitedMs: 0, skillWindow: 350 });
  const [matchView, setMatchView] = useState<MatchFoundView | null>(null);
  const [resultView, setResultView] = useState<ResultView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [offline, setOffline] = useState(false);
  const [busy, setBusy] = useState(false);
  const [history, setHistory] = useState<Record<string, unknown>[]>([]);
  const disconnectWs = useRef<(() => void) | null>(null);

  const refreshGames = useCallback(async () => {
    try {
      const { games: gs } = await fetchGames();
      setGames(gs);
      if (!selected && gs.length > 0 && gs[0].rulesets.length > 0) {
        setSelected({ gameId: gs[0].id, rulesetId: gs[0].rulesets[0].id });
      }
    } catch {
      /* server down — banner shows below */
    }
  }, [selected]);

  const refreshMe = useCallback(async () => {
    try {
      const me = await fetchMe();
      setWallet(me.wallet);
      setRatings(me.ratings);
      if (me.queueTicket) {
        setPhase("queued");
      } else if (me.liveMatchId) {
        setMatchView((prev) => prev ?? null);
        setPhase("matchFound");
      } else {
        setPhase("idle");
      }
      const h = await fetchHistory(10);
      setHistory(h.matches);
    } catch {
      clearSession();
      setWallet(null);
      setPhase("loggedOut");
    }
  }, []);

  const boot = useCallback(async () => {
    try {
      await refreshGames();
    } catch {
      setOffline(true);
      return; // stay in loading; the offline panel renders
    }
    if (storedSession()) {
      await refreshMe();
    } else {
      setPhase("loggedOut");
    }
  }, [refreshGames, refreshMe]);

  // Boot: session? games? me?
  useEffect(() => {
    boot();
  }, [boot]);

  // Live event wire (also drives the local wait timer).
  useEffect(() => {
    if (!storedSession()) return;
    const disconnect = connectWs((type, data) => {
      if (type === "queue_status") {
        setQueueStats({
          depth: (data.depth as number) ?? 0,
          waitedMs: (data.waitedMs as number) ?? 0,
          skillWindow: (data.skillWindow as number) ?? 350,
        });
      } else if (type === "match_found") {
        setMatchView({
          matchId: data.matchId as string,
          opponent: data.opponent as MatchFoundView["opponent"],
          yourRating: data.yourRating as number,
          bot: Boolean(data.bot),
        });
        setPhase("matchFound");
      } else if (type === "match_result") {
        setResultView({
          matchId: data.matchId as string,
          outcome: data.outcome as string,
          won: Boolean(data.won),
          you: data.you as ResultView["you"],
          attested: Boolean(data.attestation),
        });
        setPhase("result");
        refreshMe();
      } else if (type === "match_update") {
        if (data.state === "disputed") setPhase("disputed");
        if (data.state === "cancelled") {
          setPhase("idle");
          setError("Match cancelled (expired or opponent never showed).");
        }
      }
    });
    disconnectWs.current = disconnect;
    return () => disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wallet]);

  // Local wait ticker while queued.
  useEffect(() => {
    if (phase !== "queued") return;
    const started = Date.now() - queueStats.waitedMs;
    const t = setInterval(() => {
      setQueueStats((q) => ({ ...q, waitedMs: Date.now() - started }));
    }, 1000);
    return () => clearInterval(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [phase]);

  const connect = async () => {
    setBusy(true);
    setError(null);
    try {
      const s = await loginWithWallet();
      setWallet(s.wallet);
      await refreshMe();
    } catch (e) {
      setError(e instanceof Error ? e.message : "login failed");
    } finally {
      setBusy(false);
    }
  };

  const logout = () => {
    disconnectWs.current?.();
    clearSession();
    setWallet(null);
    setPhase("loggedOut");
  };

  const queue = async () => {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      await joinQueue(selected.gameId, selected.rulesetId);
      setQueueStats({ depth: 0, waitedMs: 0, skillWindow: 350 });
      setPhase("queued");
    } catch (e) {
      setError(e instanceof Error ? e.message : "could not join queue");
    } finally {
      setBusy(false);
    }
  };

  const leave = async () => {
    setBusy(true);
    try {
      await leaveQueue();
      setPhase("idle");
    } finally {
      setBusy(false);
    }
  };

  const report = async (result: "win" | "loss" | "draw") => {
    if (!matchView) return;
    setBusy(true);
    setError(null);
    try {
      await reportOutcome(matchView.matchId, result);
      setPhase("reported");
    } catch (e) {
      setError(e instanceof Error ? e.message : "report failed");
    } finally {
      setBusy(false);
    }
  };

  const playedAgain = async () => {
    setResultView(null);
    setMatchView(null);
    await refreshMe();
  };

  const fmtWait = (ms: number) => {
    const s = Math.floor(ms / 1000);
    return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
  };

  const myRating = ratings.find(
    (r) => selected && r.gameId === selected.gameId && r.rulesetId === selected.rulesetId,
  );

  return (
    <div className="min-h-screen bg-black text-white antialiased">
      <div className="absolute top-0 -left-1/4 w-[150%] h-[400px] bg-brand-cyan/10 blur-[120px] rounded-full pointer-events-none" />
      <div className="absolute bottom-0 -right-1/4 w-[150%] h-[400px] bg-brand-red/10 blur-[120px] rounded-full pointer-events-none" />

      <div className="relative z-10 max-w-3xl mx-auto px-6 py-12">
        {/* Build-time configuration failure: the site shipped with the
            localhost fallback for the matchmaker URL. */}
        {matchmakerMisconfigured() && (
          <div className="mb-6 rounded-2xl border border-brand-red/40 bg-brand-red/10 px-5 py-4 text-sm text-red-200">
            <p className="font-bold mb-1">Matchmaker not configured</p>
            <p className="text-red-200/90">
              This build points at <code>{AMP_SERVER_URL}</code>. Set{" "}
              <code>NEXT_PUBLIC_AMP_SERVER_URL</code> to your deployed
              amp-server URL in your hosting provider and <strong>trigger a
              rebuild</strong> (the variable is embedded at build time).
            </p>
          </div>
        )}

        {/* Header */}
        <div className="flex items-center justify-between mb-10">
          <Link href="/" className="flex items-center gap-3 group">
            <div className="w-10 h-10 rounded-xl bg-black border border-brand-cyan/30 flex items-center justify-center">
              <Swords className="w-5 h-5 text-brand-cyan" />
            </div>
            <div className="flex flex-col">
              <span className="text-xl font-black tracking-widest uppercase">AMP Arena</span>
              <span className="text-[10px] text-zinc-400 font-medium tracking-widest uppercase">
                Ranked matchmaking
              </span>
            </div>
          </Link>
          {wallet && (
            <div className="flex items-center gap-3">
              <span className="hidden sm:block text-xs text-zinc-400 font-mono">
                {wallet.slice(0, 6)}…{wallet.slice(-4)}
              </span>
              <button
                onClick={logout}
                className="text-zinc-400 hover:text-white transition-colors"
                title="Log out"
              >
                <LogOut className="w-4 h-4" />
              </button>
            </div>
          )}
        </div>

        {error && (
          <div className="mb-6 rounded-2xl border border-brand-red/30 bg-brand-red/10 px-5 py-4 text-sm text-red-200">
            {error}
          </div>
        )}

        {/* Loading / offline */}
        {phase === "loading" && !offline && (
          <div className="flex flex-col items-center py-24 text-zinc-400">
            <RefreshCw className="w-8 h-8 animate-spin mb-4" />
            Connecting to the matchmaker…
          </div>
        )}
        {phase === "loading" && offline && (
          <div className="glass-panel rounded-3xl border border-yellow-500/30 bg-yellow-500/5 p-10 text-center">
            <span className="inline-block rounded-full border border-yellow-500/40 bg-yellow-500/10 px-3 py-1 text-[10px] font-bold uppercase tracking-widest text-yellow-400 mb-4">
              Maintenance
            </span>
            <h2 className="text-2xl font-black uppercase tracking-tight mb-3">
              Matchmaking is offline
            </h2>
            <p className="text-zinc-400 max-w-md mx-auto mb-6">
              The matchmaker isn&apos;t reachable right now. Ranked play will
              be back shortly — nothing is at risk, your rating and history
              are safe.
            </p>
            <button
              onClick={() => { setOffline(false); boot(); }}
              className="inline-flex items-center gap-2 border border-white/15 hover:border-brand-cyan/50 text-zinc-200 px-6 py-3 rounded-2xl transition-colors"
            >
              <RefreshCw className="w-4 h-4" /> Retry
            </button>
          </div>
        )}

        {/* Logged out */}
        {phase === "loggedOut" && (
          <div className="glass-panel rounded-3xl border border-brand-cyan/20 p-10 text-center">
            <Wallet className="w-12 h-12 text-brand-cyan mx-auto mb-6" />
            <h1 className="text-3xl font-black uppercase tracking-tight mb-4">
              One signature. No gas.
            </h1>
            <p className="text-zinc-400 mb-8 max-w-md mx-auto">
              Connect your wallet to enter ranked matchmaking. Login is a free
              EIP-191 signature — you sign a challenge, never a transaction.
            </p>
            <button
              onClick={connect}
              disabled={busy}
              className="bg-gradient-to-r from-brand-cyan/20 to-transparent hover:from-brand-cyan/30 text-white px-8 py-4 rounded-2xl font-bold text-lg transition-all hover:scale-105 active:scale-95 border border-brand-cyan/30 hover:border-brand-cyan/60 shadow-[0_0_25px_rgba(0,229,255,0.2)] disabled:opacity-50"
            >
              {busy ? "Check your wallet…" : "Connect wallet"}
            </button>
          </div>
        )}

        {/* Idle: game picker + rating */}
        {(phase === "idle" || phase === "result") && games.length > 0 && (
          <div className="space-y-6">
            {phase === "result" && resultView && (
              <div className="glass-panel rounded-3xl border border-brand-cyan/30 p-8">
                <div className="flex items-center gap-4 mb-6">
                  {resultView.won ? (
                    <Trophy className="w-10 h-10 text-yellow-400" />
                  ) : (
                    <Frown className="w-10 h-10 text-zinc-400" />
                  )}
                  <div>
                    <h2 className="text-2xl font-black uppercase">
                      {resultView.outcome === "draw" ? "Draw" : resultView.won ? "Victory" : "Defeat"}
                    </h2>
                    <p className="text-sm text-zinc-400">Match settled · rating updated</p>
                  </div>
                </div>
                <div className="flex items-center gap-6 mb-4">
                  <div className="text-3xl font-black font-mono">
                    {Math.round(resultView.you.ratingBefore)}
                    <span className="text-zinc-500 mx-2">→</span>
                    <span className={resultView.you.ratingAfter >= resultView.you.ratingBefore ? "text-brand-cyan" : "text-brand-red"}>
                      {Math.round(resultView.you.ratingAfter)}
                    </span>
                  </div>
                  {resultView.attested && (
                    <span className="flex items-center gap-1.5 text-xs text-brand-cyan border border-brand-cyan/30 rounded-full px-3 py-1">
                      <ShieldCheck className="w-3.5 h-3.5" /> EIP-712 attested
                    </span>
                  )}
                </div>
                <button onClick={playedAgain} className="text-sm text-brand-cyan hover:underline">
                  Play again →
                </button>
              </div>
            )}

            <div className="glass-panel rounded-3xl border border-white/10 p-8">
              <div className="flex items-center gap-3 mb-6">
                <Gamepad2 className="w-6 h-6 text-brand-cyan" />
                <h2 className="text-xl font-bold">Find a match</h2>
              </div>

              <div className="space-y-3">
                {games.map((g) =>
                  g.rulesets.map((r) => {
                    const active = selected?.gameId === g.id && selected?.rulesetId === r.id;
                    return (
                      <button
                        key={`${g.id}/${r.id}`}
                        onClick={() => setSelected({ gameId: g.id, rulesetId: r.id })}
                        className={`w-full flex items-center justify-between rounded-2xl border px-5 py-4 transition-all ${
                          active
                            ? "border-brand-cyan/60 bg-brand-cyan/10"
                            : "border-white/10 hover:border-white/30 bg-white/5"
                        }`}
                      >
                        <span className="font-semibold">{g.name}</span>
                        <span className="flex items-center gap-4 text-sm text-zinc-400">
                          <span className="flex items-center gap-1.5">
                            <Users className="w-4 h-4" /> {r.queueDepth} in queue
                          </span>
                          {r.name}
                        </span>
                      </button>
                    );
                  }),
                )}
              </div>

              {myRating && (
                <div className="mt-6 flex items-center gap-6 text-sm">
                  <span className="text-zinc-400">
                    Your rating:{" "}
                    <span className="text-white font-mono font-bold">
                      {Math.round(myRating.rating)} ±{Math.round(myRating.deviation)}
                    </span>
                  </span>
                  <span className="text-zinc-500">
                    {myRating.wins}W / {myRating.losses}L / {myRating.draws}D
                  </span>
                </div>
              )}

              {(() => {
                const g = games.find((x) => x.id === selected?.gameId);
                const w = g?.nextQueueWindowUtc ? new Date(g.nextQueueWindowUtc) : null;
                if (!w) return null;
                return (
                  <p className="mt-4 text-xs text-zinc-500">
                    Prime-time queue window:{" "}
                    <span className="text-brand-cyan">
                      {w.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}{" "}
                      ({w.toLocaleDateString([], { weekday: "short" })}, your local time)
                    </span>{" "}
                    — lobbies fill fastest then.
                  </p>
                );
              })()}

              <button
                onClick={queue}
                disabled={busy || !selected}
                className="mt-8 w-full bg-gradient-to-r from-brand-cyan/20 to-transparent hover:from-brand-cyan/30 text-white px-8 py-5 rounded-2xl font-black text-lg uppercase tracking-wide transition-all hover:scale-[1.02] active:scale-95 border border-brand-cyan/30 hover:border-brand-cyan/60 shadow-[0_0_25px_rgba(0,229,255,0.15)] disabled:opacity-50"
              >
                <Zap className="inline w-5 h-5 mr-2 -mt-1" />
                Enter queue
              </button>
              <p className="mt-4 text-xs text-zinc-500 text-center">
                Free ranked play — stake AVAX modes land with on-chain escrow.
              </p>
            </div>

            {history.length > 0 && (
              <div className="glass-panel rounded-3xl border border-white/10 p-8">
                <h2 className="text-xl font-bold mb-4">Match history</h2>
                <div className="space-y-2">
                  {history.slice(0, 5).map((m) => {
                    const outcome = m.outcome as string | null;
                    const you = m.you as { wallet?: string } | undefined;
                    const won = outcome && m.winner === you?.wallet;
                    return (
                      <div
                        key={String(m.matchId)}
                        className="flex items-center justify-between rounded-xl bg-white/5 px-4 py-3 text-sm"
                      >
                        <span className="font-mono text-zinc-400">
                          {String(m.matchId).slice(0, 8)}…
                        </span>
                        <span className="text-zinc-500">{String(m.state)}</span>
                        <span className={won ? "text-brand-cyan" : outcome === "draw" ? "text-zinc-400" : "text-zinc-500"}>
                          {outcome === "draw" ? "draw" : won ? "won" : outcome ? "lost" : "—"}
                        </span>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}
          </div>
        )}

        {/* Queued */}
        {phase === "queued" && (
          <div className="glass-panel rounded-3xl border border-brand-cyan/20 p-10 text-center">
            <div className="w-20 h-20 mx-auto mb-8 rounded-full border-2 border-brand-cyan/30 border-t-brand-cyan animate-spin" />
            <h2 className="text-3xl font-black font-mono mb-2">
              {fmtWait(queueStats.waitedMs)}
            </h2>
            <p className="text-zinc-400 mb-6">
              in queue · {queueStats.depth} player{queueStats.depth === 1 ? "" : "s"} total ·
              skill window ±{Math.round(queueStats.skillWindow)}
            </p>
            <p className="text-xs text-zinc-500 mb-8 max-w-sm mx-auto">
              The matchmaker widens your skill window the longer you wait —
              tight games first, fair games eventually. Empty lobby? A
              practice bot picks you up so you&apos;re never stuck waiting.
            </p>
            <button
              onClick={leave}
              disabled={busy}
              className="inline-flex items-center gap-2 border border-white/15 hover:border-brand-red/50 hover:text-red-200 text-zinc-300 px-6 py-3 rounded-2xl transition-colors"
            >
              <CircleStop className="w-4 h-4" /> Leave queue
            </button>
          </div>
        )}

        {/* Match found */}
        {phase === "matchFound" && matchView && (
          <div className={`glass-panel rounded-3xl border p-10 ${matchView.bot ? "border-yellow-400/30" : "border-brand-cyan/30"}`}>
            <div className="text-center mb-8">
              <Swords className={`w-12 h-12 mx-auto mb-4 ${matchView.bot ? "text-yellow-400" : "text-brand-cyan"}`} />
              <h2 className="text-3xl font-black uppercase tracking-tight">
                {matchView.bot ? "Practice bot ready" : "Match found"}
              </h2>
            </div>
            <div className="grid grid-cols-2 gap-4 mb-8">
              <div className="rounded-2xl border border-brand-cyan/30 bg-brand-cyan/5 p-5 text-center">
                <p className="text-xs uppercase tracking-widest text-zinc-400 mb-2">You</p>
                <p className="text-2xl font-black font-mono">{Math.round(matchView.yourRating)}</p>
              </div>
              <div className={`rounded-2xl border p-5 text-center ${matchView.bot ? "border-yellow-400/30 bg-yellow-400/5" : "border-white/10 bg-white/5"}`}>
                <p className="text-xs uppercase tracking-widest text-zinc-400 mb-2">
                  {matchView.bot ? "Practice Bot" : "Opponent"}
                </p>
                <p className="text-2xl font-black font-mono">{Math.round(matchView.opponent.rating)}</p>
                {matchView.bot ? (
                  <p className="text-xs text-yellow-400/80 mt-1">unrated · no stress</p>
                ) : (
                  <p className="text-xs text-zinc-500 font-mono mt-1">
                    {matchView.opponent.wallet.slice(0, 6)}…{matchView.opponent.wallet.slice(-4)}
                  </p>
                )}
              </div>
            </div>
            <p className="text-sm text-zinc-400 text-center mb-8">
              {matchView.bot
                ? "Play your practice game and report the result — it settles instantly and never touches your rating."
                : "Play your match, then both players confirm the result below. Honest reporting keeps the ladder healthy — conflicts go to arbitration."}
            </p>
            <div className="grid grid-cols-3 gap-3">
              <button
                onClick={() => report("win")}
                disabled={busy}
                className="rounded-2xl border border-brand-cyan/40 bg-brand-cyan/10 hover:bg-brand-cyan/20 py-4 font-bold transition-colors disabled:opacity-50"
              >
                I won
              </button>
              <button
                onClick={() => report("draw")}
                disabled={busy}
                className="rounded-2xl border border-white/15 hover:border-white/30 bg-white/5 py-4 font-bold transition-colors disabled:opacity-50"
              >
                Draw
              </button>
              <button
                onClick={() => report("loss")}
                disabled={busy}
                className="rounded-2xl border border-white/15 hover:border-white/30 bg-white/5 py-4 font-bold transition-colors disabled:opacity-50"
              >
                I lost
              </button>
            </div>
            <p className="mt-4 text-xs text-zinc-500 text-center">
              Your wallet signs each report — tamper-proof evidence for rated and staked play.
            </p>
          </div>
        )}

        {/* Reported, waiting */}
        {phase === "reported" && (
          <div className="glass-panel rounded-3xl border border-white/10 p-10 text-center">
            <RefreshCw className="w-10 h-10 text-brand-cyan mx-auto mb-6 animate-spin" />
            <h2 className="text-2xl font-bold mb-3">Result submitted</h2>
            <p className="text-zinc-400">
              Waiting for your opponent to confirm. If they go silent past the
              match deadline, your result stands.
            </p>
          </div>
        )}

        {/* Disputed */}
        {phase === "disputed" && (
          <div className="glass-panel rounded-3xl border border-yellow-500/30 bg-yellow-500/5 p-10 text-center">
            <Scale className="w-10 h-10 text-yellow-400 mx-auto mb-6" />
            <h2 className="text-2xl font-bold mb-3">Result disputed</h2>
            <p className="text-zinc-400">
              The two reported results disagree. The match is held for operator
              arbitration — you will be notified here when it resolves.
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
