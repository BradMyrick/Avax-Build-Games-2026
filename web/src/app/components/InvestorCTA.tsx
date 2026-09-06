"use client";

import { motion } from "framer-motion";
import { Mail, BookOpen, ArrowRight, TrendingUp, Lock, Zap } from "lucide-react";
import Link from "next/link";

const pillars = [
  {
    icon: TrendingUp,
    title: "A Real, Complained-About Gap",
    description:
      "Every community, DAO, and esports org runs tournaments on a broken stack - Discord + Challonge + manual payouts. AMP replaces all of it with one trustless engine.",
  },
  {
    icon: Lock,
    title: "Verifiable By Construction",
    description:
      "Escrowed prize pools, EIP-712 verifier-attested results, and pull-only payouts on Avalanche. Tamper-proof outcomes by design - not by trust.",
  },
  {
    icon: Zap,
    title: "Grant Validated, Ecosystem Connected",
    description:
      "Awarded $15,000 from Avalanche Build Games 2026. Live on Fuji, integrated with the Team1 Avalanche builder community, and ready to scale.",
  },
];

export default function InvestorCTA() {
  return (
    <section id="partner" className="py-32 relative overflow-hidden">
      <div className="absolute top-0 left-1/2 -translate-x-1/2 w-[120%] h-[400px] bg-gradient-to-b from-brand-red/10 via-brand-cyan/5 to-transparent blur-[100px] rounded-full pointer-events-none" />
      <div className="max-w-7xl mx-auto px-6 relative z-10">
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-16 items-center">
          <div>
            <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-brand-red/10 border border-brand-red/30 mb-6">
              <span className="text-xs font-bold text-brand-red tracking-widest uppercase">
                Backed by Avalanche
              </span>
            </div>
            <h2 className="text-4xl md:text-5xl font-black mb-6 text-white leading-tight uppercase tracking-tight">
              Tournament Infrastructure<br />
              <span className="text-brand-cyan">Communities Run On</span>
            </h2>
            <p className="text-zinc-400 text-lg mb-10 leading-relaxed">
              AMP is the verifiable tournament engine for competitive gaming. Brackets, escrowed prize pools, instant payouts, and tamper-proof results - so any community can run a trustless arena in minutes.
            </p>

            <div className="space-y-6 mb-10">
              {pillars.map((p, idx) => (
                <motion.div
                  key={p.title}
                  initial={{ opacity: 0, x: -20 }}
                  whileInView={{ opacity: 1, x: 0 }}
                  viewport={{ once: true }}
                  transition={{ duration: 0.4, delay: idx * 0.1 }}
                  className="flex gap-4 items-start"
                >
                  <div className="w-10 h-10 rounded-lg bg-brand-cyan/10 border border-brand-cyan/20 flex items-center justify-center shrink-0 text-brand-cyan">
                    <p.icon className="w-5 h-5" />
                  </div>
                  <div>
                    <h4 className="font-bold text-white mb-1">{p.title}</h4>
                    <p className="text-sm text-zinc-400 leading-relaxed">
                      {p.description}
                    </p>
                  </div>
                </motion.div>
              ))}
            </div>
          </div>

          <div className="glass-panel p-10 bg-gradient-to-br from-brand-red/5 to-brand-cyan/5 relative">
            <div className="absolute -inset-0.5 bg-gradient-to-br from-brand-red/20 to-brand-cyan/20 opacity-20 blur pointer-events-none rounded-inherit" />
            <div className="relative z-10">
              <h3 className="text-2xl font-black text-white mb-3 uppercase tracking-wide">
                Partner With Us
              </h3>
              <p className="text-zinc-400 mb-8 leading-relaxed">
                Whether you&apos;re a gaming community wanting trustless tournaments, a studio launching competitive play, or a brand seeking to sponsor the next AMP Cup - let&apos;s talk.
              </p>

              <div className="flex flex-col sm:flex-row gap-4">
                <Link
                  href="mailto:brad@kodr.pro"
                  className="px-8 py-4 rounded-sm font-bold text-black bg-brand-cyan hover:bg-white transition-all flex items-center justify-center gap-2 shadow-[0_0_30px_rgba(0,229,255,0.5)] hover:shadow-[0_0_50px_rgba(0,229,255,0.8)] hover:-translate-y-1 transform duration-300 uppercase tracking-widest text-sm border-b-4 border-brand-dark-cyan active:border-b-0 active:translate-y-1"
                >
                  <Mail className="w-5 h-5 text-black" />
                  brad@kodr.pro
                </Link>
                <Link
                  href="/docs"
                  target="_blank"
                  className="px-8 py-4 rounded-sm font-bold text-brand-cyan glass-panel hover:bg-brand-cyan/10 hover:border-brand-cyan/50 transition-colors flex items-center justify-center gap-2 uppercase tracking-widest text-sm border-b-4 border-transparent"
                >
                  <BookOpen className="w-5 h-5" />
                  Read Architecture
                  <ArrowRight className="w-4 h-4" />
                </Link>
              </div>

              <div className="mt-8 pt-6 border-t border-white/10 flex items-center gap-6 text-sm text-zinc-500">
                <div className="flex items-center gap-2">
                  <span className="w-2 h-2 rounded-full bg-green-400" />
                  Live on Fuji
                </div>
                <div className="flex items-center gap-2">
                  <span className="w-2 h-2 rounded-full bg-brand-cyan" />
                  Any engine, any game
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
