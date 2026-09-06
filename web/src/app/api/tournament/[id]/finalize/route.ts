import { NextResponse } from "next/server";
import { getStore, validateBodySize } from "@/lib/store";
import { requireOrganizer } from "@/lib/auth";
import { Tournament, Outcome, TournamentState, type TournamentFormat } from "@/lib/engine";
import type { BracketState } from "@/lib/store";

export const runtime = "nodejs";

function formatOf(b: BracketState): TournamentFormat {
  if (b.format === "swiss") return { kind: TournamentState.Swiss, rounds: b.swissRounds ?? 3 };
  if (b.format === "round_robin") return { kind: TournamentState.RoundRobin };
  return { kind: TournamentState.SingleElimination };
}

/**
 * POST /api/tournament/[id]/finalize — organizer-only.
 * Server-side: reconstructs the engine, computes winners deterministically,
 * stores them as computedWinners on the bracket, enqueues a {tournamentId}-only
 * finalize job. The relayer reads computedWinners from the bracket — the job
 * payload never carries payout addresses.
 */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ id: string }> }
) {
  const { id } = await params;
  if (validateBodySize(request)) return validateBodySize(request)!;
  const tid = Number(id);
  if (!Number.isFinite(tid)) return NextResponse.json({ error: "bad id" }, { status: 400 });

  const rec = await requireOrganizer(request, tid);
  if (!rec) return NextResponse.json({ error: "unauthorized" }, { status: 401 });

  const store = getStore();
  const bracket = await store.getBracket(tid);
  if (!bracket) return NextResponse.json({ error: "bracket not found" }, { status: 404 });
  if (bracket.finalized) return NextResponse.json({ error: "already finalized" }, { status: 400 });

  // Server-side winner derivation (P0-1: no client-supplied addresses).
  const entrants = bracket.players.map((p) => ({ id: p.id, seed: p.seed }));
  const engine = Tournament.new<number>(formatOf(bracket), entrants);
  for (const r of bracket.results) {
    const o = r.outcome === "B" ? Outcome.B : r.outcome === "Draw" ? Outcome.Draw : Outcome.A;
    engine.record(r.matchId, o);
  }
  if (!engine.isComplete()) return NextResponse.json({ error: "bracket not complete" }, { status: 400 });

  const winnerIds = engine.winners();
  const placements = rec.payoutBps.length;
  const computedWinners = winnerIds
    .slice(0, placements)
    .map((id) => bracket.players.find((p) => p.id === id)?.wallet ?? "")
    .filter(Boolean);

  if (computedWinners.length !== placements) {
    return NextResponse.json({ error: "winner derivation mismatch" }, { status: 500 });
  }

  // Persist computed winners on the bracket so the relayer reads them.
  bracket.computedWinners = computedWinners;
  bracket.finalized = true;
  await store.saveBracket(tid, bracket);

  // Enqueue finalize job — tournamentId ONLY, no payout addresses.
  const jobId = await store.enqueueJob("finalize", { tournamentId: tid });
  return NextResponse.json({ ok: true, jobId, pending: true });
}
