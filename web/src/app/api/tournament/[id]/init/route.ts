import { NextResponse } from "next/server";
import { getStore, validateBodySize, type BracketState } from "@/lib/store";
import { requireOrganizer, generateManageToken } from "@/lib/auth";

export const runtime = "nodejs";

/**
 * POST /api/tournament/[id]/init — register an AVAX-funded bracket.
 * Generates a manage_token (organizer authorization). Subsequent calls require
 * the bearer token.
 */
export async function POST(
  request: Request,
  { params }: { params: Promise<{ id: string }> }
) {
  const { id } = await params;
  if (validateBodySize(request)) return validateBodySize(request)!;
  const tid = Number(id);
  if (!Number.isFinite(tid)) return NextResponse.json({ error: "bad id" }, { status: 400 });

  const store = getStore();

  // If the tournament already exists, require the bearer token (upsert auth).
  const existing = await store.getTournament(tid);
  if (existing) {
    const authed = await requireOrganizer(request, tid);
    if (!authed) return NextResponse.json({ error: "unauthorized" }, { status: 401 });
  }

  const body = (await request.json().catch(() => ({}))) as {
    sponsor?: string;
    prizePoolWei?: string;
    payoutBps?: number[];
    format?: BracketState["format"];
    swissRounds?: number;
    players?: BracketState["players"];
    txHash?: string | null;
  };

  if (!body.payoutBps || !body.format || !Array.isArray(body.players)) {
    return NextResponse.json({ error: "missing fields" }, { status: 400 });
  }

  // Validate payoutBps sums to 10000.
  const sum = body.payoutBps.reduce((a, b) => a + b, 0);
  if (sum !== 10000) return NextResponse.json({ error: "payoutBps must sum to 10000" }, { status: 400 });
  if (body.payoutBps.length > 16) return NextResponse.json({ error: "too many placements" }, { status: 400 });
  if (body.players.length > 1024) return NextResponse.json({ error: "too many players" }, { status: 400 });

  const manageToken = generateManageToken();

  await store.saveTournament({
    tournamentId: tid,
    sponsor: body.sponsor ?? "0x0",
    prizePoolWei: body.prizePoolWei ?? "0",
    token: "0x0000000000000000000000000000000000000000",
    payoutBps: body.payoutBps,
    winnerWallets: [],
    state: "OPEN",
    mode: "bracket",
    manageToken,
    organizerWallet: body.sponsor,
    txHash: body.txHash ?? null,
    createdAt: Date.now(),
  });

  await store.saveBracket(tid, {
    format: body.format,
    swissRounds: body.swissRounds,
    players: body.players,
    results: [],
  });

  return NextResponse.json({ ok: true, manageToken });
}
