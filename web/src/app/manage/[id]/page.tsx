"use client";

import { useEffect, useMemo, useState, useCallback } from "react";
import Link from "next/link";
import { useParams } from "next/navigation";
import { ethers } from "ethers";
import {
  ArrowLeft,
  Trophy,
  Check,
  ExternalLink,
  Crown,
  Swords,
  Loader2,
} from "lucide-react";
import {
  Tournament,
  Outcome,
  TournamentState,
  type TournamentFormat,
} from "@/lib/engine";
import type { BracketState } from "@/lib/store";
import { CUP_ADDRESS, AMPCUP_ABI, connectWallet, signFinalize } from "@/lib/ampCup";
import { EXPLORER_URL } from "@/lib/ampCup";

interface TournamentRecord {
  tournamentId: number;
  sponsor: string;
  prizePoolWei: string;
  payoutBps: number[];
  winnerWallets: string[];
  state: "OPEN" | "FINALIZED" | "COMPLETE" | "CANCELLED";
  paypalOrderId?: string;
  txHash?: string | null;
}

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

function outcomeOf(o: string): Outcome {
  if (o === "B") return Outcome.B;
  if (o === "Draw") return Outcome.Draw;
  if (o === "Void") return Outcome.Void;
  return Outcome.A;
}

function reconstruct(b: BracketState | null): Tournament<number> | null {
  if (!b) return null;
  const entrants = b.players.map((p) => ({ id: p.id, seed: p.seed }));
  const t = Tournament.new<number>(formatOf(b), entrants);
  for (const r of b.results) t.record(r.matchId, outcomeOf(r.outcome));
  return t;
}

export default function ManagePage() {
  const params = useParams<{ id: string }>();
  const tid = Number(params.id);
  const [record, setRecord] = useState<TournamentRecord | null>(null);
  const [bracket, setBracket] = useState<BracketState | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [manageToken, setManageToken] = useState<string | null>(null);

  const load = useCallback(async () => {
    const res = await fetch(`/api/tournament/${tid}`);
    const json = (await res.json()) as { tournament?: TournamentRecord; bracket?: BracketState };
    setRecord(json.tournament ?? null);
    setBracket(json.bracket ?? null);
  }, [tid]);

  useEffect(() => {
    setManageToken(sessionStorage.getItem(`amp_manage_${tid}`));
    load().finally(() => setLoading(false));
  }, [load, tid]);

  const engine = useMemo(() => reconstruct(bracket), [bracket]);
  const players = bracket?.players ?? [];
  const playerById = useMemo(() => {
    const m = new Map<number, Player>();
    players.forEach((p) => m.set(p.id, p));
    return m;
  }, [players]);

  const isCustodial = Boolean(record?.paypalOrderId);
  const isComplete = engine?.isComplete() ?? false;
  const isFinalized = bracket?.finalized || record?.state === "FINALIZED" || record?.state === "COMPLETE";
  const placements = record?.payoutBps.length ?? 0;

  async function persist(next: BracketState) {
    setBracket(next);
    await fetch(`/api/tournament/${tid}/bracket`, {
      method: "PUT",
      headers: {
        "Content-Type": "application/json",
        ...(manageToken ? { Authorization: `Bearer ${manageToken}` } : {}),
      },
      body: JSON.stringify(next),
    });
  }

  async function recordResult(matchId: number, outcome: Outcome) {
    if (!bracket) return;
    await persist({ ...bracket, results: [...bracket.results, { matchId, outcome: outcome as "A" | "B" | "Draw" | "Void" }] });
  }

  const winnerIds = engine?.winners() ?? [];
  const winnerWallets = winnerIds.slice(0, placements).map((id) => playerById.get(id)?.wallet ?? "");

  async function finalize() {
    if (!record || winnerWallets.length === 0) return;
    setError(null);
    setBusy("Finalizing…");
    try {
      if (isCustodial) {
        const res = await fetch(`/api/tournament/${tid}/finalize`, {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            ...(manageToken ? { Authorization: `Bearer ${manageToken}` } : {}),
          },
          body: JSON.stringify({}),
        });
        const json = (await res.json()) as { ok?: boolean; error?: string; jobId?: number; pending?: boolean };
        if (!json.ok) throw new Error(json.error || "finalize failed");
        // Poll the job until the relayer completes on-chain.
        if (json.jobId) {
          for (let i = 0; i < 30; i++) {
            setBusy(`Relayer finalizing… (${i + 1})`);
            await new Promise((r) => setTimeout(r, 2000));
            const jr = await fetch(`/api/job/${json.jobId}`);
            const job = (await jr.json()) as { status?: string; txHash?: string };
            if (job.status === "done") {
              if (record) setRecord({ ...record, state: "FINALIZED", txHash: job.txHash ?? null });
              break;
            }
            if (job.status === "failed") throw new Error("relayer finalize failed");
          }
        }
      } else {
        // Sponsor (AVAX) path: connect wallet, sign EIP-712, submit finalize.
        const provider = await connectWallet();
        const signer = await provider.getSigner();
        const cup = new ethers.Contract(CUP_ADDRESS, AMPCUP_ABI, signer);
        const cleanWinners = winnerWallets.map((w) => ethers.getAddress(w));
        const sig = await signFinalize(signer, tid, cleanWinners);
        const tx = await cup.finalizeTournament(tid, cleanWinners, sig, { gasLimit: 400_000 });
        const rcpt = await tx.wait();
        if (record) setRecord({ ...record, state: "FINALIZED", txHash: rcpt?.hash ?? null, winnerWallets });
      }
      if (bracket) await persist({ ...bracket, finalized: true });
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(null);
    }
  }

  if (loading) {
    return (
      <div className="min-h-screen bg-black flex items-center justify-center text-zinc-500">
        <Loader2 className="w-6 h-6 animate-spin" />
      </div>
    );
  }

  if (!record || !engine || !bracket) {
    return (
      <div className="min-h-screen bg-black flex flex-col items-center justify-center text-zinc-400 gap-4">
        <p>Tournament #{tid} not found.</p>
        <Link href="/" className="text-brand-cyan hover:underline">← Home</Link>
      </div>
    );
  }

  const matches = engine.matches();
  const rounds = [...new Set(matches.map((m) => m.round))].sort((a, b) => a - b);
  const roundLabel = (r: number) =>
    bracket.format === "round_robin"
      ? `Round ${r + 1}`
      : bracket.format === "swiss"
      ? `Round ${r + 1}`
      : r === rounds[rounds.length - 1]
      ? "Final"
      : r === rounds[rounds.length - 2]
      ? "Semifinals"
      : `Round ${r + 1}`;

  return (
    <div className="relative min-h-screen overflow-hidden antialiased bg-black text-white">
      <div className="absolute top-0 -left-1/4 w-[150%] h-[500px] bg-brand-cyan/10 blur-[120px] rounded-full pointer-events-none" />
      <div className="absolute bottom-0 -right-1/4 w-[150%] h-[500px] bg-brand-red/10 blur-[120px] rounded-full pointer-events-none" />

      <header className="relative z-10 max-w-6xl mx-auto px-6 pt-8 flex items-center justify-between">
        <Link href="/" className="inline-flex items-center gap-2 text-zinc-400 hover:text-brand-cyan transition-colors text-sm">
          <ArrowLeft className="w-4 h-4" /> AMP
        </Link>
        <div className="text-xs text-zinc-500">
          Cup #{tid} · {isFinalized ? "Finalized" : isComplete ? "Ready to finalize" : "In progress"}
        </div>
      </header>

      <main className="relative z-10 max-w-6xl mx-auto px-6 py-8">
        <div className="glass-panel p-6 mb-6 flex flex-wrap items-center justify-between gap-4">
          <div>
            <h1 className="text-2xl font-black uppercase tracking-tight">Organizer Console</h1>
            <p className="text-zinc-400 text-sm mt-1">
              Prize {ethers.formatEther(record.prizePoolWei)} AVAX · {players.length} players · {bracket.format.replace("_", " ")}
            </p>
          </div>
          <div className="flex items-center gap-3">
            {(bracket.disputes ?? []).length > 0 && (
              <span className="text-xs text-yellow-400 bg-yellow-400/10 border border-yellow-400/30 px-2 py-1 rounded">
                {(bracket.disputes ?? []).length} dispute{(bracket.disputes ?? []).length > 1 ? "s" : ""}
              </span>
            )}
            <a
              href={`${EXPLORER_URL}/address/${CUP_ADDRESS}`}
              target="_blank"
              rel="noreferrer"
              className="text-xs text-zinc-400 hover:text-brand-cyan flex items-center gap-1"
            >
              contract <ExternalLink className="w-3 h-3" />
            </a>
          </div>
        </div>

        {/* Rounds */}
        <div className="grid gap-5" style={{ gridTemplateColumns: `repeat(${Math.min(rounds.length, 4)}, minmax(0, 1fr))` }}>
          {rounds.map((r) => (
            <div key={r}>
              <h3 className="text-xs uppercase tracking-widest text-brand-cyan mb-3">{roundLabel(r)}</h3>
              <div className="space-y-3">
                {matches.filter((m) => m.round === r).map((m) => {
                  const a = m.a ? playerById.get(m.a.id) : null;
                  const b = m.b ? playerById.get(m.b.id) : null;
                  const decided = m.outcome !== null;
                  const winnerSide = m.outcome === Outcome.A ? "a" : m.outcome === Outcome.B ? "b" : null;
                  const canRecord = engine.pending().includes(m.id);
                  return (
                    <div
                      key={m.id}
                      className={`rounded-lg border p-3 ${
                        (bracket.disputes ?? []).includes(m.id)
                          ? "border-yellow-400/60 bg-yellow-400/5"
                          : canRecord
                          ? "border-brand-cyan/50 bg-brand-cyan/5"
                          : "border-white/10 bg-white/[0.02]"
                      }`}
                    >
                      {(bracket.disputes ?? []).includes(m.id) && (
                        <div className="text-[10px] uppercase tracking-wider text-yellow-400 mb-1">disputed — resolve below</div>
                      )}
                      <div className="space-y-1.5">
                        {[["a", a], ["b", b]].map(([side, p]) => {
                          const player = p as Player | null;
                          const isWinner = decided && winnerSide === side;
                          const isLoser = decided && winnerSide && winnerSide !== side;
                          return (
                            <div
                              key={side as string}
                              className={`flex items-center gap-2 text-sm ${isLoser ? "opacity-40" : ""}`}
                            >
                              <span className={`w-5 ${isWinner ? "text-yellow-400" : "text-zinc-600"}`}>
                                {isWinner ? <Crown className="w-4 h-4" /> : side === "a" ? "A" : "B"}
                              </span>
                              <span className={`flex-1 truncate ${isWinner ? "text-white font-bold" : "text-zinc-300"}`}>
                                {player ? player.name : <span className="text-zinc-600 italic">bye</span>}
                              </span>
                              {canRecord && (
                                <button
                                  onClick={() => recordResult(m.id, side === "a" ? Outcome.A : Outcome.B)}
                                  className="text-[10px] uppercase tracking-wider px-2 py-1 rounded bg-brand-cyan/20 text-brand-cyan hover:bg-brand-cyan/30"
                                >
                                  win
                                </button>
                              )}
                            </div>
                          );
                        })}
                      </div>
                      {m.outcome === Outcome.Draw && (
                        <div className="text-[10px] text-zinc-500 mt-1">draw</div>
                      )}
                    </div>
                  );
                })}
              </div>
            </div>
          ))}
        </div>

        {/* Finalize panel */}
        {isComplete && (
          <div className="glass-panel p-6 mt-8 border-yellow-400/30">
            <div className="flex items-center gap-2 mb-4">
              <Trophy className="w-5 h-5 text-yellow-400" />
              <h3 className="text-sm font-bold uppercase tracking-wider">Bracket complete — finalize to pay winners</h3>
            </div>
            <div className="space-y-2 mb-5">
              {winnerIds.slice(0, placements).map((id, i) => {
                const p = playerById.get(id);
                return (
                  <div key={id} className="flex items-center gap-3 bg-white/5 border border-white/10 rounded-lg p-3">
                    <span className="text-xs font-bold text-yellow-400 w-10">{["1st", "2nd", "3rd", "4th"][i]}</span>
                    <span className="text-sm text-white flex-1 truncate">{p?.name ?? "—"}</span>
                    <code className="text-[10px] text-zinc-500 truncate max-w-[40]">{p?.wallet}</code>
                  </div>
                );
              })}
            </div>

            {isFinalized ? (
              <div className="flex items-center gap-2 text-green-400 text-sm">
                <Check className="w-4 h-4" /> Finalized on-chain. Winners can now claim.
                {record.winnerWallets.slice(0, placements).map((w, i) => (
                  <Link key={i} href={`/claim?tid=${tid}&place=${i}`} className="ml-4 text-brand-cyan hover:underline text-xs flex items-center gap-1">
                    claim {["1st", "2nd", "3rd", "4th"][i]} <ExternalLink className="w-3 h-3" />
                  </Link>
                ))}
              </div>
            ) : (
              <button
                onClick={finalize}
                disabled={!!busy}
                className="px-6 py-3 rounded-sm font-bold text-black bg-yellow-400 hover:bg-yellow-300 transition-colors flex items-center gap-2 uppercase tracking-widest text-sm disabled:opacity-40"
              >
                {busy ? <Loader2 className="w-4 h-4 animate-spin" /> : <Swords className="w-4 h-4" />}
                {busy ?? `Attest Results & Pay Winners ${isCustodial ? "(via relayer)" : "(sign with wallet)"}`}
              </button>
            )}
            {error && <p className="text-xs text-brand-red mt-3 font-mono">{error}</p>}
          </div>
        )}
      </main>
    </div>
  );
}
