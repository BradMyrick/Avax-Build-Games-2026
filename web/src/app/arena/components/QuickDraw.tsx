"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { Zap, Trophy, Frown, Shield, Timer } from "lucide-react";

/**
 * QuickDraw — a reaction timing duel.
 *
 * 5 rounds. Each round: wait for the signal, click as fast as possible.
 * False start = instant round loss. First to 3 round wins takes the match.
 *
 * The bot has realistic human reaction times (230–380ms with occasional
 * slow rounds and rare false starts) — it's beatable but not trivially.
 */

type RoundState = "waiting" | "signal" | "result" | "matchOver";
type RoundResult = { playerMs: number | null; botMs: number | null; winner: "player" | "bot" };

const ROUNDS_TO_WIN = 3;
const TOTAL_ROUNDS = 5;
const SIGNAL_DELAY_MIN = 1200; // ms before the signal fires
const SIGNAL_DELAY_MAX = 4000;
const BOT_REACTION_MIN = 230;
const BOT_REACTION_MAX = 380;
const BOT_SLOW_CHANCE = 0.2; // 20% of rounds the bot is slow (400-550ms)
const BOT_FALSE_START_CHANCE = 0.05; // 5% chance the bot false-starts

function randomBetween(min: number, max: number) {
  return Math.random() * (max - min) + min;
}

export interface QuickDrawProps {
  opponentName: string;
  isBot: boolean;
  onFinish: (result: { won: boolean; roundsWon: number; avgReactionMs: number }) => void;
}

export default function QuickDraw({ opponentName, isBot, onFinish }: QuickDrawProps) {
  const [round, setRound] = useState(0);
  const [roundState, setRoundState] = useState<RoundState>("waiting");
  const [rounds, setRounds] = useState<RoundResult[]>([]);
  const [lastRound, setLastRound] = useState<RoundResult | null>(null);
  const [falseStart, setFalseStart] = useState(false);
  const [reactionTimes, setReactionTimes] = useState<number[]>([]);

  const signalTime = useRef<number>(0);
  const clickTime = useRef<number>(0);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const clickHandlerRef = useRef<(() => void) | null>(null);

  const playerWins = rounds.filter((r) => r.winner === "player").length;
  const botWins = rounds.filter((r) => r.winner === "bot").length;
  const matchOver = playerWins >= ROUNDS_TO_WIN || botWins >= ROUNDS_TO_WIN || rounds.length >= TOTAL_ROUNDS;
  const playerWon = playerWins > botWins;

  // Generate the bot's reaction time for a round
  const genBotTime = useCallback((): { ms: number | null; falseStart: boolean } => {
    if (Math.random() < BOT_FALSE_START_CHANCE) {
      return { ms: null, falseStart: true };
    }
    if (Math.random() < BOT_SLOW_CHANCE) {
      return { ms: Math.round(randomBetween(400, 550)), falseStart: false };
    }
    return { ms: Math.round(randomBetween(BOT_REACTION_MIN, BOT_REACTION_MAX)), falseStart: false };
  }, []);

  // Start a round
  const startRound = useCallback(() => {
    if (matchOver) return;
    setRound((r) => r + 1);
    setRoundState("waiting");
    setFalseStart(false);
    setLastRound(null);

    const delay = randomBetween(SIGNAL_DELAY_MIN, SIGNAL_DELAY_MAX);
    timeoutRef.current = setTimeout(() => {
      signalTime.current = performance.now();
      setRoundState("signal");
    }, delay);
  }, [matchOver]);

  // Handle the player's click
  const handleClick = useCallback(() => {
    if (roundState === "waiting") {
      // FALSE START
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
      setFalseStart(true);
      setRoundState("result");

      const bot = genBotTime();
      const result: RoundResult = {
        playerMs: null,
        botMs: bot.falseStart ? null : bot.ms,
        winner: "bot", // player false-started = bot wins (even if bot also would have false-started)
      };
      setLastRound(result);
      setRounds((prev) => [...prev, result]);

      const t = setTimeout(() => {
        if (rounds.length + 1 < TOTAL_ROUNDS && playerWins < ROUNDS_TO_WIN && botWins + 1 < ROUNDS_TO_WIN) {
          startRound();
        } else {
          setRoundState("matchOver");
        }
      }, 2000);
      timeoutRef.current = t;
      return;
    }

    if (roundState === "signal") {
      // VALID CLICK
      clickTime.current = performance.now();
      const playerMs = Math.round(clickTime.current - signalTime.current);
      const bot = genBotTime();
      setReactionTimes((prev) => [...prev, playerMs]);

      const botWon = bot.falseStart ? false : bot.ms !== null && bot.ms < playerMs;
      const result: RoundResult = {
        playerMs,
        botMs: bot.falseStart ? null : bot.ms,
        winner: botWon ? "bot" : "player",
      };

      setLastRound(result);
      setRounds((prev) => [...prev, result]);
      setRoundState("result");

      const newPlayerWins = playerWins + (result.winner === "player" ? 1 : 0);
      const newBotWins = botWins + (result.winner === "bot" ? 1 : 0);

      const t = setTimeout(() => {
        if (newPlayerWins >= ROUNDS_TO_WIN || newBotWins >= ROUNDS_TO_WIN || rounds.length + 1 >= TOTAL_ROUNDS) {
          setRoundState("matchOver");
        } else {
          startRound();
        }
      }, 2200);
      timeoutRef.current = t;
    }
  }, [roundState, genBotTime, rounds.length, playerWins, botWins, startRound]);

  // Start the first round on mount (async to avoid cascading render lint)
  useEffect(() => {
    const t = setTimeout(() => startRound(), 100);
    return () => {
      clearTimeout(t);
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Report the final result
  useEffect(() => {
    if (roundState === "matchOver" && rounds.length > 0) {
      const avg = reactionTimes.length > 0
        ? Math.round(reactionTimes.reduce((a, b) => a + b, 0) / reactionTimes.length)
        : 0;
      const t = setTimeout(() => {
        onFinish({ won: playerWon, roundsWon: playerWins, avgReactionMs: avg });
      }, 1500);
      return () => clearTimeout(t);
    }
  }, [roundState, rounds, playerWon, playerWins, reactionTimes, onFinish]);

  // Register/updated the click handler based on state
  useEffect(() => {
    clickHandlerRef.current = handleClick;
  }, [handleClick]);

  const bgColor =
    roundState === "signal"
      ? "bg-emerald-500"
      : roundState === "waiting"
        ? "bg-red-900/80"
        : roundState === "result"
          ? falseStart
            ? "bg-orange-900/60"
            : lastRound?.winner === "player"
              ? "bg-emerald-900/40"
              : "bg-rose-900/40"
          : "bg-black";

  return (
    <div className="fixed inset-0 z-50 select-none" onClick={() => clickHandlerRef.current?.()}>
      {/* Game area */}
      <div
        className={`absolute inset-0 transition-colors duration-150 ${bgColor}`}
        style={{ cursor: roundState === "signal" ? "crosshair" : roundState === "waiting" ? "not-allowed" : "default" }}
      >
        {/* HUD */}
        <div className="absolute top-0 left-0 right-0 p-4 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <span className={`text-sm font-bold px-3 py-1 rounded-full ${playerWins > botWins ? "bg-emerald-500/20 text-emerald-300" : "bg-white/10 text-zinc-400"}`}>
              YOU {playerWins}
            </span>
            <span className="text-zinc-600 text-xs">vs</span>
            <span className={`text-sm font-bold px-3 py-1 rounded-full ${botWins > playerWins ? "bg-rose-500/20 text-rose-300" : "bg-white/10 text-zinc-400"}`}>
              {isBot ? "BOT" : opponentName.slice(0, 6)} {botWins}
            </span>
          </div>
          <span className="text-xs text-zinc-500 font-mono">
            Round {Math.min(round, TOTAL_ROUNDS)}/{TOTAL_ROUNDS}
          </span>
        </div>

        {/* Center content */}
        <div className="absolute inset-0 flex flex-col items-center justify-center pointer-events-none">
          {roundState === "waiting" && (
            <>
              <div className="text-6xl md:text-8xl font-black text-red-300/70 tracking-tight mb-4 animate-pulse">
                WAIT
              </div>
              <p className="text-red-200/50 text-sm">
                Click when the screen turns green
              </p>
              <p className="text-red-200/30 text-xs mt-2">
                False start = round loss
              </p>
            </>
          )}

          {roundState === "signal" && (
            <>
              <Zap className="w-20 h-20 md:w-28 md:h-28 text-white mb-4 drop-shadow-[0_0_30px_rgba(255,255,255,0.5)]" />
              <div className="text-6xl md:text-8xl font-black text-white tracking-tighter">
                CLICK!
              </div>
            </>
          )}

          {roundState === "result" && lastRound && (
            <div className="text-center">
              {falseStart ? (
                <>
                  <Frown className="w-16 h-16 text-orange-400 mx-auto mb-3" />
                  <div className="text-4xl font-black text-orange-300 mb-2">FALSE START</div>
                  <p className="text-orange-200/60 text-sm">Round lost</p>
                </>
              ) : (
                <>
                  <div
                    className={`text-4xl md:text-6xl font-black mb-4 ${
                      lastRound.winner === "player" ? "text-emerald-300" : "text-rose-300"
                    }`}
                  >
                    {lastRound.winner === "player" ? "YOU WIN!" : "OPPONENT WINS"}
                  </div>
                  <div className="flex items-center justify-center gap-8 text-sm font-mono">
                    <div className="text-center">
                      <div className="text-zinc-500 text-xs uppercase tracking-widest mb-1">You</div>
                      <div className={`text-2xl font-bold ${lastRound.winner === "player" ? "text-emerald-400" : "text-zinc-400"}`}>
                        {lastRound.playerMs !== null ? `${lastRound.playerMs}ms` : "—"}
                      </div>
                    </div>
                    <div className="text-zinc-700 text-xl">vs</div>
                    <div className="text-center">
                      <div className="text-zinc-500 text-xs uppercase tracking-widest mb-1">
                        {isBot ? "Bot" : "Opp"}
                      </div>
                      <div className={`text-2xl font-bold ${lastRound.winner === "bot" ? "text-rose-400" : "text-zinc-400"}`}>
                        {lastRound.botMs !== null ? `${lastRound.botMs}ms` : "F/S"}
                      </div>
                    </div>
                  </div>
                </>
              )}
            </div>
          )}

          {roundState === "matchOver" && (
            <div className="text-center">
              {playerWon ? (
                <>
                  <Trophy className="w-16 h-16 md:w-20 md:h-20 text-yellow-400 mx-auto mb-4 drop-shadow-[0_0_20px_rgba(250,204,21,0.3)]" />
                  <div className="text-4xl md:text-6xl font-black text-yellow-300 tracking-tight mb-2">
                    VICTORY
                  </div>
                  <div className="text-lg text-zinc-400">
                    {playerWins}–{botWins} · avg {reactionTimes.length > 0 ? Math.round(reactionTimes.reduce((a, b) => a + b, 0) / reactionTimes.length) : 0}ms
                  </div>
                </>
              ) : (
                <>
                  <Shield className="w-16 h-16 md:w-20 md:h-20 text-zinc-500 mx-auto mb-4" />
                  <div className="text-4xl md:text-6xl font-black text-zinc-400 tracking-tight mb-2">
                    DEFEAT
                  </div>
                  <div className="text-lg text-zinc-500">
                    {playerWins}–{botWins} · avg {reactionTimes.length > 0 ? Math.round(reactionTimes.reduce((a, b) => a + b, 0) / reactionTimes.length) : 0}ms
                  </div>
                </>
              )}
              <p className="text-xs text-zinc-600 mt-6 flex items-center justify-center gap-1">
                <Timer className="w-3 h-3" />
                Reporting result…
              </p>
            </div>
          )}
        </div>

        {/* Round indicator dots */}
        <div className="absolute bottom-8 left-1/2 -translate-x-1/2 flex gap-3">
          {Array.from({ length: TOTAL_ROUNDS }).map((_, i) => {
            const r = rounds[i];
            return (
              <div
                key={i}
                className={`w-3 h-3 rounded-full transition-colors ${
                  r
                    ? r.winner === "player"
                      ? "bg-emerald-400"
                      : "bg-rose-400"
                    : i === rounds.length
                      ? "bg-white/50 animate-pulse"
                      : "bg-white/10"
                }`}
              />
            );
          })}
        </div>
      </div>
    </div>
  );
}
