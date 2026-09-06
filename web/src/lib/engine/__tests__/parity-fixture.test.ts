// Temporary vitest harness that runs the generator logic.
import { test } from "vitest";
import { Tournament, Outcome, TournamentState } from "../index";
import { writeFileSync, mkdirSync } from "node:fs";

function mulberry32(a: number) {
  return function () {
    a |= 0; a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

test("generate parity fixtures", () => {
  const cases: any[] = [];
  for (const n of [2, 3, 4, 5, 6, 8, 9, 12, 16]) {
    for (let variant = 0; variant < 4; variant++) {
      const rng = mulberry32(n * 1000 + variant);
      const players = Array.from({ length: n }, (_, i) => ({
        id: i + 1,
        seed: ((i * 7 + variant * 3) % n) + 1,
        wallet: `0x${(((i + 1) * 0x1111) % 0xffff).toString(16).padStart(40, "0")}`,
      }));
      const engine = Tournament.new<number>(
        { kind: TournamentState.SingleElimination },
        players.map((p) => ({ id: p.id, seed: p.seed }))
      );
      const results: { matchId: number; outcome: string }[] = [];
      for (let guard = 0; guard < 10000 && !engine.isComplete(); guard++) {
        const pending = engine.pending();
        if (!pending.length) break;
        const id = pending[Math.floor(rng() * pending.length)];
        const outcome = rng() < 0.5 ? Outcome.A : Outcome.B;
        engine.record(id, outcome);
        results.push({ matchId: id, outcome: outcome === Outcome.A ? "A" : "B" });
      }
      if (!engine.isComplete()) throw new Error(`incomplete n=${n} v=${variant}`);
      const winnerWallets = engine
        .winners()
        .map((id) => players.find((p) => p.id === id)?.wallet ?? "");
      cases.push({ n, variant, players, results, winnerWallets });
    }
  }
  mkdirSync("../relayer/tests/fixtures", { recursive: true });
  writeFileSync(
    "../relayer/tests/fixtures/single_elim_parity.json",
    JSON.stringify({ cases }, null, 1)
  );
});
