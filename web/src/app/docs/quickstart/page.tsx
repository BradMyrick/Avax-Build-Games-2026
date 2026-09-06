import Link from "next/link";
import { Check, Copy, Rocket, ArrowRight, Terminal, Key, Swords, Trophy, Zap } from "lucide-react";

export const metadata = {
  title: "AMP Quickstart — Connect Your Game in 5 Minutes",
  description: "Step-by-step guide to integrate AMP matchmaking: wallet login, queue, match, report, settle.",
};

const steps = [
  {
    icon: Key,
    title: "Connect a Wallet",
    code: `// 1. Request a challenge from the matchmaker
const res = await fetch(\`\${AMP_SERVER_URL}/v1/auth/challenge\`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ wallet: userAddress }),
});
const { challenge } = await res.json();

// 2. Ask the player to sign it (free — no gas, no transaction)
const signature = await window.ethereum.request({
  method: "personal_sign",
  params: [toHex(challenge), userAddress],
});

// 3. Exchange the signature for a session token
const login = await fetch(\`\${AMP_SERVER_URL}/v1/auth/verify\`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ wallet: userAddress, signature, challenge }),
});
const { token } = await login.json();

// Store the token — it's valid for 7 days
localStorage.setItem("amp_token", token);`,
    note: "One signature. No gas. No transaction. The player signs a human-readable message that proves wallet ownership.",
  },
  {
    icon: Zap,
    title: "Join the Queue",
    code: `// Join a ranked queue (free play)
const queue = await fetch(\`\${AMP_SERVER_URL}/v1/queue/join\`, {
  method: "POST",
  headers: {
    "Content-Type": "application/json",
    Authorization: \`Bearer \${token}\`,
  },
  body: JSON.stringify({
    gameId: "amp-tactics",
    rulesetId: "ranked-1v1",
  }),
});
const { ticketId, queueDepth, skillWindow } = await queue.json();`,
    note: "The matchmaker pairs players by Glicko-2 skill rating with an expanding window — tight matches first, fair matches eventually.",
  },
  {
    icon: Swords,
    title: "Receive Your Match",
    code: `// Connect the WebSocket for real-time events
const ws = new WebSocket(
  \`\${AMP_SERVER_URL.replace("http", "ws")}/v1/ws?token=\${token}\`
);

ws.onmessage = (event) => {
  const { type, data } = JSON.parse(event.data);
  
  if (type === "match_found") {
    // data.matchId, data.opponent.wallet, data.opponent.rating
    // data.expiresAt — report your result before this
    startMatch(data);
  }
  
  if (type === "queue_status") {
    // data.depth, data.waitedMs, data.skillWindow
    updateQueueUI(data);
  }
};`,
    note: "The WebSocket pushes match assignments, queue status, and results in real time. No polling required.",
  },
  {
    icon: Trophy,
    title: "Report the Result",
    code: `// Both players confirm the outcome
await fetch(\`\${AMP_SERVER_URL}/v1/matches/\${matchId}/report\`, {
  method: "POST",
  headers: {
    "Content-Type": "application/json",
    Authorization: \`Bearer \${token}\`,
  },
  body: JSON.stringify({
    result: "win", // "win" | "loss" | "draw"
    transcriptHash: gameHash, // optional: deterministic replay hash
    signature: signedResult, // optional: EIP-191 for staked matches
  }),
});

// If both players agree → instant settlement
// If they disagree → operator arbitration
// If one goes silent → the reporter's result stands`,
    note: "Two agreeing reports settle instantly. The EIP-712 attestation is portable — anyone can verify the match result on-chain.",
  },
];

export default function QuickstartPage() {
  return (
    <div className="min-h-screen bg-black text-white antialiased">
      <div className="absolute top-0 -left-1/4 w-[150%] h-[300px] bg-brand-cyan/10 blur-[120px] rounded-full pointer-events-none" />

      <div className="relative z-10 max-w-4xl mx-auto px-6 py-16">
        {/* Header */}
        <div className="mb-12">
          <Link
            href="/docs"
            className="text-sm text-zinc-500 hover:text-brand-cyan transition-colors"
          >
            ← Docs
          </Link>
          <div className="inline-flex items-center gap-2 rounded-full border border-brand-cyan/30 bg-brand-cyan/5 px-4 py-1.5 text-xs font-bold uppercase tracking-widest text-brand-cyan mt-6 mb-4">
            <Rocket className="w-3.5 h-3.5" />
            Quickstart
          </div>
          <h1 className="text-4xl font-black uppercase tracking-tight mb-4">
            Connect Your Game in <span className="text-brand-cyan">5 Minutes</span>
          </h1>
          <p className="text-lg text-zinc-400 leading-relaxed">
            Four API calls and a WebSocket connection. That&apos;s the entire
            integration. AMP handles the matchmaking, ratings, escrow, and
            settlement.
          </p>
        </div>

        {/* Environment */}
        <div className="glass-panel rounded-2xl border border-white/10 p-6 mb-10">
          <h2 className="text-sm font-bold uppercase tracking-widest text-brand-cyan mb-3">
            Environment
          </h2>
          <pre className="bg-black/60 border border-white/10 rounded-xl px-4 py-3 text-sm font-mano text-zinc-300">
            <code>{`AMP_SERVER_URL=https://amp.playwithamp.xyz  # production matchmaker
AMP_SERVER_URL=http://localhost:8080        # local development`}</code>
          </pre>
        </div>

        {/* Steps */}
        <div className="space-y-8">
          {steps.map((step, i) => (
            <div
              key={step.title}
              className="glass-panel rounded-3xl border border-white/10 overflow-hidden"
            >
              <div className="flex items-center gap-4 px-6 py-5 border-b border-white/10">
                <div className="w-10 h-10 rounded-xl bg-brand-cyan/10 border border-brand-cyan/20 flex items-center justify-center shrink-0">
                  <step.icon className="w-5 h-5 text-brand-cyan" />
                </div>
                <div>
                  <div className="text-[10px] font-black uppercase tracking-widest text-zinc-500">
                    Step {i + 1}
                  </div>
                  <h2 className="text-lg font-bold">{step.title}</h2>
                </div>
              </div>
              <div className="p-6">
                <div className="relative group">
                  <pre className="bg-black/60 border border-white/10 rounded-xl px-4 py-4 text-xs md:text-sm font-mono text-zinc-300 overflow-x-auto leading-relaxed">
                    <code>{step.code}</code>
                  </pre>
                  <button
                    className="absolute top-3 right-3 p-2 rounded-lg bg-white/5 border border-white/10 opacity-0 group-hover:opacity-100 transition-opacity"
                    aria-label="Copy code"
                  >
                    <Copy className="w-4 h-4 text-zinc-400" />
                  </button>
                </div>
                <p className="mt-4 text-sm text-zinc-400 leading-relaxed">
                  {step.note}
                </p>
              </div>
            </div>
          ))}
        </div>

        {/* Next steps */}
        <div className="mt-12 flex flex-col sm:flex-row gap-4">
          <Link
            href="/docs/api"
            className="flex-1 glass-panel rounded-2xl border border-brand-cyan/30 p-6 hover:border-brand-cyan/60 transition-all group"
          >
            <div className="text-xs font-black uppercase tracking-widest text-brand-cyan mb-2">
              Next
            </div>
            <div className="font-bold text-lg group-hover:text-brand-cyan transition-colors">
              Full API Reference →
            </div>
            <p className="text-sm text-zinc-500 mt-1">
              Every endpoint, WebSocket event, and EIP-712 type.
            </p>
          </Link>
          <Link
            href="/docs/contracts"
            className="flex-1 glass-panel rounded-2xl border border-white/10 p-6 hover:border-brand-cyan/40 transition-all group"
          >
            <div className="text-xs font-black uppercase tracking-widest text-zinc-500 mb-2">
              On-chain
            </div>
            <div className="font-bold text-lg group-hover:text-brand-cyan transition-colors">
              Contracts & Addresses →
            </div>
            <p className="text-sm text-zinc-500 mt-1">
              Deployed addresses, ABIs, and gas costs.
            </p>
          </Link>
        </div>
      </div>
    </div>
  );
}
