import Link from "next/link";
import { HelpCircle, ChevronDown } from "lucide-react";

export const metadata = {
  title: "AMP FAQ — Frequently Asked Questions",
  description: "Answers about AMP matchmaking: staking, ratings, disputes, fees, and how the protocol handles edge cases.",
};

const faqs = [
  {
    q: "What is AMP?",
    a: "AMP (Avalanche Matchmaking Protocol) is a decentralized multiplayer matchmaking layer. Players queue by skill rating, optionally stake cryptocurrency, play their match, and settle the result through cryptographic quorum signatures on Avalanche. No central operator is needed to hold funds or verify outcomes.",
  },
  {
    q: "How does matchmaking work?",
    a: "Players enter a queue with their Glicko-2 skill rating. The matchmaker pairs players within an expanding skill window — tight matches first, fair matches eventually. For N-player lobbies (FFA, battle royale), a commit-reveal phase with a blockhash-seeded shuffle prevents pre-coordinated groups from targeting the same lobby.",
  },
  {
    q: "What is Glicko-2 and why does it matter?",
    a: "Glicko-2 is a rating system that tracks both skill (rating) and uncertainty (rating deviation). New players start uncertain and converge quickly. AMP's implementation is order-independent (any permutation of opponents yields bit-identical results, fuzz-gated), making it safe for distributed execution.",
  },
  {
    q: "Do I need to stake money to play?",
    a: "No. Free play is the default — no wallet gas, no transaction, just a free signature to log in. Staked matches (where players deposit AVAX into escrow) are opt-in for competitive modes.",
  },
  {
    q: "How does staked settlement work?",
    a: "Each player deposits a stake (prize pool) plus a reporting bond. After the match, players sign the result with EIP-712 typed data. When K = ⌊2N/3⌋+1 players submit identical results, the match settles on-chain: stakes pay out per the tier structure, reporting bonds return to signers, and non-signers' bonds are slashed 50/50 to the relayer and rank 1.",
  },
  {
    q: "What happens if someone lies about the result?",
    a: "Two cases: (1) Both players report different results → the match enters dispute and a bonded verifier resolves it. The dishonest party forfeits their stake, bond, and challenge deposit. (2) A player goes silent → the reporter's result stands after a timeout. Non-signers forfeit their reporting bond.",
  },
  {
    q: "How are ratings protected from smurfing?",
    a: "Party members get γ-anti-boost recalibration: a lower-rated player's rating gain from a party win is scaled down proportionally to the rating gap. A high-rated player cannot carry a smurf account to free rating.",
  },
  {
    q: "What is a 'reporting bond'?",
    a: "A small deposit (typically 5% of the stake) held in escrow during the match. You get it back when you submit your signed result. If you disconnect without signing, the bond is slashed. This makes honest reporting incentive-compatible even for losing players.",
  },
  {
    q: "What games can use AMP?",
    a: "Any game with a deterministic or verifiable outcome. The protocol needs: (1) a clear win/loss/draw result, (2) optionally a transcript hash (deterministic replay hash for anti-cheat), and (3) a client that can sign EIP-712 messages. Board games, card games, strategy games, and competitive shooters are all fits.",
  },
  {
    q: "How do I integrate AMP into my game?",
    a: "Four API calls and a WebSocket connection. See the Quickstart guide. For Rust game servers, you can also embed the `amp-match-core` crate directly (Glicko-2, queue algorithms, commit-reveal — zero dependencies beyond serde).",
  },
  {
    q: "What are the fees?",
    a: "The protocol takes a configurable rake (currently 1%) on settled staked matches. Studios that register their game receive a share (currently 20% of the rake). Free play costs nothing. Total fees are capped at 8%.",
  },
  {
    q: "Is AMP live on mainnet?",
    a: "Not yet. All contracts are deployed and verified on Avalanche Fuji testnet. Mainnet deployment follows an external audit and timelock governance setup. Free-play matchmaking (no funds at risk) is live in production now.",
  },
  {
    q: "What happens if the matchmaker server goes down?",
    a: "Free-play matches: your ratings are safe (persisted in Postgres). No funds are at risk. Staked matches: funds are in on-chain escrow, not the server. If the server disappears, the grace path allows rank-1 claimants to settle unilaterally after a timeout. The smart contracts are the security boundary, not the server.",
  },
  {
    q: "How does AMP prevent collusion in FFA lobbies?",
    a: "Commit-reveal: players submit a blinded hash of their identity before the lobby forms. After enough players commit, they reveal their salts. The lobby assignment is then shuffled using the latest Avalanche blockhash as the seed — no one can predict which lobby they'll land in until the block is mined.",
  },
  {
    q: "Can I run my own matchmaker?",
    a: "Yes. The amp-server is open source (Apache-2.0). You can run it with your own Postgres and register your game's verifiers on the contract. Or embed `amp-match-core` in your game server for the algorithms without the service.",
  },
];

export default function FaqPage() {
  return (
    <div className="min-h-screen bg-black text-white antialiased">
      <div className="absolute top-0 -left-1/4 w-[150%] h-[300px] bg-brand-cyan/10 blur-[120px] rounded-full pointer-events-none" />

      <div className="relative z-10 max-w-3xl mx-auto px-6 py-16">
        <div className="mb-12 text-center">
          <Link href="/docs" className="text-sm text-zinc-500 hover:text-brand-cyan transition-colors">
            ← Docs
          </Link>
          <div className="inline-flex items-center gap-2 rounded-full border border-brand-cyan/30 bg-brand-cyan/5 px-4 py-1.5 text-xs font-bold uppercase tracking-widest text-brand-cyan mt-6 mb-4">
            <HelpCircle className="w-3.5 h-3.5" />
            FAQ
          </div>
          <h1 className="text-4xl font-black uppercase tracking-tight mb-4">
            Frequently Asked <span className="text-brand-cyan">Questions</span>
          </h1>
        </div>

        <div className="space-y-3">
          {faqs.map((faq, i) => (
            <details
              key={i}
              className="glass-panel rounded-2xl border border-white/10 overflow-hidden group"
            >
              <summary className="flex items-center justify-between cursor-pointer px-6 py-4 hover:bg-white/5 transition-colors list-none">
                <span className="font-bold text-sm md:text-base">{faq.q}</span>
                <ChevronDown className="w-4 h-4 text-zinc-500 shrink-0 ml-4 group-open:rotate-180 transition-transform" />
              </summary>
              <div className="px-6 pb-5 pt-0">
                <p className="text-sm text-zinc-400 leading-relaxed">{faq.a}</p>
              </div>
            </details>
          ))}
        </div>

        <div className="mt-12 text-center space-y-2">
          <p className="text-sm text-zinc-500">Still have questions?</p>
          <Link href="/docs/quickstart" className="text-sm text-brand-cyan hover:underline">
            Read the Quickstart →
          </Link>
          <br />
          <a
            href="https://github.com/BradMyrick/Avalanche-Matchmaking-Protocol"
            target="_blank"
            rel="noopener noreferrer"
            className="text-sm text-zinc-500 hover:text-brand-cyan transition-colors"
          >
            Browse the source on GitHub →
          </a>
        </div>
      </div>
    </div>
  );
}
