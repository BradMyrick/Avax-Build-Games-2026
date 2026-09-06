"use client";

import { useState, useMemo } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { motion, AnimatePresence } from "framer-motion";
import { PayPalScriptProvider, PayPalButtons } from "@paypal/react-paypal-js";
import { ethers } from "ethers";
import {
  Trophy,
  Coins,
  Wallet,
  Users,
  ChevronRight,
  ChevronLeft,
  Check,
  ExternalLink,
  Sparkles,
  ArrowLeft,
  LayoutDashboard,
} from "lucide-react";
import {
  CUP_ADDRESS, EXPLORER_URL,
  PAYOUT_PRESETS,
  connectWallet,
  signFinalize,
  AMPCUP_ABI,
} from "@/lib/ampCup";
import { generateWallets, parseAddressList, type GeneratedWallet } from "@/lib/wallet";
import { InfoTip } from "@/app/components/InfoTip";

type Currency = "USD" | "AVAX";

interface Result {
  ok: boolean;
  tournamentId?: number;
  txHash?: string | null;
  funded?: boolean;
  pending?: boolean;
  note?: string;
  winnerWallets: { placement: number; address: string }[];
  snowtrace?: string | null;
  error?: string;
}

const PLACE_LABELS = ["1st", "2nd", "3rd", "4th"];

export default function SetupPage() {
  const router = useRouter();
  const [step, setStep] = useState(0);
  const [name, setName] = useState("Community Cup");
  const [presetKey, setPresetKey] = useState<keyof typeof PAYOUT_PRESETS>("top3");
  const [currency, setCurrency] = useState<Currency>("USD");
  const [amount, setAmount] = useState("50");
  const [addressText, setAddressText] = useState("");
  const [result, setResult] = useState<Result | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [mode, setMode] = useState<"instant" | "bracket">("instant");
  const [playersText, setPlayersText] = useState("Alice, 0xaaaa…\nBob, 0xbbbb…\nCarlos, 0xcccc…\nDana, 0xdddd…");

  const preset = PAYOUT_PRESETS[presetKey];
  const placements = preset.bps.length;

  // Parse the players textarea
  const players = useMemo(
    () =>
      playersText
        .split("\n")
        .map((l) => l.trim())
        .filter(Boolean)
        .map((line, i) => {
          const [rawName, rawWallet] = line.split(",").map((s) => s.trim());
          const wallet = (rawWallet ?? rawName ?? "").trim();
          const isAddr = wallet.startsWith("0x") && wallet.length >= 6;
          const playerName = isAddr ? rawName ?? `Player ${i + 1}` : rawName ?? `Player ${i + 1}`;
          return {
            id: i,
            name: playerName,
            wallet: isAddr ? wallet : `0x${(i + 1).toString(16).padStart(40, "0")}`,
            seed: i + 1,
          };
        }),
    [playersText]
  );

  const winnerWallets: string[] = parseAddressList(addressText);

  const canProceedWinners =
    mode === "bracket"
      ? players.length >= 2
      : winnerWallets.length === placements;

  /** Poll the relayer job queue until the funded tournamentId is known. */
  async function pollJobForTournament(jobId?: number): Promise<number | null> {
    if (!jobId) return null;
    for (let i = 0; i < 60; i++) {
      setBusy(`Funding on Fuji via relayer… (${i + 1})`);
      const r = await fetch(`/api/job/${jobId}`);
      const job = (await r.json()) as {
        status?: string;
        tournamentId?: number | null;
        error?: string;
      };
      if (job.status === "done") return job.tournamentId ?? null;
      if (job.status === "failed") throw new Error(job.error || "relayer job failed");
      await new Promise((res) => setTimeout(res, 2000));
    }
    return null;
  }

  // ── Launch: AVAX (browser wallet) path ──
  async function launchAvax() {
    try {
      setBusy("Connecting wallet…");
      const provider = await connectWallet();
      const signer = await provider.getSigner();
      const sponsor = await signer.getAddress();
      const cup = new ethers.Contract(CUP_ADDRESS, AMPCUP_ABI, signer);

      setBusy("Funding prize pool on Fuji…");
      const deadline = BigInt(Math.floor(Date.now() / 1000) + 7 * 86400);
      const value = ethers.parseEther(amount);
      const createTx = await cup.createTournament(preset.bps, sponsor, deadline, {
        value,
        gasLimit: 500_000,
      });
      const createRcpt = await createTx.wait();
      const tournamentId = Number((await cup.nextTournamentId()) - BigInt(1));

      if (mode === "bracket") {
        setBusy("Setting up the bracket…");
        const initRes = await fetch(`/api/tournament/${tournamentId}/init`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            sponsor,
            prizePoolWei: value.toString(),
            payoutBps: preset.bps,
            format: "single_elimination",
            players,
            txHash: createRcpt?.hash ?? null,
          }),
        });
        const initJson = (await initRes.json()) as { manageToken?: string };
        if (initJson.manageToken) {
          sessionStorage.setItem(`amp_manage_${tournamentId}`, initJson.manageToken);
        }
        router.push(`/manage/${tournamentId}`);
        return;
      }

      setBusy("Attesting winners & finalizing…");
      const sig = await signFinalize(signer, tournamentId, winnerWallets);
      const finalizeTx = await cup.finalizeTournament(tournamentId, winnerWallets, sig, {
        gasLimit: 400_000,
      });
      const rcpt = await finalizeTx.wait();

      setResult({
        ok: true,
        tournamentId,
        txHash: rcpt?.hash ?? null,
        funded: true,
        winnerWallets: winnerWallets.map((address, i) => ({ placement: i, address })),
        snowtrace: `${EXPLORER_URL}/address/${CUP_ADDRESS}`,
      });
    } catch (e) {
      setResult({
        ok: false,
        error: (e as Error).message,
        winnerWallets: [],
      });
    } finally {
      setBusy(null);
    }
  }

  // ── Launch: USD (PayPal) capture path ──
  async function capturePaypal(orderID: string) {    try {
      setBusy("Verifying payment & funding tournament…");
      const tournamentBody =
        mode === "bracket"
          ? { name, payoutBps: preset.bps, mode: "bracket", format: "single_elimination" as const, players, finalizeDays: 7 }
          : { name, payoutBps: preset.bps, mode: "instant" as const, winnerWallets, finalizeDays: 7 };
      const res = await fetch("/api/paypal/capture", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ orderID, tournament: tournamentBody }),
      });
      const json = (await res.json()) as {
        ok?: boolean;
        error?: string;
        jobId?: number;
        pending?: boolean;
        amountUsd?: number;
        manageToken?: string;
        replay?: boolean;
      };
      if (!json.ok) throw new Error(json.error || "Capture failed");

      // Custodial funding is async: the web enqueues a job; the isolated Rust
      // relayer drains it, funds on-chain, and writes back the tournamentId.
      const tournamentId = await pollJobForTournament(json.jobId);
      if (tournamentId == null) throw new Error("Relayer did not complete funding in time.");

      // Bracket mode: hand off to the organizer console.
      if (mode === "bracket") {
        if (json.manageToken) {
          sessionStorage.setItem(`amp_manage_${tournamentId}`, json.manageToken);
        }
        router.push(`/manage/${tournamentId}`);
        return;
      }

      setResult({
        ok: true,
        tournamentId,
        txHash: null,
        funded: true,
        winnerWallets: winnerWallets.map((address, i) => ({
          placement: i,
          address,
        })),
        snowtrace: `${EXPLORER_URL}/address/${CUP_ADDRESS}`,
      });
    } catch (e) {
      setResult({
        ok: false,
        error: (e as Error).message,
        winnerWallets: [],
      });
    } finally {
      setBusy(null);
    }
  }

  const clientId = process.env.NEXT_PUBLIC_PAYPAL_CLIENT_ID || "";

  return (
    <div className="relative min-h-screen overflow-hidden antialiased bg-black text-white">
      <div className="absolute top-0 -left-1/4 w-[150%] h-[500px] bg-brand-cyan/10 blur-[120px] rounded-full pointer-events-none" />
      <div className="absolute bottom-0 -right-1/4 w-[150%] h-[500px] bg-brand-red/10 blur-[120px] rounded-full pointer-events-none" />

      <header className="relative z-10 max-w-5xl mx-auto px-6 pt-10">
        <Link href="/" className="inline-flex items-center gap-2 text-zinc-400 hover:text-brand-cyan transition-colors text-sm">
          <ArrowLeft className="w-4 h-4" /> Back to AMP
        </Link>
      </header>

      <main className="relative z-10 max-w-3xl mx-auto px-6 py-12">
        <div className="text-center mb-10">
          <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-brand-cyan/10 border border-brand-cyan/30 mb-4">
            <Sparkles className="w-3 h-3 text-brand-cyan" />
            <span className="text-xs font-bold text-brand-cyan tracking-widest uppercase">Launch a Tournament</span>
          </div>
          <h1 className="text-4xl md:text-5xl font-black uppercase tracking-tight mb-3">
            Set Up Your <span className="text-brand-cyan">Cup</span>
          </h1>
          <p className="text-zinc-400 text-lg">
            Prize pool, payout split, winner wallets — fund with card or wallet. Winners claim instantly on Avalanche.
          </p>
        </div>

        {/* Stepper */}
        {!result && (
          <div className="flex items-center justify-center gap-2 mb-8">
            {["Prize", "Winners", "Fund"].map((label, i) => (
              <div key={label} className="flex items-center gap-2">
                <div className={`w-7 h-7 rounded-full flex items-center justify-center text-xs font-bold border ${step >= i ? "bg-brand-cyan text-black border-brand-cyan" : "bg-white/5 text-zinc-500 border-white/10"}`}>
                  {step > i ? <Check className="w-3.5 h-3.5" /> : i + 1}
                </div>
                <span className={`text-xs uppercase tracking-wider ${step >= i ? "text-white" : "text-zinc-600"}`}>{label}</span>
                {i < 2 && <div className="w-8 h-px bg-white/10 mx-1" />}
              </div>
            ))}
          </div>
        )}

        <AnimatePresence mode="wait">
          {/* ── RESULT ── */}
          {result && (
            <motion.div
              key="result"
              initial={{ opacity: 0, y: 16 }}
              animate={{ opacity: 1, y: 0 }}
              className="glass-panel p-8"
            >
              {result.ok ? (
                <>
                  <div className="text-center mb-6">
                    <div className="w-16 h-16 rounded-2xl bg-green-500/15 border border-green-500/30 flex items-center justify-center mx-auto mb-4 text-green-400">
                      <Trophy className="w-8 h-8" />
                    </div>
                    <h2 className="text-2xl font-black uppercase mb-2">Tournament Live</h2>
                    <p className="text-zinc-400">
                      {result.pending
                        ? "Recorded — funding pending relayer setup."
                        : `AMP Cup #${result.tournamentId} funded & finalized on Fuji.`}
                    </p>
                  </div>

                  {result.tournamentId != null && (
                    <Row label="Tournament ID" value={`#${result.tournamentId}`} />
                  )}
                  {result.txHash && (
                    <Row label="Transaction" value={result.txHash.slice(0, 18) + "…"} href={`${EXPLORER_URL}/tx/${result.txHash}`} />
                  )}
                  {result.snowtrace && (
                    <Row label="Contract" value={CUP_ADDRESS.slice(0, 10) + "…" + CUP_ADDRESS.slice(-4)} href={result.snowtrace} />
                  )}
                  {result.note && (
                    <p className="text-xs text-yellow-400/80 bg-yellow-400/5 border border-yellow-400/20 rounded-lg p-3 mt-4">{result.note}</p>
                  )}

                  <div className="mt-6">
                    <h3 className="text-sm font-bold uppercase tracking-wider text-zinc-300 mb-3">Winner claim wallets</h3>
                    <div className="space-y-2">
                      {result.winnerWallets.map((w) => (
                        <div key={w.placement} className="flex items-center gap-3 bg-white/5 border border-white/10 rounded-lg p-3">
                          <span className="text-xs font-bold text-brand-cyan w-10">{PLACE_LABELS[w.placement]}</span>
                          <code className="text-xs text-zinc-300 flex-1 truncate">{w.address}</code>
                          {result.tournamentId != null && (
                            <Link
                              href={`/claim?tid=${result.tournamentId}&place=${w.placement}`}
                              className="text-[10px] text-brand-cyan hover:underline flex items-center gap-1"
                            >
                              claim <ExternalLink className="w-3 h-3" />
                            </Link>
                          )}
                        </div>
                      ))}
                    </div>
                    <p className="text-[11px] text-zinc-500 mt-3">
                      Share each wallet&rsquo;s private key with its winner. They import it in any wallet and call <code className="text-brand-cyan">claimPrize</code> on the contract to receive their payout.
                    </p>
                  </div>

                  <div className="flex gap-3 mt-8">
                    <Link href="/setup" onClick={() => { setResult(null); setStep(0); }} className="flex-1 px-6 py-3 rounded-sm font-bold text-black bg-brand-cyan hover:bg-white transition-colors text-center uppercase tracking-widest text-sm">
                      Run Another
                    </Link>
                    <Link href="/" className="flex-1 px-6 py-3 rounded-sm font-bold text-brand-cyan glass-panel hover:bg-brand-cyan/10 text-center uppercase tracking-widest text-sm">
                      Home
                    </Link>
                  </div>
                </>
              ) : (
                <div className="text-center">
                  <h2 className="text-2xl font-black uppercase mb-3 text-brand-red">Launch Failed</h2>
                  <p className="text-zinc-400 text-sm mb-6 font-mono">{result.error}</p>
                  <button onClick={() => setResult(null)} className="px-6 py-3 rounded-sm font-bold text-brand-cyan glass-panel hover:bg-brand-cyan/10 uppercase tracking-widest text-sm">
                    Back to Setup
                  </button>
                </div>
              )}
            </motion.div>
          )}

          {/* ── STEP 0: PRIZE ── */}
          {!result && step === 0 && (
            <motion.div key="s0" initial={{ opacity: 0, x: 16 }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0, x: -16 }} className="space-y-6">
              <Panel icon={<Trophy className="w-5 h-5" />} title="Tournament name">
                <input value={name} onChange={(e) => setName(e.target.value)} className="w-full bg-black/40 border border-white/10 rounded-lg px-4 py-3 text-white focus:border-brand-cyan outline-none" />
              </Panel>

              <Panel icon={<Coins className="w-5 h-5" />} title="Prize pool & payout split">
                <div className="flex items-center gap-2 mb-2">
                  <span className="text-xs uppercase tracking-wider text-zinc-500">Tournament type</span>
                  <InfoTip>
                    <strong className="text-white">Instant payout:</strong> winners are paid immediately after funding. You provide the winner wallets upfront — no bracket to manage.<br/><br/>
                    <strong className="text-white">Run a bracket:</strong> players compete in a single-elimination bracket. You record results from the organizer console; winners are determined by match outcomes.
                  </InfoTip>
                </div>
                <div className="grid grid-cols-2 gap-3 mb-4">
                  <Toggle active={mode === "instant"} onClick={() => setMode("instant")} icon={<Sparkles className="w-4 h-4" />} label="Instant payout" />
                  <Toggle active={mode === "bracket"} onClick={() => setMode("bracket")} icon={<Trophy className="w-4 h-4" />} label="Run a bracket" />
                </div>
                <div className="flex items-center gap-2 mb-2">
                  <span className="text-xs uppercase tracking-wider text-zinc-500">Payment method</span>
                  <InfoTip>
                    <strong className="text-white">USD (PayPal):</strong> pay with a credit/debit card. AMP&rsquo;s custodial relayer funds the on-chain prize pool for you — no wallet or crypto needed.<br/><br/>
                    <strong className="text-white">AVAX (Wallet):</strong> pay directly with AVAX from your Avalanche wallet. You fund the prize pool yourself on-chain.
                  </InfoTip>
                </div>
                <div className="grid grid-cols-2 gap-3 mb-4">
                  <Toggle active={currency === "USD"} onClick={() => setCurrency("USD")} icon={<span>$</span>} label="USD (Card / PayPal)" />
                  <Toggle active={currency === "AVAX"} onClick={() => setCurrency("AVAX")} icon={<Wallet className="w-4 h-4" />} label="AVAX (Wallet)" />
                </div>
                <label className="text-xs uppercase tracking-wider text-zinc-500">Prize amount ({currency})</label>
                <input type="number" min="1" value={amount} onChange={(e) => setAmount(e.target.value)} className="w-full bg-black/40 border border-white/10 rounded-lg px-4 py-3 text-white focus:border-brand-cyan outline-none mt-1" />
                <label className="text-xs uppercase tracking-wider text-zinc-500 mt-4 block">Payout split</label>
                <div className="grid grid-cols-2 gap-2 mt-1">
                  {Object.entries(PAYOUT_PRESETS).map(([key, p]) => (
                    <button key={key} onClick={() => setPresetKey(key as keyof typeof PAYOUT_PRESETS)} className={`text-left p-3 rounded-lg border text-sm transition-colors ${presetKey === key ? "bg-brand-cyan/10 border-brand-cyan/50 text-white" : "bg-white/5 border-white/10 text-zinc-400 hover:border-white/20"}`}>
                      {p.label}
                    </button>
                  ))}
                </div>
              </Panel>

              <NavRow onBack={() => setStep(0)} onNext={() => setStep(1)} nextLabel="Next: Winners" disabled={!name || !amount} />
            </motion.div>
          )}

          {/* ── STEP 1: WINNERS / PLAYERS ── */}
          {!result && step === 1 && (
            <motion.div key="s1" initial={{ opacity: 0, x: 16 }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0, x: -16 }} className="space-y-6">
              {mode === "bracket" ? (
                <Panel icon={<Users className="w-5 h-5" />} title={`Player wallets (${players.length})`}>
                  <textarea
                    value={playersText}
                    onChange={(e) => setPlayersText(e.target.value)}
                    rows={8}
                    placeholder={"One player per line:  name, 0xWALLET\nAlice, 0xaaaa…\nBob, 0xbbbb…"}
                    className="w-full bg-black/40 border border-white/10 rounded-lg px-4 py-3 text-white focus:border-brand-cyan outline-none font-mono text-xs"
                  />
                  <p className="text-[11px] text-zinc-500 mt-2">
                    {players.length} players · single-elimination bracket · top {placements} paid ({preset.label}).
                    Run the bracket round-by-round from the organizer console after funding.
                  </p>
                </Panel>
              ) : (
                <Panel icon={<Users className="w-5 h-5" />} title={`Winner wallets (${placements} placement${placements > 1 ? "s" : ""})`}>
                  <textarea
                    value={addressText}
                    onChange={(e) => setAddressText(e.target.value)}
                    placeholder={`Enter ${placements} wallet address(es) in placement order (1st, 2nd…), one per line:\n0x1234…\n0x5678…`}
                    rows={5}
                    className="w-full bg-black/40 border border-white/10 rounded-lg px-4 py-3 text-white focus:border-brand-cyan outline-none font-mono text-xs"
                  />
                  <p className="text-[11px] text-zinc-500 mt-2">Parsed {winnerWallets.length}/{placements} valid addresses. Winners claim their prize from the contract after the tournament is funded.</p>
                </Panel>
              )}
              <NavRow onBack={() => setStep(0)} onNext={() => setStep(2)} nextLabel="Next: Fund" disabled={!canProceedWinners} />
            </motion.div>
          )}

          {/* ── STEP 2: FUND ── */}
          {!result && step === 2 && (
            <motion.div key="s2" initial={{ opacity: 0, x: 16 }} animate={{ opacity: 1, x: 0 }} exit={{ opacity: 0, x: -16 }} className="space-y-6">
              <Panel icon={<Coins className="w-5 h-5" />} title="Review & fund">
                <div className="space-y-2 mb-6">
                  <Row label="Name" value={name} />
                  <Row label="Prize" value={`${amount} ${currency}`} />
                  <Row label="Split" value={preset.label} />
                  <Row label="Winners" value={`${placements} wallet${placements > 1 ? "s" : ""}`} />
                  <Row label="Protocol fee" value="2% on payouts" />
                </div>

                {busy && (
                  <div className="text-center text-zinc-400 text-sm py-4">{busy}</div>
                )}

                {!busy && currency === "AVAX" && (
                  <button onClick={launchAvax} className="w-full px-6 py-4 rounded-sm font-bold text-black bg-brand-cyan hover:bg-white transition-all flex items-center justify-center gap-2 uppercase tracking-widest text-sm">
                    <Wallet className="w-5 h-5" /> Connect Wallet & Fund
                  </button>
                )}

                {!busy && currency === "USD" && (
                  clientId ? (
                    <PayPalScriptProvider options={{ clientId, currency: "USD", intent: "capture" }}>
                      <PayPalButtons
                        style={{ layout: "vertical", label: "pay", color: "blue", shape: "rect" }}
                        createOrder={async () => {
                          const res = await fetch("/api/paypal/create-order", {
                            method: "POST",
                            headers: { "Content-Type": "application/json" },
                            body: JSON.stringify({ amountUsd: Number(amount), tournamentName: name }),
                          });
                          const json = (await res.json()) as { id?: string; error?: string };
                          if (!json.id) throw new Error(json.error || "create-order failed");
                          return json.id;
                        }}
                        onApprove={async (data) => {
                          await capturePaypal(data.orderID);
                        }}
                      />
                    </PayPalScriptProvider>
                  ) : (
                    <div className="text-center text-zinc-500 text-sm bg-white/5 border border-white/10 rounded-lg p-4">
                      PayPal not configured. Set <code className="text-brand-cyan">NEXT_PUBLIC_PAYPAL_CLIENT_ID</code> + server creds to enable card payments, or switch to AVAX.
                    </div>
                  )
                )}
              </Panel>
              <NavRow onBack={() => setStep(1)} onNext={() => {}} nextLabel="" hideNext />
            </motion.div>
          )}
        </AnimatePresence>

        <p className="text-center text-[11px] text-zinc-600 mt-8">
          Open beta · Fuji testnet · <a className="text-zinc-500 hover:text-brand-cyan" href={`${EXPLORER_URL}/address/${CUP_ADDRESS}`} target="_blank" rel="noreferrer">view contract</a>
        </p>
      </main>
    </div>
  );
}

// ── Small UI helpers ──

function Panel({ icon, title, children }: { icon: React.ReactNode; title: string; children: React.ReactNode }) {
  return (
    <div className="glass-panel p-6">
      <div className="flex items-center gap-2 mb-4 text-brand-cyan">
        {icon}
        <h3 className="text-sm font-bold uppercase tracking-wider text-white">{title}</h3>
      </div>
      {children}
    </div>
  );
}

function Toggle({ active, onClick, icon, label }: { active: boolean; onClick: () => void; icon: React.ReactNode; label: string }) {
  return (
    <button onClick={onClick} className={`flex items-center gap-2 p-3 rounded-lg border text-sm transition-colors ${active ? "bg-brand-cyan/10 border-brand-cyan/50 text-white" : "bg-white/5 border-white/10 text-zinc-400 hover:border-white/20"}`}>
      <span className="text-brand-cyan">{icon}</span> {label}
    </button>
  );
}

function Row({ label, value, href }: { label: string; value: string; href?: string }) {
  const content = (
    <div className="flex justify-between items-center py-2 border-b border-white/5 last:border-0">
      <span className="text-xs uppercase tracking-wider text-zinc-500">{label}</span>
      <span className="text-sm text-white font-mono flex items-center gap-1">
        {value}
        {href && <ExternalLink className="w-3 h-3 text-zinc-500" />}
      </span>
    </div>
  );
  return href ? (
    <a href={href} target="_blank" rel="noreferrer" className="block hover:bg-white/5 rounded transition-colors">{content}</a>
  ) : content;
}

function NavRow({ onBack, onNext, nextLabel, disabled, hideNext }: { onBack: () => void; onNext: () => void; nextLabel: string; disabled?: boolean; hideNext?: boolean }) {
  return (
    <div className="flex gap-3">
      <button onClick={onBack} className="px-6 py-3 rounded-sm font-bold text-zinc-300 glass-panel hover:bg-white/5 flex items-center gap-1 uppercase tracking-widest text-sm">
        <ChevronLeft className="w-4 h-4" /> Back
      </button>
      {!hideNext && (
        <button onClick={onNext} disabled={disabled} className="flex-1 px-6 py-3 rounded-sm font-bold text-black bg-brand-cyan hover:bg-white transition-colors flex items-center justify-center gap-2 uppercase tracking-widest text-sm disabled:opacity-40 disabled:cursor-not-allowed">
          {nextLabel} <ChevronRight className="w-4 h-4" />
        </button>
      )}
    </div>
  );
}
