import { ethers } from "ethers";

/**
 * AMPTournamentCup — live on Fuji.
 * Sponsor-funded prize-pool escrow: sponsor funds → verifier attests winners →
 * winners pull-claim. This module drives the on-chain side from the browser.
 *
 * Repo standard: network values come from NEXT_PUBLIC_* env vars with
 * documented Fuji defaults (see web/.env.example) — no address or chain id
 * without an env path.
 */
export const CUP_ADDRESS =
  process.env.NEXT_PUBLIC_AMP_CUP_ADDRESS ||
  "0x7c743c1c9ae3e7a65d030098f2249b7787d66dff";
export const FUJI_CHAIN_ID = Number(
  process.env.NEXT_PUBLIC_AMP_CHAIN_ID || "43113",
);
export const FUJI_RPC =
  process.env.NEXT_PUBLIC_AMP_RPC_URL ||
  "https://api.avax-test.network/ext/bc/C/rpc";
/** Block explorer base (no trailing slash) for user-facing links. */
export const EXPLORER_URL =
  process.env.NEXT_PUBLIC_AMP_EXPLORER_URL || "https://testnet.snowtrace.io";

export const AMPCUP_ABI = [
  "function createTournament(uint16[] payoutBps, address verifier, uint64 finalizeDeadline) payable returns (uint256)",
  "function createTournamentERC20(address token, uint256 amount, uint16[] payoutBps, address verifier, uint64 finalizeDeadline) returns (uint256)",
  "function finalizeTournament(uint256 tournamentId, address[] winners, bytes signature)",
  "function claimPrize(uint256 tournamentId, uint256 placement)",
  "function nextTournamentId() view returns (uint256)",
  "function domainSeparator() view returns (bytes32)",
  "function TOURNAMENT_RESULT_TYPEHASH() view returns (bytes32)",
  "function getTournament(uint256) view returns (tuple(address sponsor, address token, uint256 prizePool, uint16[] payoutBps, address verifier, address[] winners, uint8 state, uint64 createdAt, uint64 finalizeDeadline))",
  "event TournamentCreated(uint256 indexed tournamentId, address indexed sponsor, address token, uint256 prizePool, address verifier)",
  "event TournamentFinalized(uint256 indexed tournamentId, bytes32 indexed winnersRoot)",
  "event PrizeClaimed(uint256 indexed tournamentId, uint256 indexed placement, address winner, uint256 amount)",
] as const;

/** EIP-712 domain for AMPTournamentCup finalize signatures. */
export const EIP712_DOMAIN = {
  name: "AMPTournamentCup",
  version: "1",
  chainId: FUJI_CHAIN_ID,
  verifyingContract: CUP_ADDRESS,
} as const;

export const TOURNAMENT_RESULT_TYPE = [
  { name: "tournamentId", type: "uint256" },
  { name: "winners", type: "address[]" },
];

/** Preset payout splits (basis points, sum = 10000). */
export const PAYOUT_PRESETS: Record<string, { label: string; bps: number[] }> = {
  winnerTakesAll: { label: "Winner takes all", bps: [10000] },
  top2: { label: "Top 2 (70 / 30)", bps: [7000, 3000] },
  top3: { label: "Top 3 (60 / 30 / 10)", bps: [6000, 3000, 1000] },
  top4: { label: "Top 4 (50 / 25 / 15 / 10)", bps: [5000, 2500, 1500, 1000] },
};

/** Sign the EIP-712 finalize message with a verifier's signer (browser or key). */
export async function signFinalize(
  signer: ethers.Signer,
  tournamentId: number | bigint,
  winners: string[]
): Promise<string> {
  // Validate + checksum all addresses to prevent ENS resolution attempts.
  const addresses = winners.map((w) => ethers.getAddress(w));
  return signer.signTypedData(
    EIP712_DOMAIN,
    { TournamentResult: TOURNAMENT_RESULT_TYPE },
    { tournamentId, winners: addresses }
  );
}

/** Connect to an injected browser wallet (Core / MetaMask) on Fuji. */
export async function connectWallet(): Promise<ethers.BrowserProvider> {
  const ethereum = (window as unknown as { ethereum?: { request: (a: unknown) => Promise<unknown> } }).ethereum;
  if (!ethereum) throw new Error("No wallet found. Install the Core or MetaMask wallet.");
  await ethereum.request({ method: "eth_requestAccounts" });
  const provider = new ethers.BrowserProvider(ethereum);
  const network = await provider.getNetwork();
  if (Number(network.chainId) !== FUJI_CHAIN_ID) {
    throw new Error(`Switch your wallet to the Avalanche Fuji testnet (chain ${FUJI_CHAIN_ID}).`);
  }
  return provider;
}
