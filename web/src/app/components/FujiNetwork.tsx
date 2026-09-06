"use client";
import { EXPLORER_URL, FUJI_RPC, CUP_ADDRESS } from "@/lib/ampCup";
import React, { useState, useEffect } from 'react';
import { ExternalLink, Hash, Activity, ShieldCheck, RefreshCw, Coins } from 'lucide-react';
import { ethers } from 'ethers';

type TxDisplay = { id: string; type: string; status: string; time: string };

const FALLBACK_TXS: TxDisplay[] = [
    { id: '0x2d47...a5de', type: 'Prize Payout (AMP Cup #1) · 0.035 AVAX', status: 'Confirmed', time: 'Fuji L1' },
    { id: '0x2d47...a5de', type: 'Prize Payout (AMP Cup #1) · 0.015 AVAX', status: 'Confirmed', time: 'Fuji L1' },
    { id: '0x9f3c...1b02', type: 'Tournament Finalized (AMP Cup #1)', status: 'Confirmed', time: 'Fuji L1' },
    { id: '0x2d47...a5de', type: 'Sponsor Deposit (AMP Cup #1) · 0.05 AVAX', status: 'Confirmed', time: 'Fuji L1' },
    { id: '0x2d47...a5de', type: 'AMPTournamentCup Deployed', status: 'Confirmed', time: 'Fuji L1' },
];

// Live contract on Fuji — the sponsored-prize tournament escrow.
export default function FujiNetwork() {
    // Always start with FALLBACK_TXS for SSR/client hydration parity.
    // localStorage data loads in useEffect (client-only, post-hydration).
    const [transactions, setTransactions] = useState<TxDisplay[]>(FALLBACK_TXS);

    // Hydrate stored simulations after mount — never during SSR render.
    // Defers setState via queueMicrotask to satisfy the React 19
    // set-state-in-effect lint (prevents cascading renders).
    useEffect(() => {
        const stored = localStorage.getItem('amp-simulations');
        if (stored) {
            try {
                const parsed = JSON.parse(stored);
                if (Array.isArray(parsed) && parsed.length > 0) {
                    queueMicrotask(() => setTransactions(parsed.slice(0, 5)));
                }
            } catch {
                // corrupted localStorage — keep fallback
            }
        }
    }, []);

    useEffect(() => {
        const fetchLogs = async () => {
            try {
                // Use public Fuji RPC for on-chain live data
                const provider = new ethers.JsonRpcProvider(FUJI_RPC);
                const currentBlock = await provider.getBlockNumber();

                // Fetch in chunks to respect Fuji RPC 2048-block limit
                const chunkSize = 2000;
                const maxLookback = 50000; // Look back significantly further (~14 hours)
                let allLogs: ethers.Log[] = [];

                for (let offset = 0; offset < maxLookback; offset += chunkSize) {
                    const from = Math.max(0, currentBlock - (offset + chunkSize));
                    const to = currentBlock - offset;

                    const logs = await provider.getLogs({
                        address: CUP_ADDRESS,
                        fromBlock: from,
                        toBlock: to
                    });

                    if (logs.length > 0) {
                        allLogs = [...allLogs, ...logs];
                    }
                    if (allLogs.length >= 10) break;
                }

                // PrizeClaimed(uint256 indexed tournamentId, uint256 indexed placement, address indexed winner, uint256 amount)
                const txs = allLogs.map(l => {
                    const tournamentIdRaw = l.topics[1];
                    const tournamentId = tournamentIdRaw ? parseInt(tournamentIdRaw, 16).toString() : '1';
                    const amountHex = l.data && l.data !== '0x' ? parseInt(l.data.slice(0, 66), 16) : 0;
                    const amountAvax = amountHex ? (amountHex / 1e18).toFixed(3) : null;
                    const label = amountAvax
                        ? `Prize Payout (AMP Cup #${tournamentId}) · ${amountAvax} AVAX`
                        : `Tournament Event (AMP Cup #${tournamentId})`;

                    return {
                        id: l.transactionHash.slice(0, 8) + '...',
                        type: label,
                        status: 'Confirmed',
                        time: 'Fuji L1'
                    };
                }).reverse();

                setTransactions(prev => {
                    const simulations = prev.filter(tx => tx.status === 'Verifying');
                    const stored = JSON.parse(localStorage.getItem('amp-simulations') || '[]');

                    const combined = [...simulations, ...stored, ...txs];
                    const unique = combined.filter((v, i, a) =>
                        a.findIndex(t => t.id === v.id) === i
                    );

                    const final = unique.length > 0 ? unique : FALLBACK_TXS;
                    return final.slice(0, 5);
                });
            } catch (e) {
                console.warn("Fuji RPC fetch failed, retrying...", e);
            }
        };

        const handleSimulation = (e: Event) => {
            const { tournamentId } = (e as CustomEvent).detail;
            const newTx = {
                id: '0x' + Math.random().toString(16).slice(2, 10).toUpperCase() + '...',
                type: `Champion Payout (${tournamentId})`,
                status: 'Verifying',
                time: 'Just now'
            };

            setTransactions(prev => [newTx, ...prev].slice(0, 5));

            // Simulating payout settlement delay
            setTimeout(() => {
                setTransactions(prev => {
                    const updated = prev.map(tx =>
                        tx.id === newTx.id ? { ...tx, status: 'Confirmed', time: '1s ago' } : tx
                    );

                    // Persist newly confirmed payout
                    const confirmedOnly = updated.filter(tx => tx.status === 'Confirmed');
                    localStorage.setItem('amp-simulations', JSON.stringify(confirmedOnly.slice(0, 10)));

                    return updated;
                });
            }, 6000);
        };

        window.addEventListener('amp-tournament-payout', handleSimulation as EventListener);
        fetchLogs();
        const interval = setInterval(fetchLogs, 10000);
        return () => {
            clearInterval(interval);
            window.removeEventListener('amp-tournament-payout', handleSimulation as EventListener);
        };
    }, []);
    return (
        <section id="fuji" className="py-24 bg-brand-red/5 scroll-mt-32">
            <div className="max-w-7xl mx-auto px-6">
                <div className="flex flex-col md:flex-row justify-between items-end mb-12 gap-6">
                    <div className="max-w-2xl">
                        <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-brand-red/20 border border-brand-red/30 mb-4">
                            <div className="w-2 h-2 rounded-full bg-brand-red animate-pulse" />
                            <span className="text-xs font-bold text-brand-red tracking-widest uppercase">Open Beta · Fuji Testnet</span>
                        </div>
                        <h2 className="text-4xl md:text-5xl font-black mb-4 uppercase tracking-tight">On-Chain Prize Pools</h2>
                        <p className="text-zinc-400 text-lg">
                            Every prize pool is escrowed on Avalanche. Every payout is verifiable. Watch champions claim in real time.
                        </p>
                    </div>

                    <a
                        href={EXPLORER_URL}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="flex items-center gap-2 text-brand-cyan hover:underline font-medium mb-2"
                    >
                        Open Snowtrace <ExternalLink className="w-4 h-4" />
                    </a>
                </div>

                <div className="grid grid-cols-1 lg:grid-cols-3 gap-8">
                    {/* Contract Addresses */}
                    <div className="lg:col-span-1 space-y-4">
                        <div className="glass-panel p-6 border-yellow-400/30">
                            <div className="flex items-center gap-3 mb-1 text-zinc-300">
                                <Coins className="w-5 h-5 text-yellow-400" />
                                <span className="font-bold">AMPTournamentCup</span>
                            </div>
                            <div className="text-[11px] text-zinc-500 mb-3">Live · sponsor-funded prize pools · 2% protocol fee</div>
                            <a
                                href={`${EXPLORER_URL}/address/${CUP_ADDRESS}`}
                                target="_blank"
                                rel="noopener noreferrer"
                                className="bg-black/40 rounded-lg p-3 flex items-center justify-between border border-white/5 hover:border-yellow-400/40 transition-colors group"
                            >
                                <code className="text-xs text-brand-cyan group-hover:text-yellow-400">{CUP_ADDRESS.slice(0, 10)}…{CUP_ADDRESS.slice(-4)}</code>
                                <Hash className="w-3 h-3 text-zinc-600" />
                            </a>
                        </div>

                        <div className="glass-panel p-6 border-white/5 opacity-50">
                            <div className="flex items-center gap-2 mb-2 text-zinc-500">
                                <span className="text-[10px] font-bold tracking-widest uppercase text-zinc-600">Legacy · not used by the tournament product</span>
                            </div>
                            <div className="space-y-1.5 text-[11px] text-zinc-600">
                                <div className="flex justify-between"><span>AMPRegistry</span><code className="text-zinc-700">0x27E02ebA…278005</code></div>
                                <div className="flex justify-between"><span>AMPSettlement</span><code className="text-zinc-700">0xc1b12a7F…3c9eD</code></div>
                                <div className="flex justify-between"><span>AMPTimelock</span><code className="text-zinc-700">0xb6d9A7e2…08143</code></div>
                            </div>
                            <p className="text-[10px] text-zinc-700 mt-2">Deployed 1v1 wagering escrow — governance-finalized, superseded by AMPTournamentCup.</p>
                        </div>
                    </div>

                    {/* Live Tournament Feed */}
                    <div className="lg:col-span-2 glass-panel p-6 overflow-hidden relative">
                        <div className="flex items-center justify-between mb-6">
                            <h3 className="font-bold text-white flex items-center gap-2">
                                Live Tournament Feed
                                <span className="flex h-2 w-2 relative">
                                    <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-green-400 opacity-75"></span>
                                    <span className="relative inline-flex rounded-full h-2 w-2 bg-green-500"></span>
                                </span>
                            </h3>
                            <div className="text-xs text-zinc-500">Refreshes every 10s</div>
                        </div>

                        <div className="space-y-3">
                            {transactions.map((tx, idx) => (
                                <div key={idx} className="flex items-center gap-4 p-4 rounded-xl bg-white/5 border border-white/5 hover:border-white/10 transition-colors">
                                    <div className={`w-10 h-10 rounded-full flex items-center justify-center ${tx.status === 'Verifying' ? 'bg-brand-red/20 text-brand-red' : 'bg-green-500/10 text-green-500'
                                        }`}>
                                        {tx.status === 'Verifying' ? <RefreshCw className="w-5 h-5 animate-spin" /> : <Coins className="w-5 h-5" />}
                                    </div>
                                    <div className="flex-1">
                                        <div className="flex justify-between items-start">
                                            <span className="text-sm font-bold text-white">{tx.type}</span>
                                            <span className="text-[10px] text-zinc-500 font-mono">{tx.id}</span>
                                        </div>
                                        <div className="flex justify-between items-center mt-1">
                                            <span className={`text-[10px] font-medium ${tx.status === 'Verifying' ? 'text-brand-red' : 'text-green-500'
                                                }`}>{tx.status}</span>
                                            <span className="text-[10px] text-zinc-600">{tx.time}</span>
                                        </div>
                                    </div>
                                </div>
                            ))}
                        </div>

                        <div className="mt-6 pt-6 border-t border-white/5 text-center">
                                <a
                                href={`${EXPLORER_URL}/address/${CUP_ADDRESS}`}
                                target="_blank"
                                rel="noopener noreferrer"
                                className="text-sm font-medium text-yellow-400 hover:text-brand-cyan transition-colors"
                            >
                                View AMP Cup Activity
                            </a>
                        </div>
                    </div>
                </div>
            </div>
        </section>
    );
}
