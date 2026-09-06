import Link from "next/link";
import {
  BookOpen,
  Rocket,
  Code2,
  FileText,
  HelpCircle,
  ArrowRight,
  Terminal,
  Shield,
  Users,
  Zap,
  Globe,
} from "lucide-react";

export const metadata = {
  title: "AMP Documentation — Build Multiplayer Games on Avalanche",
  description:
    "Everything you need to integrate AMP as your multiplayer protocol: quickstart guides, API reference, contract addresses, and architecture.",
};

const sections = [
  {
    href: "/docs/quickstart",
    icon: Rocket,
    title: "Quickstart",
    desc: "Connect your game to AMP in 5 minutes. Wallet login, queue, match, settle.",
    tag: "Start here",
  },
  {
    href: "/docs/api",
    icon: Code2,
    title: "API Reference",
    desc: "REST + WebSocket endpoints, EIP-712 typed data, and the embeddable Rust core.",
    tag: "Reference",
  },
  {
    href: "/docs/contracts",
    icon: FileText,
    title: "Contracts & Addresses",
    desc: "Deployed addresses on Fuji, ABI highlights, gas costs, and fee configuration.",
    tag: "On-chain",
  },
  {
    href: "/docs/faq",
    icon: HelpCircle,
    title: "FAQ",
    desc: "Common questions about staking, ratings, disputes, and how AMP handles edge cases.",
    tag: "Answers",
  },
];

const features = [
  {
    icon: Users,
    title: "N-Player Matchmaking",
    desc: "Parties, teams, FFA (4–16), and battle royale (16–64). Glicko-2 skill ratings with anti-boost.",
  },
  {
    icon: Shield,
    title: "Trustless Settlement",
    desc: "K-of-N quorum signatures settle results on Avalanche. No oracle, no operator trust required.",
  },
  {
    icon: Zap,
    title: "Sub-Second Queue",
    desc: "100ms in-memory tick with expanding skill windows. Commit-reveal prevents lobby targeting.",
  },
  {
    icon: Globe,
    title: "Cross-Game Identity",
    desc: "One wallet, one rating graph. Portable EIP-712 skill attestations that follow the player.",
  },
];

export default function DocsHub() {
  return (
    <div className="min-h-screen bg-black text-white antialiased">
      <div className="absolute top-0 -left-1/4 w-[150%] h-[400px] bg-brand-cyan/10 blur-[120px] rounded-full pointer-events-none" />
      <div className="absolute bottom-0 -right-1/4 w-[150%] h-[400px] bg-brand-red/10 blur-[120px] rounded-full pointer-events-none" />

      <div className="relative z-10 max-w-6xl mx-auto px-6 py-16">
        {/* Header */}
        <div className="text-center mb-16">
          <div className="inline-flex items-center gap-2 rounded-full border border-brand-cyan/30 bg-brand-cyan/5 px-4 py-1.5 text-xs font-bold uppercase tracking-widest text-brand-cyan mb-6">
            <BookOpen className="w-3.5 h-3.5" />
            Documentation
          </div>
          <h1 className="text-4xl md:text-5xl font-black uppercase tracking-tight mb-4">
            Build with <span className="text-brand-cyan">AMP</span>
          </h1>
          <p className="max-w-2xl mx-auto text-lg text-zinc-400 leading-relaxed">
            AMP is the multiplayer protocol for games on Avalanche. Players
            queue by skill, stake to win, and settle results through
            cryptographic quorum. Integrate in minutes — no backend required.
          </p>
        </div>

        {/* Doc Cards */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-5 mb-16">
          {sections.map((s) => (
            <Link
              key={s.href}
              href={s.href}
              className="group glass-panel rounded-3xl border border-white/10 hover:border-brand-cyan/40 p-8 transition-all hover:-translate-y-1"
            >
              <div className="flex items-start justify-between mb-4">
                <div className="w-12 h-12 rounded-2xl bg-brand-cyan/10 border border-brand-cyan/20 flex items-center justify-center group-hover:scale-110 transition-transform">
                  <s.icon className="w-6 h-6 text-brand-cyan" />
                </div>
                <span className="text-[10px] font-black uppercase tracking-widest text-zinc-500 border border-zinc-700 rounded-full px-3 py-1">
                  {s.tag}
                </span>
              </div>
              <h2 className="text-xl font-bold mb-2 group-hover:text-brand-cyan transition-colors">
                {s.title}
              </h2>
              <p className="text-sm text-zinc-400 leading-relaxed">{s.desc}</p>
              <div className="mt-4 inline-flex items-center gap-1 text-sm text-brand-cyan opacity-0 group-hover:opacity-100 transition-opacity">
                Read <ArrowRight className="w-4 h-4" />
              </div>
            </Link>
          ))}
        </div>

        {/* What AMP provides */}
        <div className="mb-16">
          <h2 className="text-2xl font-black uppercase tracking-tight text-center mb-8">
            What AMP gives your game
          </h2>
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
            {features.map((f) => (
              <div
                key={f.title}
                className="glass-panel rounded-2xl border border-white/10 p-6"
              >
                <f.icon className="w-8 h-8 text-brand-cyan mb-4" />
                <h3 className="font-bold text-sm uppercase tracking-wide mb-2">
                  {f.title}
                </h3>
                <p className="text-xs text-zinc-400 leading-relaxed">{f.desc}</p>
              </div>
            ))}
          </div>
        </div>

        {/* Architecture at a glance */}
        <div className="glass-panel rounded-3xl border border-white/10 p-8 mb-16">
          <h2 className="text-xl font-black uppercase tracking-tight mb-6 flex items-center gap-2">
            <Terminal className="w-5 h-5 text-brand-cyan" />
            Architecture at a glance
          </h2>
          <div className="grid grid-cols-1 lg:grid-cols-3 gap-6 text-sm">
            <div>
              <div className="font-bold text-brand-cyan mb-2 uppercase text-xs tracking-widest">
                Your Game (Client)
              </div>
              <ul className="space-y-1.5 text-zinc-400">
                <li>• Wallet login (one free EIP-191 signature)</li>
                <li>• Queue via REST API</li>
                <li>• Receive match assignments via WebSocket</li>
                <li>• Report results with EIP-712 signatures</li>
              </ul>
            </div>
            <div>
              <div className="font-bold text-brand-cyan mb-2 uppercase text-xs tracking-widest">
                AMP Server (Rust)
              </div>
              <ul className="space-y-1.5 text-zinc-400">
                <li>• Glicko-2 skill ratings, expanding windows</li>
                <li>• Party/team/FFA/BR lobby formation</li>
                <li>• K-of-N quorum collection (120s window)</li>
                <li>• Commit-reveal anti-collusion</li>
              </ul>
            </div>
            <div>
              <div className="font-bold text-brand-cyan mb-2 uppercase text-xs tracking-widest">
                Avalanche (On-chain)
              </div>
              <ul className="space-y-1.5 text-zinc-400">
                <li>• Dual-deposit escrow (stake + bond)</li>
                <li>• Verifier-attested settlement</li>
                <li>• Pull-only payouts (claimable ledger)</li>
                <li>• Studio/protocol fee split (≤8%)</li>
              </ul>
            </div>
          </div>
        </div>

        {/* Embed the core */}
        <div className="glass-panel rounded-3xl border border-brand-cyan/20 p-8 text-center">
          <h2 className="text-xl font-black uppercase tracking-tight mb-3">
            Or embed the matchmaking core directly
          </h2>
          <p className="text-sm text-zinc-400 max-w-2xl mx-auto mb-6">
            Don&apos;t want to run a server? Pull in{" "}
            <code className="text-brand-cyan">amp-match-core</code> — a
            zero-dependency Rust crate with the Glicko-2 math, rule engine,
            and queue algorithms. Your game, your process.
          </p>
          <pre className="text-left inline-block bg-black/60 border border-white/10 rounded-2xl px-6 py-4 text-sm font-mono">
            <code className="text-zinc-300">{`[dependencies]
amp-match-core = "0.1"`}</code>
          </pre>
        </div>

        {/* Footer link back */}
        <div className="text-center mt-12">
          <Link
            href="/"
            className="text-sm text-zinc-500 hover:text-brand-cyan transition-colors"
          >
            ← Back to playwithamp.xyz
          </Link>
        </div>
      </div>
    </div>
  );
}
