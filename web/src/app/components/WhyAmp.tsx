"use client";

import { Bot, FileLock2, Fingerprint, Swords, CalendarClock, Layers } from "lucide-react";

/**
 * The three honest questions any matchmaking protocol has to answer —
 * asked out loud, answered with what ships today.
 */
export default function WhyAmp() {
  const pillars = [
    {
      id: "cold-start",
      icon: Bot,
      accent: "text-yellow-400",
      border: "border-yellow-400/30",
      glow: "bg-yellow-400/10",
      title: "Empty lobbies? We planned for that",
      question: "The cold-start liquidity trap: peer-to-peer matchmaking stalls without concurrent players.",
      points: [
        {
          icon: Bot,
          text: "Practice-bot fill: wait past the threshold and the house opponent picks you up — instant play, zero rating impact, nobody bounces off an empty queue.",
        },
        {
          icon: CalendarClock,
          text: "Prime-time queue windows concentrate concurrent players into scheduled blocks instead of spreading them across 24 dead hours.",
        },
        {
          icon: Fingerprint,
          text: "Free-first funnel: login is one gasless signature and ranked play costs nothing — the widest possible top of the ladder.",
        },
      ],
    },
    {
      id: "oracle",
      icon: FileLock2,
      accent: "text-brand-cyan",
      border: "border-brand-cyan/30",
      glow: "bg-brand-cyan/10",
      title: "Who says who won? You both do — cryptographically",
      question: "Oracle-free outcome attestation: verifying results without central custody or costly disputes.",
      points: [
        {
          icon: FileLock2,
          text: "Every result report is signed by the player's own wallet (EIP-191) — non-repudiable evidence, verifiable by anyone, forever.",
        },
        {
          icon: Layers,
          text: "Both players submit a match transcript hash; matching hashes strengthen agreement, mismatched hashes trigger dispute — multi-party session hashing, not trust.",
        },
        {
          icon: Swords,
          text: "Settlement on Avalanche: the contract's hash-agreement mode lets players settle staked matches directly on-chain with no operator in the path; verifier attestations cover the rest.",
        },
      ],
    },
    {
      id: "moat",
      icon: Fingerprint,
      accent: "text-brand-red",
      border: "border-brand-red/30",
      glow: "bg-brand-red/10",
      title: "Why not just settle privately?",
      question: "Fee extraction vs. value add: a bare rake on 1v1 wagers gets bypassed. AMP's rake buys infrastructure.",
      points: [
        {
          icon: Fingerprint,
          text: "Cross-game identity & MMR: one wallet, one rating graph, portable EIP-712 skill attestations that follow you between games and studios.",
        },
        {
          icon: Swords,
          text: "Ranked integrity you can't self-host alone: opponent pool, griefing arbitration, signed-report evidence, and dispute handling that just works.",
        },
        {
          icon: Layers,
          text: "The full toolkit: Glicko-2 matchmaker + embeddable core library, automated brackets and escrowed prize cups, and settlement rails on Avalanche. The rake funds the ladder — the ladder is why players stay.",
        },
      ],
    },
  ];

  return (
    <section id="why-amp" className="py-32 max-w-7xl mx-auto px-6 scroll-mt-32 relative">
      <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[120%] h-[300px] bg-brand-red/5 blur-[150px] rounded-full pointer-events-none" />

      <div className="relative z-10 mb-20 text-center">
        <p className="text-sm font-bold uppercase tracking-[0.3em] text-zinc-400 mb-4">
          The three hard problems
        </p>
        <h2 className="text-4xl md:text-6xl font-black uppercase tracking-tight mb-6">
          Matchmaking protocols die of{" "}
          <span className="text-brand-red">three things</span>
        </h2>
        <p className="max-w-2xl mx-auto text-lg text-zinc-400">
          We designed for them from day one — not as roadmap promises, as
          shipped mechanics.
        </p>
      </div>

      <div className="relative z-10 grid grid-cols-1 lg:grid-cols-3 gap-6 text-left">
        {pillars.map((p) => (
          <div
            key={p.id}
            className={`glass-panel p-8 rounded-3xl border ${p.border} hover:${p.glow} transition-colors group`}
          >
            <div className="flex flex-col gap-2 mb-6">
              <p.icon className={`w-10 h-10 ${p.accent} mb-2`} />
              <h3 className="text-xl font-bold leading-snug">{p.title}</h3>
              <p className="text-sm text-zinc-500 italic">{p.question}</p>
            </div>
            <ul className="space-y-4">
              {p.points.map((pt, i) => (
                <li key={i} className="flex gap-3 text-sm text-zinc-300 leading-relaxed">
                  <pt.icon className={`w-4 h-4 mt-0.5 shrink-0 ${p.accent}`} />
                  <span>{pt.text}</span>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>
    </section>
  );
}
