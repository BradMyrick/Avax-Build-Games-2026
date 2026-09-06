import Link from "next/link";
import { FileText, ExternalLink, Shield, DollarSign, Gauge, Layers } from "lucide-react";

export const metadata = {
  title: "AMP Contracts — Deployed Addresses & Gas Costs",
  description: "Deployed contract addresses on Avalanche Fuji, gas profiles, fee configuration, and architecture notes.",
};

const contracts = [
  {
    name: "AMPMultiplayer",
    version: "v2 (N-player)",
    address: "0xcabf7b626172fE55d54f03c346563671AbcC77f7",
    desc: "N-player dual-deposit escrow + K-of-N quorum settlement + prove-your-payout claims",
    features: ["2–64 player lobbies", "Stake + reporting bond", "K = ⌊2N/3⌋+1 quorum", "Pull-only payouts"],
  },
  {
    name: "AMPSettlement",
    version: "v1 (1v1)",
    address: "0x78ec93e66255a74873d20DD62C6595A389272126",
    desc: "1v1 settlement with verifier attestation and fee-split router",
    features: ["Studio/protocol fee split", "RT hash-agree mode", "Arbiter disputes"],
  },
  {
    name: "AMPRegistry",
    version: "v1",
    address: "0xf6B0eA6c88c574c4BbEAdC186AAfe72C43C2cDc2",
    desc: "Game registry, escrow holder, verifier whitelisting",
    features: ["Game registration", "Escrow custody", "Verifier management"],
  },
  {
    name: "AMPTournamentCup",
    version: "v1",
    address: "0x7c743c1c9ae3e7a65d030098f2249b7787d66dff",
    desc: "Sponsor-funded tournament prize pools",
    features: ["EIP-712 finalization", "Pull-claim payouts"],
  },
];

const gasTable = [
  { size: "8 players", quorum: "K=6", gas: "152,962", note: "Spec gate: <250k" },
  { size: "16 players", quorum: "K=11", gas: "195,844", note: "" },
  { size: "64 players (BR)", quorum: "K=43", gas: "404,021", note: "" },
];

export default function ContractsPage() {
  return (
    <div className="min-h-screen bg-black text-white antialiased">
      <div className="absolute top-0 -left-1/4 w-[150%] h-[300px] bg-brand-cyan/10 blur-[120px] rounded-full pointer-events-none" />

      <div className="relative z-10 max-w-4xl mx-auto px-6 py-16">
        <div className="mb-12">
          <Link href="/docs" className="text-sm text-zinc-500 hover:text-brand-cyan transition-colors">
            ← Docs
          </Link>
          <div className="inline-flex items-center gap-2 rounded-full border border-brand-cyan/30 bg-brand-cyan/5 px-4 py-1.5 text-xs font-bold uppercase tracking-widest text-brand-cyan mt-6 mb-4">
            <FileText className="w-3.5 h-3.5" />
            On-chain
          </div>
          <h1 className="text-4xl font-black uppercase tracking-tight mb-4">
            Contracts & <span className="text-brand-cyan">Addresses</span>
          </h1>
          <p className="text-lg text-zinc-400 leading-relaxed">
            All contracts deployed on Avalanche Fuji testnet. Sourcify
            verified. Mainnet deployment pending external audit.
          </p>
        </div>

        {/* Contract cards */}
        <div className="space-y-6 mb-12">
          {contracts.map((c) => (
            <div key={c.name} className="glass-panel rounded-3xl border border-white/10 p-6">
              <div className="flex items-start justify-between mb-3 flex-wrap gap-2">
                <div>
                  <h2 className="text-lg font-bold">{c.name}</h2>
                  <span className="text-xs text-zinc-500 uppercase tracking-widest">
                    {c.version}
                  </span>
                </div>
                <a
                  href={`https://testnet.snowtrace.io/address/${c.address}`}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-xs text-brand-cyan hover:underline inline-flex items-center gap-1"
                >
                  Snowtrace <ExternalLink className="w-3 h-3" />
                </a>
              </div>
              <code className="block text-xs font-mono text-brand-cyan bg-black/40 border border-white/10 rounded-lg px-3 py-2 mb-3 overflow-x-auto">
                {c.address}
              </code>
              <p className="text-sm text-zinc-400 mb-3">{c.desc}</p>
              <div className="flex flex-wrap gap-2">
                {c.features.map((f) => (
                  <span
                    key={f}
                    className="text-[10px] font-bold uppercase tracking-wider text-zinc-400 border border-white/10 rounded-full px-3 py-1"
                  >
                    {f}
                  </span>
                ))}
              </div>
            </div>
          ))}
        </div>

        {/* Gas table */}
        <div className="glass-panel rounded-3xl border border-white/10 p-6 mb-12">
          <h2 className="text-xl font-black uppercase tracking-tight mb-4 flex items-center gap-2">
            <Gauge className="w-5 h-5 text-brand-cyan" />
            Settlement Gas (AMPMultiplayer)
          </h2>
          <p className="text-sm text-zinc-400 mb-4">
            Gas scales with signatures, not recipients (prove-your-payout
            pattern: settlement records the ladder; players claim separately).
          </p>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-white/10 text-left text-xs uppercase tracking-widest text-zinc-500">
                  <th className="pb-3 pr-4">Lobby Size</th>
                  <th className="pb-3 pr-4">Quorum</th>
                  <th className="pb-3 pr-4">Gas</th>
                  <th className="pb-3">Note</th>
                </tr>
              </thead>
              <tbody>
                {gasTable.map((g) => (
                  <tr key={g.size} className="border-b border-white/5">
                    <td className="py-3 pr-4 font-mono">{g.size}</td>
                    <td className="py-3 pr-4 font-mono text-brand-cyan">{g.quorum}</td>
                    <td className="py-3 pr-4 font-mono">{g.gas}</td>
                    <td className="py-3 text-zinc-500 text-xs">{g.note}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>

        {/* Fee model */}
        <div className="glass-panel rounded-3xl border border-white/10 p-6 mb-12">
          <h2 className="text-xl font-black uppercase tracking-tight mb-4 flex items-center gap-2">
            <DollarSign className="w-5 h-5 text-brand-cyan" />
            Fee Model
          </h2>
          <div className="space-y-3 text-sm text-zinc-400">
            <div className="flex justify-between border-b border-white/5 pb-2">
              <span>Protocol rake (of gross pool)</span>
              <span className="font-mono text-white">1% (100 bps)</span>
            </div>
            <div className="flex justify-between border-b border-white/5 pb-2">
              <span>Studio share (of rake)</span>
              <span className="font-mono text-white">20% (2000 bps)</span>
            </div>
            <div className="flex justify-between border-b border-white/5 pb-2">
              <span>Max total fees</span>
              <span className="font-mono text-white">8% (800 bps)</span>
            </div>
            <div className="flex justify-between">
              <span>Non-signer bond slash</span>
              <span className="font-mono text-white">50/50 relayer + rank 1</span>
            </div>
          </div>
        </div>

        {/* Security */}
        <div className="glass-panel rounded-3xl border border-white/10 p-6 mb-12">
          <h2 className="text-xl font-black uppercase tracking-tight mb-4 flex items-center gap-2">
            <Shield className="w-5 h-5 text-brand-cyan" />
            Security Properties
          </h2>
          <ul className="space-y-2 text-sm text-zinc-400">
            <li className="flex gap-2">
              <span className="text-brand-cyan">✓</span> Pull-only payouts — settlement never transfers; it credits a claimable ledger
            </li>
            <li className="flex gap-2">
              <span className="text-brand-cyan">✓</span> Checks-effects-interactions on every external call
            </li>
            <li className="flex gap-2">
              <span className="text-brand-cyan">✓</span> ReentrancyGuard + Pausable + Ownable2Step
            </li>
            <li className="flex gap-2">
              <span className="text-brand-cyan">✓</span> Value conservation fuzz-gated (256 runs × 128k calls, to 1 wei)
            </li>
            <li className="flex gap-2">
              <span className="text-brand-cyan">✓</span> Quorum intersection: two conflicting K-quorums provably share ≥ f+1 signers
            </li>
            <li className="flex gap-2">
              <span className="text-brand-cyan">✓</span> Reverting-receiver DoS isolation (pull pattern)
            </li>
            <li className="flex gap-2">
              <span className="text-brand-cyan">✓</span> Immutable payout profiles (claims recompute tiers trustlessly)
            </li>
          </ul>
        </div>

        {/* Architecture note */}
        <div className="glass-panel rounded-3xl border border-brand-cyan/20 p-6">
          <h2 className="text-xl font-black uppercase tracking-tight mb-4 flex items-center gap-2">
            <Layers className="w-5 h-5 text-brand-cyan" />
            v1 vs v2 Architecture
          </h2>
          <p className="text-sm text-zinc-400 leading-relaxed">
            v1 contracts (<code className="text-brand-cyan">AMPRegistry</code>,{" "}
            <code className="text-brand-cyan">AMPSettlement</code>) handle 1v1
            matches with a single verifier attestation. v2 (
            <code className="text-brand-cyan">AMPMultiplayer</code>) extends to
            N players with K-of-N quorum settlement, dual-deposit escrow
            (stake + reporting bond), grace-path timeouts, bonded-verifier
            disputes, and prove-your-payout claims. Both are live on Fuji.
          </p>
        </div>

        <div className="mt-12 text-center">
          <Link href="/docs/faq" className="text-sm text-brand-cyan hover:underline">
            Read the FAQ →
          </Link>
        </div>
      </div>
    </div>
  );
}
