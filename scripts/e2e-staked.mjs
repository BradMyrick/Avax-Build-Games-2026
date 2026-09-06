/**
 * AMP Multiplayer Staked E2E Test — Fuji testnet.
 * 
 * Full pipeline: createLobby → 8×joinLobby (deposit) → 6×signLadder (quorum)
 * → settleMultiplayer → verify payouts + conservation.
 * 
 * Run: node e2e-staked.mjs
 * Requires: web/node_modules (ethers), funded test wallets in /tmp/opencode/e2e-wallets/wallets.json
 */
import { ethers } from "ethers";
import fs from "fs";

const RPC = "https://api.avax-test.network/ext/bc/C/rpc";
const CHAIN_ID = 43113;
const MP_ADDRESS = "0xcabf7b626172fE55d54f03c346563671AbcC77f7";
const N = 8;
const K = Math.floor((2 * N) / 3) + 1; // = 6
const STAKE = ethers.parseEther("0.001");
const BOND = ethers.parseEther("0.00005");
const GAS_LIMIT = 500000;

const MP_ABI = [
  "function createLobby(bytes32 matchId, uint256 gameId, uint64 lobbySize, uint256 stakePerPlayer, uint256 bondPerPlayer, uint16 payoutProfileId, uint64 escrowFillSeconds) external",
  "function joinLobby(bytes32 matchId) external payable",
  "function settleMultiplayer(bytes32 matchId, address[] rankedPlacements, bytes32 transcriptHash, uint256 sessionNonce, uint256 signerBitmask, bytes packedSignatures) external",
  "function claimPayout(bytes32 matchId, address[] rankedPlacements, bytes32 transcriptHash) external",
  "function claimFees(bytes32 matchId, address[] rankedPlacements, bytes32 transcriptHash) external",
  "function withdraw() external",
  "function getMatch(bytes32 matchId) view returns (tuple(uint256,uint64,uint16,uint256,uint256,uint64,uint64,uint64,uint64,uint64,uint8,uint256,address[64],bytes32,bytes32,uint256,uint256,uint256,uint64,uint64,uint96,uint32,uint96,uint96,uint32,uint96,uint96,uint32,bytes32,uint64))",
  "function claimable(address) view returns (uint256)",
  "function quorumOf(uint64) pure returns (uint64)",
  "event LobbyCreated(bytes32 indexed matchId, uint256 indexed gameId, uint64 lobbySize, uint256 stake, uint256 bond, uint16 profileId)",
  "event LobbyReady(bytes32 indexed matchId, uint64 quorumUntil, uint64 graceUntil)",
  "event Settled(bytes32 indexed matchId, bytes32 indexed transcriptHash, uint256 signerCount, uint256 slashedBonds, bool viaGrace)",
];

const LADDER_TYPEHASH = ethers.id("MultiplayerLadder(bytes32 matchId,bytes32 gameId,address[] rankedPlacements,bytes32 transcriptHash,uint256 sessionNonce)");
const DOMAIN_TYPEHASH = ethers.id("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)");

function computeDomainSeparator() {
  return ethers.keccak256(ethers.AbiCoder.defaultAbiCoder().encode(
    ["bytes32", "bytes32", "bytes32", "uint256", "address"],
    [DOMAIN_TYPEHASH, ethers.id("AMPMultiplayer"), ethers.id("1"), CHAIN_ID, MP_ADDRESS]
  ));
}

function computeLadderDigest(matchId, gameId, ranked, transcriptHash, nonce) {
  const domainSep = computeDomainSeparator();
  const rankedRoot = ethers.keccak256(ethers.concat(ranked.map(a => ethers.zeroPadValue(a, 32))));
  const structHash = ethers.keccak256(ethers.AbiCoder.defaultAbiCoder().encode(
    ["bytes32", "bytes32", "bytes32", "bytes32", "uint256"],
    [LADDER_TYPEHASH, matchId, gameId, rankedRoot, transcriptHash, nonce]
  ).slice(2)); // remove 0x
  
  return ethers.keccak256(ethers.concat(["0x1901", domainSep, structHash]));
}

// Simpler: just build the typed data for ethers' signTypedData
function buildTypedData(matchId, gameId, ranked, transcriptHash, nonce) {
  return {
    domain: { name: "AMPMultiplayer", version: "1", chainId: CHAIN_ID, verifyingContract: MP_ADDRESS },
    types: {
      MultiplayerLadder: [
        { name: "matchId", type: "bytes32" },
        { name: "gameId", type: "bytes32" },
        { name: "rankedPlacements", type: "address[]" },
        { name: "transcriptHash", type: "bytes32" },
        { name: "sessionNonce", type: "uint256" },
      ],
    },
    primaryType: "MultiplayerLadder",
    message: { matchId, gameId, rankedPlacements: ranked, transcriptHash, sessionNonce: nonce },
  };
}

async function main() {
  console.log("═".repeat(60));
  console.log(" AMP Multiplayer Staked E2E Test — Fuji Testnet");
  console.log("═".repeat(60));

  const provider = new ethers.JsonRpcProvider(RPC);
  const wallets = JSON.parse(fs.readFileSync("/tmp/opencode/e2e-wallets/wallets.json", "utf8"));

  // Use the deployer as the lobby creator / settler
  const deployerKey = fs.readFileSync("../secrets/fuji-deployer.key", "utf8").trim();
  const deployer = new ethers.Wallet(deployerKey, provider);
  console.log(`\nDeployer: ${deployer.address}`);
  console.log(`Balance: ${ethers.formatEther(await provider.getBalance(deployer.address))} AVAX`);
  console.log(`Players: ${N} (quorum K=${K})`);
  console.log(`Stake: ${ethers.formatEther(STAKE)} AVAX, Bond: ${ethers.formatEther(BOND)} AVAX`);

  const mp = new ethers.Contract(MP_ADDRESS, MP_ABI, deployer);

  // ── Step 1: Create lobby ─────────────────────────────
  console.log("\n── Step 1: Create Lobby ──");
  const matchId = ethers.id(`e2e-staked-${Date.now()}`);
  const gameId = ethers.zeroPadValue("0x01", 32);
  const nonce = BigInt(Date.now());

  console.log(`  matchId: ${matchId}`);
  const tx1 = await mp.createLobby(matchId, gameId, N, STAKE, BOND, 1, 600, { gasLimit: GAS_LIMIT });
  const rc1 = await tx1.wait();
  console.log(`  ✓ Lobby created (tx: ${rc1.hash.slice(0, 16)}…)`);

  // ── Step 2: 8 players join (deposit stake + bond) ────
  console.log("\n── Step 2: Players Join (deposit) ──");
  const deposit = STAKE + BOND;
  for (let i = 0; i < N; i++) {
    const player = new ethers.Wallet(wallets[i].key, provider);
    const mpPlayer = mp.connect(player);
    const tx = await mpPlayer.joinLobby(matchId, { value: deposit, gasLimit: GAS_LIMIT });
    await tx.wait();
    console.log(`  ✓ Player ${i} (${wallets[i].address.slice(0, 8)}…) deposited ${ethers.formatEther(deposit)} AVAX`);
  }

  // Verify lobby is ready
  const matchData = await mp.getMatch(matchId);
  console.log(`  ✓ Lobby state: ${matchData[10]} (1=Open→2=Ready after all join)`);

  // ── Step 3: Compute rankings and sign ladders ────────
  console.log("\n── Step 3: Sign Ladders (EIP-712) ──");
  // Rank players: wallet 0 = rank 1 (winner), wallet 1 = rank 2, etc.
  const ranked = wallets.map(w => w.address);
  const transcriptHash = ethers.id("e2e-test-transcript");
  
  const typedData = buildTypedData(matchId, gameId, ranked, transcriptHash, nonce);
  
  const signatures = [];
  const signerWallets = [];
  for (let i = 0; i < K; i++) { // Only K signers needed
    const player = new ethers.Wallet(wallets[i].key, provider);
    const sig = await player.signTypedData(typedData.domain, typedData.types, typedData.message);
    signatures.push(sig);
    signerWallets.push(player.address);
    console.log(`  ✓ Player ${i} signed ladder (rank ${i + 1})`);
  }

  // Pack signatures: strip 0x prefix, concatenate (65 bytes each)
  const packedSigs = "0x" + signatures.map(s => s.slice(2)).join("");

  // Build signer bitmask: bits 0..K-1
  let bitmask = 0n;
  for (let i = 0; i < K; i++) bitmask |= (1n << BigInt(i));

  console.log(`  Signer bitmask: 0x${bitmask.toString(16)}`);
  console.log(`  Signature count: ${signatures.length} (K=${K})`);

  // ── Step 4: Settle on-chain ──────────────────────────
  console.log("\n── Step 4: Settle On-Chain ──");
  
  // Record balances before settlement
  const balancesBefore = {};
  for (const w of wallets) {
    balancesBefore[w.address] = await mp.claimable(w.address);
  }

  const tx4 = await mp.settleMultiplayer(
    matchId, ranked, transcriptHash, nonce, bitmask, packedSigs,
    { gasLimit: 800000 }
  );
  const rc4 = await tx4.wait();
  console.log(`  ✓ Settled on-chain (tx: ${rc4.hash.slice(0, 16)}…, gas: ${rc4.gasUsed})`);

  // ── Step 5: Verify payouts ───────────────────────────
  console.log("\n── Step 5: Verify Payouts ──");
  
  const matchAfter = await mp.getMatch(matchId);
  console.log(`  Final state: ${matchAfter[10]} (5=Settled)`);

  // Record claimable balances after settlement
  let totalCredited = 0n;
  for (let i = 0; i < N; i++) {
    const claimable = await mp.claimable(wallets[i].address);
    const diff = claimable - balancesBefore[wallets[i].address];
    totalCredited += diff;
    if (diff > 0n) {
      console.log(`  Player ${i} (rank ${i + 1}): ${ethers.formatEther(diff)} AVAX claimable`);
    }
  }

  // Fee recipients
  const treasuryClaimable = await mp.claimable(deployer.address);
  console.log(`  Treasury (deployer): ${ethers.formatEther(treasuryClaimable)} AVAX claimable`);

  // ── Step 6: Verify conservation ───────────────────────
  console.log("\n── Step 6: Conservation Check ──");
  const totalDeposited = deposit * BigInt(N);
  const totalDistributed = totalCredited + treasuryClaimable;
  console.log(`  Total deposited:   ${ethers.formatEther(totalDeposited)} AVAX`);
  console.log(`  Total distributed: ${ethers.formatEther(totalDistributed)} AVAX`);
  console.log(`  Contract balance:  ${ethers.formatEther(await provider.getBalance(MP_ADDRESS))} AVAX (includes other lobbies)`);

  const conservationOk = totalDistributed >= totalDeposited - 1n; // 1 wei tolerance for rounding
  console.log(`  Conservation: ${conservationOk ? "✓ HOLDS" : "✗ VIOLATED"}`);

  // ── Step 7: Player claims payout ──────────────────────
  console.log("\n── Step 7: Player Claims ──");
  const winner = new ethers.Wallet(wallets[0].key, provider);
  const mpWinner = mp.connect(winner);
  const claimTx = await mpWinner.claimPayout(matchId, ranked, transcriptHash, { gasLimit: 300000 });
  await claimTx.wait();
  const winnerClaimable = await mp.claimable(wallets[0].address);
  console.log(`  ✓ Winner claimed payout (remaining claimable: ${ethers.formatEther(winnerClaimable)})`);

  const winnerBalance = await provider.getBalance(wallets[0].address);
  console.log(`  Winner wallet balance: ${ethers.formatEther(winnerBalance)} AVAX`);

  // ── Summary ───────────────────────────────────────────
  console.log("\n" + "═".repeat(60));
  console.log(" ✅ STAKED E2E TEST COMPLETE");
  console.log("═".repeat(60));
  console.log(`  Lobby created, ${N} players deposited, ${K} signed, settled on-chain`);
  console.log(`  Conservation: ${conservationOk ? "VERIFIED" : "FAILED"}`);
  console.log(`  Match: ${matchId}`);
}

main()
  .then(() => process.exit(0))
  .catch(err => {
    console.error("❌ E2E TEST FAILED:", err.message);
    console.error(err.stack);
    process.exit(1);
  });
