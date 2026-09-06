"use client";

import { useEffect, useMemo, useState, useCallback } from "react";
import Link from "next/link";
import { useParams } from "next/navigation";
import { CUP_ADDRESS, EXPLORER_URL } from "@/lib/ampCup";
import {
  ArrowLeft,
  Crown,
  Trophy,
  Loader2,
  Users,
  RefreshCw,
} from "lucide-react";
import {
  Tournament,
  Outcome,
  TournamentState,
  type TournamentFormat,
} from "@/lib/engine";
import type { BracketState } from "@/lib/store";

interface Player {
  id: number;
  wallet: string;
  name: string;
  seed: number;
}

function formatOf(b: BracketState): TournamentFormat {
  if (b.format === "swiss") return { kind: TournamentState.Swiss, rounds: b.swissRounds ?? 3 };
  if (b.format === "round_robin") return { kind: TournamentState.RoundRobin };
  return { kind: TournamentState.SingleElimination };
}

function reconstruct(b: BracketState | null): Tournament<number> | null {
  if (!b) return null;
  const entrants = b.players.map((p) => ({ id: p.id, seed: p.seed }));
  const t = Tournament.new<number>(formatOf(b), entrants);
  for (const r of b.results) {
    const o = r.outcome === "B" ? Outcome.B : r.outcome === "Draw" ? Outcome.Draw : Outcome.A;
    t.record(r.matchId, o);
  }
  return t;
}

export default function CupPage() {
  const params = useParams<{ id: string }>();
  const tid = Number(params.id);
  const [bracket, setBracket] = useState<BracketState | null>(null);
  const [loading, setLoading] = useState(true);
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null);

  const load = useCallback(async () => {
    try {
      const res = await fetch(`/api/tournament/${tid}`);
      const json = (await res.json()) as { bracket?: BracketState };
      setBracket(json.bracket ?? null);
      setLastUpdated(new Date());
    } catch {
      /* network blip — keep last state, retry next interval */
    }
  }, [tid]);

  useEffect(() => {
    // Initial load + 5s polling. setState happens post-await (async), not
    // synchronously in the effect body — the rule is a false positive here.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    load().finally(() => setLoading(false));
    const interval = setInterval(load, 5000);
    return () => clearInterval(interval);
  }, [load]);

  const engine = useMemo(() => reconstruct(bracket), [bracket]);
  const players = bracket?.players ?? [];
  const playerById = useMemo(() => {
    const m = new Map<number, Player>();
    players.forEach((p) => m.set(p.id, p));
    return m;
  }, [players]);

  if (loading) {
    return (
      <div className="min-h-screen bg-black flex items-center justify-center text-zinc-500">
        <Loader2 className="w-6 h-6 animate-spin" />
      </div>
    );
  }

  if (!bracket || !engine) {
    return (
      <div className="min-h-screen bg-black flex flex-col items-center justify-center text-zinc-400 gap-3">
        <Trophy className="w-8 h-8 text-zinc-600" />
        <p>Cup #{tid} not found or not yet started.</p>
        <Link href="/" className="text-brand-cyan hover:underline text-sm">← Home</Link>
      </div>
    );
  }

  const matches = engine.matches();
  const rounds = [...new Set(matches.map((m) => m.round))].sort((a, b) => a - b);
  const isComplete = engine.isComplete();
  const champion = isComplete ? engine.winners()[0] : null;
  const championPlayer = champion != null ? playerById.get(champion) : null;
  const disputes = bracket.disputes ?? [];

  const roundLabel = (r: number, total: number) => {
    if (bracket.format !== "single_elimination") return `Round ${r + 1}`;
    if (r === total - 1) return "Final";
    if (r === total - 2) return "Semifinals";
    if (r === total - 3) return "Quarterfinals";
    return `Round ${r + 1}`;
  };

  return (
    <div className="relative min-h-screen overflow-hidden antialiased bg-black text-white">
      <div className="absolute top-0 -left-1/4 w-[150%] h-[500px] bg-brand-cyan/10 blur-[120px] rounded-full pointer-events-none" />
      <div className="absolute bottom-0 -right-1/4 w-[150%] h-[500px] bg-brand-red/10 blur-[120px] rounded-full pointer-events-none" />

      <header className="relative z-10 max-w-6xl mx-auto px-6 pt-8 flex items-center justify-between">
        <Link href="/" className="inline-flex items-center gap-2 text-zinc-400 hover:text-brand-cyan transition-colors text-sm">
          <ArrowLeft className="w-4 h-4" /> AMP
        </Link>
        <div className="flex items-center gap-3 text-xs text-zinc-500">
          <span className="flex items-center gap-1">
            <span className="relative flex h-1.5 w-1.5">
              <span className="animate-ping absolute h-full w-full rounded-full bg-green-400 opacity-75" />
              <span className="relative rounded-full h-1.5 w-1.5 bg-green-400" />
            </span>
            live
          </span>
          <button onClick={load} className="hover:text-brand-cyan flex items-center gap-1">
            <RefreshCw className="w-3 h-3" /> {lastUpdated ? "updated" : "refresh"}
          </button>
        </div>
      </header>

      <main className="relative z-10 max-w-6xl mx-auto px-6 py-8">
        {/* Champion banner */}
        {isComplete && championPlayer && (
          <div className="glass-panel p-6 mb-6 text-center border-yellow-400/40">
            <Crown className="w-10 h-10 text-yellow-400 mx-auto mb-2 drop-shadow-[0_0_15px_rgba(250,204,21,0.6)]" />
            <div className="text-xs uppercase tracking-widest text-yellow-400 mb-1">Champion</div>
            <div className="text-3xl font-black uppercase tracking-tight">{championPlayer.name}</div>
            <code className="text-xs text-zinc-500">{championPlayer.wallet}</code>
          </div>
        )}

        <div className="flex items-center justify-between mb-5">
          <div>
            <h1 className="text-2xl font-black uppercase tracking-tight">Cup #{tid}</h1>
            <p className="text-zinc-400 text-sm">
              {players.length} players · {bracket.format.replace("_", " ")} ·{" "}
              {isComplete ? "final" : "in progress"}
            </p>
          </div>
          <div className="flex items-center gap-2 text-xs text-zinc-500">
            <Users className="w-4 h-4" />
            <Link href={`/play/${tid}`} className="hover:text-brand-cyan">I&rsquo;m a player →</Link>
          </div>
        </div>

        {disputes.length > 0 && (
          <div className="text-xs text-yellow-400 bg-yellow-400/10 border border-yellow-400/30 rounded-lg px-3 py-2 mb-5 inline-block">
            {disputes.length} match{disputes.length > 1 ? "es" : ""} awaiting organizer ruling
          </div>
        )}

        {/* Bracket rounds */}
        <div className="overflow-x-auto pb-4">
          <div className="grid gap-4 min-w-max" style={{ gridTemplateColumns: `repeat(${Math.min(rounds.length, 5)}, minmax(220px, 1fr))` }}>
            {rounds.map((r) => (
              <div key={r}>
                <h3 className="text-xs uppercase tracking-widest text-brand-cyan mb-3 px-1">
                  {roundLabel(r, rounds.length)}
                </h3>
                <div className="space-y-2.5">
                  {matches
                    .filter((m) => m.round === r)
                    .map((m) => {
                      const a = m.a ? playerById.get(m.a.id) : null;
                      const b = m.b ? playerById.get(m.b.id) : null;
                      const decided = m.outcome !== null;
                      const winnerIsA = decided && m.outcome === Outcome.A;
                      const winnerIsB = decided && m.outcome === Outcome.B;
                      const live = engine.pending().includes(m.id);
                      return (
                        <div
                          key={m.id}
                          className={`rounded-lg border p-2.5 transition-colors ${
                            live
                              ? "border-brand-cyan/50 bg-brand-cyan/5 shadow-[0_0_15px_rgba(0,229,255,0.1)]"
                              : "border-white/10 bg-white/[0.02]"
                          }`}
                        >
                          {[
                            { p: a, won: winnerIsA, lost: winnerIsB },
                            { p: b, won: winnerIsB, lost: winnerIsA },
                          ].map((row, i) => (
                            <div
                              key={i}
                              className={`flex items-center gap-2 py-1 px-1.5 rounded text-sm ${
                                row.lost ? "opacity-40" : row.won ? "bg-yellow-400/10" : ""
                              }`}
                            >
                              {row.won && <Crown className="w-3 h-3 text-yellow-400 shrink-0" />}
                              <span className={`truncate ${row.won ? "text-white font-bold" : "text-zinc-300"}`}>
                                {row.p ? row.p.name : <span className="text-zinc-600 italic text-xs">bye</span>}
                              </span>
                              {row.p && (
                                <span className="ml-auto text-[10px] text-zinc-600 shrink-0">#{row.p.seed}</span>
                              )}
                            </div>
                          ))}
                          {live && (
                            <div className="text-[9px] uppercase tracking-wider text-brand-cyan text-center mt-1">live</div>
                          )}
                        </div>
                      );
                    })}
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* Standings */}
        {isComplete && (
          <div className="mt-8">
            <h3 className="text-xs uppercase tracking-widest text-brand-cyan mb-3">Final standings</h3>
            <div className="space-y-1.5">
              {engine.winners().map((id, i) => {
                const p = playerById.get(id);
                if (!p) return null;
                const place = i + 1;
                return (
                  <div key={id} className="flex items-center gap-3 bg-white/5 border border-white/10 rounded-lg p-2.5">
                    <span className={`w-8 text-center font-black ${place <= 3 ? "text-yellow-400" : "text-zinc-500"}`}>
                      {place}
                    </span>
                    <span className="text-sm text-white">{p.name}</span>
                    <code className="text-[10px] text-zinc-600 ml-auto truncate">{p.wallet}</code>
                  </div>
                );
              })}
            </div>
          </div>
        )}

        <p className="text-center text-[11px] text-zinc-600 mt-8">
          Powered by the AMP Verifiable Tournament Engine ·{" "}
          <a href={`${EXPLORER_URL}/address/${CUP_ADDRESS}`} target="_blank" rel="noreferrer" className="hover:text-brand-cyan">
            view contract
          </a>
        </p>
      </main>
    </div>
  );
}
