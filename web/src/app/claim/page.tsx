"use client";

import { useState, Suspense } from "react";
import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { ethers } from "ethers";
import {
  Wallet,
  KeyRound,
  ArrowLeft,
  Check,
  ExternalLink,
  Coins,
} from "lucide-react";
import { CUP_ADDRESS, connectWallet, AMPCUP_ABI, FUJI_RPC, EXPLORER_URL } from "@/lib/ampCup";

type Mode = "wallet" | "key";
type Status = "idle" | "claiming" | "done" | "error";

export default function ClaimPage() {
  return (
    <Suspense fallback={<div className="min-h-screen bg-black" />}>
      <ClaimContent />
    </Suspense>
  );
}

function ClaimContent() {
  const params = useSearchParams();
  const [tournamentId, setTournamentId] = useState(params.get("tid") ?? "");
  const [placement, setPlacement] = useState(params.get("place") ?? "0");
  const [mode, setMode] = useState<Mode>("wallet");
  const [privateKey, setPrivateKey] = useState("");
  const [status, setStatus] = useState<Status>("idle");
  const [txHash, setTxHash] = useState<string | null>(null);
  const [payout, setPayout] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function claim() {
    setStatus("claiming");
    setError(null);
    setTxHash(null);
    setPayout(null);
    try {
      const tid = BigInt(tournamentId);
      const place = BigInt(placement);

      let signer: ethers.Signer;
      if (mode === "wallet") {
        const provider = await connectWallet();
        signer = await provider.getSigner();
      } else {
        if (!privateKey.startsWith("0x") || privateKey.length !== 66) {
          throw new Error("Enter a valid 0x-prefixed 32-byte private key.");
        }
        const provider = new ethers.JsonRpcProvider(
          FUJI_RPC
        );
        signer = new ethers.Wallet(privateKey, provider);
      }

      const cup = new ethers.Contract(CUP_ADDRESS, AMPCUP_ABI, signer);
      const winner = await signer.getAddress();

      // Balance-delta so we can show the prize received (net of gas for the key path).
      const provider = signer.provider;
      const before = provider ? await provider.getBalance(winner) : BigInt(0);

      const tx = await cup.claimPrize(tid, place, { gasLimit: 200_000 });
      const rcpt = await tx.wait();
      const after = provider ? await provider.getBalance(winner) : BigInt(0);

      setTxHash(rcpt?.hash ?? null);
      // For the imported-key path, net of gas; for the wallet path, gross prize.
      const delta = after - before;
      setPayout(ethers.formatEther(delta < BigInt(0) ? BigInt(0) : delta));
      setStatus("done");
    } catch (e) {
      setError((e as Error).message);
      setStatus("error");
    }
  }

  return (
    <div className="relative min-h-screen overflow-hidden antialiased bg-black text-white">
      <div className="absolute top-0 -left-1/4 w-[150%] h-[500px] bg-brand-cyan/10 blur-[120px] rounded-full pointer-events-none" />
      <div className="absolute bottom-0 -right-1/4 w-[150%] h-[500px] bg-brand-red/10 blur-[120px] rounded-full pointer-events-none" />

      <header className="relative z-10 max-w-5xl mx-auto px-6 pt-10">
        <Link href="/" className="inline-flex items-center gap-2 text-zinc-400 hover:text-brand-cyan transition-colors text-sm">
          <ArrowLeft className="w-4 h-4" /> Back to AMP
        </Link>
      </header>

      <main className="relative z-10 max-w-xl mx-auto px-6 py-12">
        <div className="text-center mb-10">
          <div className="w-16 h-16 rounded-2xl bg-yellow-400/15 border border-yellow-400/30 flex items-center justify-center mx-auto mb-4 text-yellow-400">
            <Coins className="w-8 h-8" />
          </div>
          <h1 className="text-4xl md:text-5xl font-black uppercase tracking-tight mb-3">
            Claim Your <span className="text-brand-cyan">Prize</span>
          </h1>
          <p className="text-zinc-400">
            Pull your escrowed payout from an AMP tournament on Avalanche.
          </p>
        </div>

        <div className="glass-panel p-6 space-y-5">
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="text-xs uppercase tracking-wider text-zinc-500">Tournament ID</label>
              <input
                value={tournamentId}
                onChange={(e) => setTournamentId(e.target.value)}
                inputMode="numeric"
                className="w-full bg-black/40 border border-white/10 rounded-lg px-3 py-2.5 text-white focus:border-brand-cyan outline-none mt-1"
              />
            </div>
            <div>
              <label className="text-xs uppercase tracking-wider text-zinc-500">Placement</label>
              <select
                value={placement}
                onChange={(e) => setPlacement(e.target.value)}
                className="w-full bg-black/40 border border-white/10 rounded-lg px-3 py-2.5 text-white focus:border-brand-cyan outline-none mt-1"
              >
                <option value="0">1st</option>
                <option value="1">2nd</option>
                <option value="2">3rd</option>
                <option value="3">4th</option>
              </select>
            </div>
          </div>

          <div>
            <label className="text-xs uppercase tracking-wider text-zinc-500 block mb-2">Claim with</label>
            <div className="grid grid-cols-2 gap-3">
              <button
                onClick={() => setMode("wallet")}
                className={`flex items-center justify-center gap-2 p-3 rounded-lg border text-sm transition-colors ${mode === "wallet" ? "bg-brand-cyan/10 border-brand-cyan/50 text-white" : "bg-white/5 border-white/10 text-zinc-400"}`}
              >
                <Wallet className="w-4 h-4 text-brand-cyan" /> Connect wallet
              </button>
              <button
                onClick={() => setMode("key")}
                className={`flex items-center justify-center gap-2 p-3 rounded-lg border text-sm transition-colors ${mode === "key" ? "bg-brand-cyan/10 border-brand-cyan/50 text-white" : "bg-white/5 border-white/10 text-zinc-400"}`}
              >
                <KeyRound className="w-4 h-4 text-brand-cyan" /> Import private key
              </button>
            </div>
          </div>

          {mode === "key" && (
            <div>
              <label className="text-xs uppercase tracking-wider text-zinc-500">Winner&rsquo;s private key</label>
              <input
                value={privateKey}
                onChange={(e) => setPrivateKey(e.target.value)}
                type="password"
                placeholder="0x…"
                className="w-full bg-black/40 border border-white/10 rounded-lg px-3 py-2.5 text-white focus:border-brand-cyan outline-none mt-1 font-mono text-xs"
              />
              <p className="text-[11px] text-zinc-500 mt-1">
                The key generated for your placement. It signs the claim directly — never sent anywhere except Avalanche.
              </p>
            </div>
          )}

          <button
            onClick={claim}
            disabled={status === "claiming" || !tournamentId}
            className="w-full px-6 py-3.5 rounded-sm font-bold text-black bg-brand-cyan hover:bg-white transition-colors flex items-center justify-center gap-2 uppercase tracking-widest text-sm disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {status === "claiming" ? "Claiming…" : (
              <>
                <Coins className="w-5 h-5" /> Claim Prize
              </>
            )}
          </button>

          {status === "done" && (
            <div className="bg-green-500/10 border border-green-500/30 rounded-lg p-4 text-center">
              <Check className="w-6 h-6 text-green-400 mx-auto mb-2" />
              <p className="text-green-400 font-bold mb-1">Prize claimed!</p>
              {payout && <p className="text-sm text-zinc-300">+{payout} AVAX</p>}
              {txHash && (
                <a
                  href={`${EXPLORER_URL}/tx/${txHash}`}
                  target="_blank"
                  rel="noreferrer"
                  className="inline-flex items-center gap-1 text-xs text-brand-cyan hover:underline mt-2"
                >
                  View tx <ExternalLink className="w-3 h-3" />
                </a>
              )}
            </div>
          )}

          {status === "error" && error && (
            <div className="bg-brand-red/10 border border-brand-red/30 rounded-lg p-3 text-sm text-brand-red font-mono">
              {error}
            </div>
          )}
        </div>

        <p className="text-center text-[11px] text-zinc-600 mt-6">
          Open beta · Fuji testnet · <a className="text-zinc-500 hover:text-brand-cyan" href={`${EXPLORER_URL}/address/${CUP_ADDRESS}`} target="_blank" rel="noreferrer">view contract</a>
        </p>
      </main>
    </div>
  );
}
