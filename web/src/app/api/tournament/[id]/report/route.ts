import { NextResponse } from "next/server";
import { getStore, validateBodySize, type BracketState } from "@/lib/store";
import { Tournament, Outcome, TournamentState, type TournamentFormat } from "@/lib/engine";
import { ethers } from "ethers";

export const runtime = "nodejs";

function formatOf(b: BracketState): TournamentFormat {
  if (b.format === "swiss") return { kind: TournamentState.Swiss, rounds: b.swissRounds ?? 3 };
  if (b.format === "round_robin") return { kind: TournamentState.RoundRobin };
  return { kind: TournamentState.SingleElimination };
}

function reconstruct(b: BracketState): Tournament<number> {
  const entrants = b.players.map((p) => ({ id: p.id, seed: p.seed }));
  const t = Tournament.new<number>(formatOf(b), entrants);
  for (const r of b.results) {
    const o = r.outcome === "B" ? Outcome.B : r.outcome === "Draw" ? Outcome.Draw : Outcome.A;
    t.record(r.matchId, o);
  }
  return t;
}

/**
 * POST /api/tournament/[id]/report — player result submission.
 * Requires EIP-191 signature proving the caller controls `wallet`.
 * Body: { wallet, matchId, outcome, nonce, ts, sig }
 */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ id: string }> }
) {
  const { id } = await params;
  if (validateBodySize(request)) return validateBodySize(request)!;
  const tid = Number(id);
  if (!Number.isFinite(tid)) return NextResponse.json({ error: "bad id" }, { status: 400 });

  const body = (await request.json().catch(() => ({}))) as {
    wallet?: string;
    matchId?: number;
    outcome?: "A" | "B" | "Draw";
    nonce?: string;
    ts?: number;
    sig?: string;
  };

  if (!body.wallet || body.matchId == null || !body.outcome || !body.nonce || !body.ts || !body.sig) {
    return NextResponse.json({ error: "wallet, matchId, outcome, nonce, ts, sig required" }, { status: 400 });
  }

  // Timestamp window (60s).
  const now = Math.floor(Date.now() / 1000);
  if (Math.abs(now - body.ts) > 60) {
    return NextResponse.json({ error: "timestamp expired" }, { status: 400 });
  }

  // Verify the EIP-191 signature.
  const message = `AMP-report:${tid}:${body.matchId}:${body.outcome}:${body.nonce}:${body.ts}`;
  let recovered: string;
  try {
    recovered = ethers.verifyMessage(message, body.sig);
  } catch {
    return NextResponse.json({ error: "invalid signature" }, { status: 401 });
  }
  if (recovered.toLowerCase() !== body.wallet.toLowerCase()) {
    return NextResponse.json({ error: "signature does not match wallet" }, { status: 401 });
  }

  // Nonce replay protection.
  const store = getStore();
  const fresh = await store.useNonce(body.wallet.toLowerCase(), tid, body.matchId, body.nonce);
  if (!fresh) return NextResponse.json({ error: "nonce already used" }, { status: 409 });

  const bracket = await store.getBracket(tid);
  if (!bracket) return NextResponse.json({ error: "bracket not found" }, { status: 404 });
  if (bracket.finalized) return NextResponse.json({ error: "tournament finalized" }, { status: 400 });

  const player = bracket.players.find((p) => p.wallet.toLowerCase() === body.wallet!.toLowerCase());
  if (!player) return NextResponse.json({ error: "not a player" }, { status: 403 });

  const engine = reconstruct(bracket);
  const match = engine.matches().find((m) => m.id === body.matchId);
  if (!match) return NextResponse.json({ error: "match not found" }, { status: 404 });
  if (match.outcome !== null) return NextResponse.json({ error: "match decided" }, { status: 400 });

  let side: "A" | "B" | null = null;
  if (match.a?.id === player.id) side = "A";
  else if (match.b?.id === player.id) side = "B";
  if (!side) return NextResponse.json({ error: "not in this match" }, { status: 403 });

  const reports = (bracket.reports ?? []).filter((r) => !(r.matchId === body.matchId && r.side === side));
  reports.push({ matchId: body.matchId, side, wallet: body.wallet, outcome: body.outcome });

  const otherSide = side === "A" ? "B" : "A";
  const other = reports.find((r) => r.matchId === body.matchId && r.side === otherSide);

  let status: "waiting" | "confirmed" | "disputed" = "waiting";
  const disputes = bracket.disputes ?? [];

  if (other) {
    if (other.outcome === body.outcome) {
      bracket.results = [...bracket.results, { matchId: body.matchId, outcome: body.outcome }];
      status = "confirmed";
    } else {
      if (!disputes.includes(body.matchId)) disputes.push(body.matchId);
      status = "disputed";
    }
  }

  bracket.reports = reports;
  bracket.disputes = disputes;
  await store.saveBracket(tid, bracket);

  return NextResponse.json({ ok: true, status });
}
