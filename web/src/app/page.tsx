import Link from "next/link";
import MatchmakingDemo from "./components/MatchmakingDemo";
import FujiNetwork from "./components/FujiNetwork";
import RotatingText from "./components/RotatingText";
import EngineTabs from "./components/EngineTabs";
import GrantBadge from "./components/GrantBadge";
import SecurityAudits from "./components/SecurityAudits";
import InvestorCTA from "./components/InvestorCTA";
import TournamentShowcase from "./components/TournamentShowcase";
import WhyAmp from "./components/WhyAmp";
import { Rocket, Coins, ShieldCheck, Github, Trophy } from "lucide-react";

export default function Home() {
  return (
    <div className="relative min-h-screen overflow-hidden antialiased bg-black text-white">
      {/* Dynamic Background Elements */}
      <div className="absolute top-0 -left-1/4 w-[150%] h-[500px] bg-brand-cyan/10 blur-[120px] rounded-full pointer-events-none" />
      <div className="absolute bottom-0 -right-1/4 w-[150%] h-[500px] bg-brand-red/10 blur-[120px] rounded-full pointer-events-none" />

      {/* Navigation */}
      <header className="fixed top-0 w-full z-50 glass-panel border-b border-brand-cyan/10 rounded-none bg-black/60 relative">
        <div className="absolute bottom-0 left-0 w-full h-[1px] bg-gradient-to-r from-transparent via-brand-cyan/50 to-transparent" />
        <div className="max-w-7xl mx-auto px-6 h-20 flex items-center justify-between">
          <div className="flex items-center gap-3 bg-black/40 border border-brand-cyan/30 px-5 py-2.5 rounded-2xl hover:bg-black/60 hover:border-brand-cyan/60 transition-all duration-300 backdrop-blur-xl shadow-[0_0_20px_rgba(0,229,255,0.1)] hover:shadow-[0_0_30px_rgba(0,229,255,0.3)] group cursor-pointer">
            <div className="w-10 h-10 rounded-xl overflow-hidden shadow-[0_0_15px_rgba(0,229,255,0.4)] group-hover:scale-105 transition-transform duration-300 bg-black flex items-center justify-center p-1">
              <img src="/amp-icon.png" alt="AMP Icon" className="w-full h-full object-contain drop-shadow-[0_0_8px_rgba(255,255,255,0.8)]" />
            </div>
            <div className="flex flex-col justify-center">
              <span className="text-xl md:text-2xl font-black tracking-widest uppercase text-white drop-shadow-[0_0_8px_rgba(0,229,255,0.5)] leading-none mb-0.5">AMP</span>
              <span className="text-[10px] text-zinc-400 font-medium tracking-widest uppercase leading-none hidden sm:block">Tournaments</span>
            </div>
          </div>

          <nav className="hidden md:flex gap-8 text-sm font-medium text-zinc-300">
            <Link href="/arena" className="hover:text-white hover:text-brand-cyan transition-colors duration-300">Play Ranked</Link>
            <Link href="#features" className="hover:text-white hover:text-brand-cyan transition-colors duration-300">Features</Link>
            <Link href="#why-amp" className="hover:text-white hover:text-brand-cyan transition-colors duration-300">Why AMP</Link>
            <Link href="#showcase" className="hover:text-white hover:text-brand-cyan transition-colors duration-300">Showcase</Link>
            <Link href="#demo" className="hover:text-white hover:text-brand-cyan transition-colors duration-300">How It Works</Link>
            <Link href="#fuji" className="hover:text-white hover:text-brand-cyan transition-colors duration-300">Prize Pools</Link>
            <Link href="#api" className="hover:text-white hover:text-brand-cyan transition-colors duration-300">API</Link>
            <Link href="/dashboard" className="hover:text-brand-cyan transition-colors duration-300">Dashboard</Link>
          </nav>

          <Link
            href="https://github.com/bradmyrick/Avalanche-Matchmaking-Protocol"
            target="_blank"
            className="hidden md:flex bg-gradient-to-r from-brand-cyan/10 to-transparent hover:from-brand-cyan/20 text-white px-6 py-2.5 rounded-2xl font-bold transition-all hover:scale-105 active:scale-95 border border-brand-cyan/20 hover:border-brand-cyan/50 items-center gap-2 shadow-[0_0_15px_rgba(0,229,255,0.1)] hover:shadow-[0_0_25px_rgba(0,229,255,0.3)] backdrop-blur-md"
          >
            <Github className="w-5 h-5 text-brand-cyan" />
            GitHub
          </Link>
        </div>
      </header>

      {/* Hero Section */}
      <main className="relative z-10 pt-40">
        <section className="max-w-7xl mx-auto px-6 flex flex-col items-center text-center pb-20">
          <div className="relative mb-24 group mt-12">
            {/* Ambient intense glow behind the logo */}
            <div className="absolute -inset-10 bg-gradient-to-t from-brand-cyan/20 to-brand-red/10 blur-[100px] rounded-full opacity-60 group-hover:opacity-100 transition-opacity duration-1000 pointer-events-none" />

            <div className="relative transform-gpu transition-all duration-1000 hover:-translate-y-4 shadow-[0_40px_100px_-20px_rgba(0,0,0,1)] hover:shadow-[0_60px_120px_-20px_rgba(0,229,255,0.3)] rounded-[3rem] border border-white/10 bg-gradient-to-b from-white/5 to-transparent p-4 md:p-6 backdrop-blur-sm z-20">
              <img
                src="/amp-logo.png"
                alt="AMP Logo"
                className="w-72 md:w-[28rem] lg:w-[36rem] h-auto rounded-[2.5rem] drop-shadow-[0_50px_40px_rgba(0,0,0,0.9)]"
              />
            </div>
          </div>

          {/* Grant Badge */}
          <GrantBadge />

          <h1 className="text-5xl md:text-7xl lg:text-8xl font-black tracking-tighter mb-8 leading-[1.1] uppercase drop-shadow-[0_0_30px_rgba(0,229,255,0.2)]">
            <span className="text-brand-quaternary-cyan drop-shadow-[0_0_25px_rgba(0,229,255,0.4)]">OPEN</span><br />
            MATCHMA<span className="text-yellow-400 drop-shadow-[0_0_20px_rgba(250,204,21,0.8)]">KING</span><br />

            <span className="text-4xl md:text-5xl text-zinc-400 lowercase font-medium tracking-normal block my-4">for</span>
            <RotatingText />
          </h1>

          <p className="max-w-4xl text-xl md:text-2xl text-zinc-300 mb-12 leading-relaxed font-medium">
            Real ranked matchmaking — queue up, get matched, climb the ladder. <br className="hidden md:block" />
            <strong className="text-brand-cyan font-bold tracking-wide">AMP</strong> runs the queue, rates every player, and attests results on-chain. <br className="hidden md:block" />
            <span className="text-white/90">Stake AVAX when it matters. Tournaments when it counts.</span>
          </p>

          <div className="flex flex-col sm:flex-row gap-6 mb-24">
            <Link
              href="/arena"
              className="px-8 py-4 rounded-sm font-bold text-black bg-brand-cyan hover:bg-white transition-all flex items-center justify-center gap-2 shadow-[0_0_30px_rgba(0,229,255,0.5)] hover:shadow-[0_0_50px_rgba(0,229,255,0.8)] hover:-translate-y-1 transform duration-300 uppercase tracking-widest text-sm border-b-4 border-brand-dark-cyan active:border-b-0 active:translate-y-1"
            >
              <Rocket className="w-5 h-5 text-black" />
              Play Ranked — Free
            </Link>
            <Link
              href="/setup"
              className="px-8 py-4 rounded-sm font-bold text-brand-cyan glass-panel hover:bg-brand-cyan/10 hover:border-brand-cyan/50 transition-colors flex items-center justify-center gap-2 uppercase tracking-widest text-sm border-b-4 border-transparent hover:shadow-[0_0_30px_rgba(0,229,255,0.2)]"
            >
              <Trophy className="w-5 h-5 text-brand-cyan" />
              Host a Tournament
            </Link>
          </div>

          {/* Core Features Grid */}
          <div id="features" className="w-full grid grid-cols-1 md:grid-cols-3 gap-6 text-left scroll-mt-32 relative z-10">
            <div className="glass-panel p-8 hover:bg-brand-cyan/5 transition-colors group">
              <div className="w-12 h-12 rounded-sm bg-brand-cyan/20 flex items-center justify-center mb-6 text-brand-cyan group-hover:scale-110 transition-transform shadow-[0_0_15px_rgba(0,229,255,0.3)] border border-brand-cyan/30">
                <Rocket className="w-6 h-6" />
              </div>
              <h3 className="text-xl font-bold mb-3 text-white uppercase tracking-wide">Spin Up in Minutes</h3>
              <p className="text-zinc-400 leading-relaxed font-medium">Configure a bracket, fund the prize pool, and open registration. No servers to babysit, no Solidity to learn. Your community is competing by tonight.</p>
            </div>

            <div className="glass-panel p-8 hover:bg-yellow-400/5 transition-colors group relative overflow-hidden">
              <div className="absolute top-0 right-0 w-32 h-32 bg-yellow-400/10 rounded-full blur-2xl -mr-16 -mt-16 pointer-events-none group-hover:bg-yellow-400/30 transition-colors" />
              <div className="w-12 h-12 rounded-sm bg-yellow-400/20 flex items-center justify-center mb-6 text-yellow-400 group-hover:scale-110 transition-transform shadow-[0_0_15px_rgba(250,204,21,0.3)] border border-yellow-400/30">
                <Coins className="w-6 h-6" />
              </div>
              <h3 className="text-xl font-bold mb-3 text-white uppercase tracking-wide">Escrowed Prize Pools</h3>
              <p className="text-zinc-400 leading-relaxed font-medium">Sponsors deposit USDC or AVAX into on-chain escrow. Champions pull-claim instantly. No manual payouts, no trust, no rug - TimelockController-governed end to end.</p>
            </div>

            <div className="glass-panel p-8 hover:bg-brand-red/5 transition-colors group">
              <div className="w-12 h-12 rounded-sm bg-brand-dark-red/40 flex items-center justify-center mb-6 text-brand-red group-hover:scale-110 transition-transform shadow-[0_0_15px_rgba(232,65,66,0.3)] border border-brand-red/30">
                <ShieldCheck className="w-6 h-6" />
              </div>
              <h3 className="text-xl font-bold mb-3 text-white uppercase tracking-wide">Verifiable Results</h3>
              <p className="text-zinc-400 leading-relaxed font-medium">Every outcome is EIP-712 signed and committed on Avalanche. Tamper-proof brackets, dispute-resistant champions, and a portable skill résumé that follows every player.</p>
            </div>
          </div>
        </section>

        {/* Showcase - tournament media (photos/clips) */}
        <TournamentShowcase />

        {/* The three hard problems — cold start, oracle-free results, the moat */}
        <WhyAmp />

        {/* Interactive Demo - the 4-step flow */}
        <MatchmakingDemo />

        {/* Fuji Network - on-chain prize pools */}
        <FujiNetwork />

        {/* SDK Section */}
        <section id="api" className="py-32 max-w-7xl mx-auto px-6 scroll-mt-32 relative">
          <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[120%] h-[300px] bg-brand-cyan/5 blur-[150px] rounded-full pointer-events-none" />
          <EngineTabs />
        </section>

        {/* Security Audits */}
        <SecurityAudits />

        {/* Investor / Partnership CTA */}
        <InvestorCTA />
      </main>

      {/* Footer */}
      <footer className="py-12 border-t border-brand-cyan/20 bg-black/80 backdrop-blur-xl relative z-10">
        <div className="absolute top-0 left-0 w-full h-[1px] bg-gradient-to-r from-transparent via-brand-cyan/50 to-transparent" />
        <div className="max-w-7xl mx-auto px-6 flex flex-col md:flex-row justify-between items-center gap-6">
          <div className="flex items-center gap-3">
            <div className="w-6 h-6 rounded-sm bg-gradient-to-tr from-brand-dark-cyan to-brand-cyan shadow-[0_0_10px_rgba(0,229,255,0.5)]" />
            <span className="font-bold tracking-widest uppercase text-white">AMP Protocols</span>
          </div>
          <div className="flex items-center gap-2">
            <p className="text-sm text-zinc-500 font-medium">
              &copy; 2026 Avalanche Matchmaking Protocol. Backed by Avalanche Build Games 2026.
            </p>
          </div>
          <div className="flex gap-6 text-zinc-400 font-medium uppercase tracking-wider text-xs">
            <Link href="https://github.com/bradmyrick/Avalanche-Matchmaking-Protocol" className="hover:text-brand-cyan transition-colors">GitHub</Link>
            <Link href="mailto:brad@kodr.pro" className="hover:text-brand-cyan transition-colors">Contact</Link>
            <Link href="/terms" className="hover:text-brand-cyan transition-colors">Terms</Link>
            <Link href="https://docs.page/bradmyrick/Avalanche-Matchmaking-Protocol" target="_blank" className="hover:text-brand-cyan transition-colors">Docs</Link>
          </div>
        </div>
      </footer>
    </div>
  );
}
