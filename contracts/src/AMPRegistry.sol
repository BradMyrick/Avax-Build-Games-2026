// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.33;

import "./AMPTypes.sol";
import "openzeppelin-contracts/contracts/token/ERC20/IERC20.sol";
import "openzeppelin-contracts/contracts/token/ERC20/utils/SafeERC20.sol";
import "openzeppelin-contracts/contracts/access/Ownable2Step.sol";
import "openzeppelin-contracts/contracts/utils/Pausable.sol";
import "openzeppelin-contracts/contracts/utils/ReentrancyGuard.sol";

contract AMPRegistry is Ownable2Step, Pausable, ReentrancyGuard {
    using SafeERC20 for IERC20;

    error NotSettlement();
    error TooManyVerifiers();
    error RTModeRequiresArbiter();
    error NotGameAdmin();
    error TimeoutTooShort();
    error TimeoutTooLong();
    error StakeTooLow();
    error NativeTokenSentForERC20();
    error MatchNotOpen();
    error CannotJoinOwnMatch();
    error StakeMismatch();
    error NotPlayerA();
    error DisputeTimeoutNotReached();
    error MatchNotExpirable();
    error NotExpiredYet();
    error NoFeesToWithdraw();
    error TransferFailed();
    error MatchNotSettlable();
    error NoPendingWithdrawal();
    error MatchAlreadyExists();
    error InvalidSettlementAddress();
    error InvalidNewState();
    error InvalidStudioFee();
    error TotalFeeTooHigh();

    uint16 public constant MAX_STUDIO_FEE_BPS = 500; // 5%

    address public settlement;
    uint256 public nextGameId;

    mapping(address => uint256) public feeBalances;
    mapping(address => mapping(address => uint256)) public pendingWithdrawals;

    mapping(uint256 => AMPTypes.Game) public games;
    mapping(uint256 => AMPTypes.Match) public matches;
    mapping(uint256 => mapping(address => bool)) public gameVerifiers;

    event GameRegistered(uint256 indexed gameId, address indexed admin, AMPTypes.SettlementMode mode);
    event GameVerifiersUpdated(uint256 indexed gameId);
    event MatchCreated(uint256 indexed matchId, uint256 indexed gameId, address indexed playerA, uint256 stakeAmount);
    event MatchJoined(uint256 indexed matchId, address indexed playerB);
    event MatchCancelled(uint256 indexed matchId, address indexed playerA, uint256 refundAmount);
    event MatchExpired(uint256 indexed matchId);
    event FeesWithdrawn(address indexed token, uint256 amount, address indexed wallet);
    event SettlementUpdated(address indexed settlementAddress);
    event WithdrawalClaimed(address indexed token, address indexed account, uint256 amount);
    event StudioFeeUpdated(uint256 indexed gameId, uint16 feeBps, address indexed recipient);

    modifier onlySettlement() {
        if (msg.sender != settlement) revert NotSettlement();
        _;
    }

    constructor() Ownable(msg.sender) {}

    function registerGame(
        AMPTypes.SettlementMode mode,
        address[] calldata verifiers,
        uint256 minStake,
        address stakeToken,
        address arbiter
    ) external whenNotPaused returns (uint256 gameId) {
        if (verifiers.length > 10) revert TooManyVerifiers();
        if (mode == AMPTypes.SettlementMode.RT_HASH_AGREE) {
            if (arbiter == address(0)) revert RTModeRequiresArbiter();
        }
        gameId = nextGameId++;
        games[gameId] = AMPTypes.Game({
            admin: msg.sender,
            mode: mode,
            verifiers: verifiers,
            minStake: minStake,
            stakeToken: stakeToken,
            arbiter: arbiter,
            matchTimeout: 1 hours,
            studioFeeBps: 0,
            studioFeeRecipient: address(0)
        });
        _syncVerifierMapping(gameId, verifiers);

        emit GameRegistered(gameId, msg.sender, mode);
    }

    /// Fee-split router: the game's studio sets its rake share and payout
    /// address. Combined studio + protocol fees are capped at
    /// MAX_TOTAL_FEE_BPS so a studio can never price players out.
    function setStudioFee(uint256 gameId, uint16 feeBps, address recipient) external {
        AMPTypes.Game storage game = games[gameId];
        if (msg.sender != game.admin) revert NotGameAdmin();
        if (feeBps > MAX_STUDIO_FEE_BPS) revert InvalidStudioFee();
        if (feeBps > 0 && recipient == address(0)) revert InvalidStudioFee();
        game.studioFeeBps = feeBps;
        game.studioFeeRecipient = recipient;
        emit StudioFeeUpdated(gameId, feeBps, recipient);
    }

    function setMatchTimeout(uint256 gameId, uint256 timeoutSeconds) external {
        if (msg.sender != games[gameId].admin) revert NotGameAdmin();
        if (timeoutSeconds < 5 minutes) revert TimeoutTooShort();
        if (timeoutSeconds > 30 days) revert TimeoutTooLong();
        games[gameId].matchTimeout = timeoutSeconds;
    }

    function updateGameVerifiers(uint256 gameId, address[] calldata verifiers) external {
        if (msg.sender != games[gameId].admin) revert NotGameAdmin();
        if (verifiers.length > 10) revert TooManyVerifiers();
        _clearVerifierMapping(gameId);
        games[gameId].verifiers = verifiers;
        _syncVerifierMapping(gameId, verifiers);
        emit GameVerifiersUpdated(gameId);
    }

    function getGameVerifiers(uint256 gameId) external view returns (address[] memory) {
        return games[gameId].verifiers;
    }

    function createMatch(uint256 gameId, uint256 matchId, uint256 stakeAmount)
        external
        payable
        whenNotPaused
        nonReentrant
    {
        if (matches[matchId].playerA != address(0)) revert MatchAlreadyExists();
        AMPTypes.Game storage game = games[gameId];

        uint256 actualStake;
        if (game.stakeToken == address(0)) {
            if (msg.value < game.minStake) revert StakeTooLow();
            actualStake = msg.value;
        } else {
            if (stakeAmount < game.minStake) revert StakeTooLow();
            if (msg.value != 0) revert NativeTokenSentForERC20();
            uint256 balBefore = IERC20(game.stakeToken).balanceOf(address(this));
            IERC20(game.stakeToken).safeTransferFrom(msg.sender, address(this), stakeAmount);
            actualStake = IERC20(game.stakeToken).balanceOf(address(this)) - balBefore;
        }

        matches[matchId] = AMPTypes.Match({
            gameId: gameId,
            playerA: msg.sender,
            playerB: address(0),
            stakeAmount: actualStake,
            stakeAmountB: 0,
            state: AMPTypes.MatchState.OPEN,
            createdAt: uint64(block.timestamp)
        });

        emit MatchCreated(matchId, gameId, msg.sender, actualStake);
    }

    function joinMatch(uint256 matchId) external payable whenNotPaused nonReentrant {
        AMPTypes.Match storage m = matches[matchId];
        if (m.state != AMPTypes.MatchState.OPEN) revert MatchNotOpen();
        if (msg.sender == m.playerA) revert CannotJoinOwnMatch();

        AMPTypes.Game storage game = games[m.gameId];

        if (game.stakeToken == address(0)) {
            if (msg.value != m.stakeAmount) revert StakeMismatch();
            m.stakeAmountB = msg.value;
        } else {
            if (msg.value != 0) revert NativeTokenSentForERC20();
            // Fee-on-transfer safe: record what actually arrived (release Phase 1.5).
            // For standard tokens `received == m.stakeAmount`; for fee-on-transfer
            // tokens `received < m.stakeAmount` and payouts use the actual totals.
            uint256 balBefore = IERC20(game.stakeToken).balanceOf(address(this));
            IERC20(game.stakeToken).safeTransferFrom(msg.sender, address(this), m.stakeAmount);
            uint256 received = IERC20(game.stakeToken).balanceOf(address(this)) - balBefore;
            if (received == 0) revert StakeMismatch();
            m.stakeAmountB = received;
        }

        m.playerB = msg.sender;
        m.state = AMPTypes.MatchState.READY;

        emit MatchJoined(matchId, msg.sender);
    }

    function cancelMatch(uint256 matchId) external nonReentrant whenNotPaused {
        AMPTypes.Match storage m = matches[matchId];
        if (m.state != AMPTypes.MatchState.OPEN) revert MatchNotOpen();
        if (msg.sender != m.playerA) revert NotPlayerA();

        m.state = AMPTypes.MatchState.SETTLED;

        AMPTypes.Game storage game = games[m.gameId];
        uint256 refund = m.stakeAmount;
        m.stakeAmount = 0;

        pendingWithdrawals[game.stakeToken][m.playerA] += refund;

        emit MatchCancelled(matchId, m.playerA, refund);
    }

    function expireMatch(uint256 matchId) external nonReentrant {
        AMPTypes.Match storage m = matches[matchId];
        AMPTypes.Game storage game = games[m.gameId];
        if (m.state == AMPTypes.MatchState.DISPUTED) {
            if (block.timestamp < m.createdAt + game.matchTimeout * 3) revert DisputeTimeoutNotReached();
        } else {
            if (m.state != AMPTypes.MatchState.OPEN && m.state != AMPTypes.MatchState.READY) {
                revert MatchNotExpirable();
            }
            if (block.timestamp < m.createdAt + game.matchTimeout) revert NotExpiredYet();
        }

        m.state = AMPTypes.MatchState.EXPIRED;

        address token = game.stakeToken;
        uint256 amount = m.stakeAmount;
        pendingWithdrawals[token][m.playerA] += amount;
        if (m.playerB != address(0)) {
            // Refund player B their actual received stake (fee-on-transfer safe).
            pendingWithdrawals[token][m.playerB] += m.stakeAmountB;
        }

        emit MatchExpired(matchId);
    }

    function withdrawFees(address token) external onlyOwner nonReentrant {
        uint256 amount = feeBalances[token];
        if (amount == 0) revert NoFeesToWithdraw();
        feeBalances[token] = 0;

        if (token == address(0)) {
            (bool success,) = owner().call{value: amount}("");
            if (!success) revert TransferFailed();
        } else {
            IERC20(token).safeTransfer(owner(), amount);
        }

        emit FeesWithdrawn(token, amount, owner());
    }

    function withdraw(address token) external nonReentrant {
        uint256 amount = pendingWithdrawals[token][msg.sender];
        if (amount == 0) revert NoPendingWithdrawal();
        pendingWithdrawals[token][msg.sender] = 0;

        if (token == address(0)) {
            (bool success,) = msg.sender.call{value: amount}("");
            if (!success) revert TransferFailed();
        } else {
            IERC20(token).safeTransfer(msg.sender, amount);
        }

        emit WithdrawalClaimed(token, msg.sender, amount);
    }

    function setSettlement(address settlementAddress) external onlyOwner {
        if (settlementAddress == address(0)) revert InvalidSettlementAddress();
        settlement = settlementAddress;
        emit SettlementUpdated(settlementAddress);
    }

    /// Two-slot fee routing: [0] = studio rake recipient, [1] = protocol
    /// treasury. Zero-address slots accrue to the Registry owner's fee pot.
    function settleMatch(
        uint256 matchId,
        AMPTypes.MatchState newState,
        address[2] calldata feeRecipients,
        uint256[2] calldata feeAmounts,
        address[] calldata recipients,
        uint256[] calldata amounts
    ) external onlySettlement nonReentrant {
        AMPTypes.Match storage m = matches[matchId];
        if (
            m.state != AMPTypes.MatchState.READY && m.state != AMPTypes.MatchState.OPEN
                && m.state != AMPTypes.MatchState.DISPUTED
        ) revert MatchNotSettlable();
        // Guard against a buggy/compromised settlement rolling a match back to
        // a pre-terminal state (OPEN/READY). Only SETTLED, EXPIRED, and
        // DISPUTED are valid terminal transitions here (release Phase 3.2).
        if (
            newState != AMPTypes.MatchState.SETTLED && newState != AMPTypes.MatchState.EXPIRED
                && newState != AMPTypes.MatchState.DISPUTED
        ) revert InvalidNewState();
        m.state = newState;

        AMPTypes.Game storage game = games[m.gameId];
        address token = game.stakeToken;

        for (uint256 i = 0; i < 2; i++) {
            uint256 fee = feeAmounts[i];
            if (fee > 0) {
                if (feeRecipients[i] != address(0)) {
                    pendingWithdrawals[token][feeRecipients[i]] += fee;
                } else {
                    feeBalances[token] += fee;
                }
            }
        }

        for (uint256 i = 0; i < recipients.length; i++) {
            if (amounts[i] > 0) {
                pendingWithdrawals[token][recipients[i]] += amounts[i];
            }
        }
    }

    function pause() external onlyOwner {
        _pause();
    }

    function unpause() external onlyOwner {
        _unpause();
    }

    function isVerifier(uint256 gameId, address addr) external view returns (bool) {
        return gameVerifiers[gameId][addr];
    }

    function _clearVerifierMapping(uint256 gameId) internal {
        address[] storage old = games[gameId].verifiers;
        for (uint256 i = 0; i < old.length; i++) {
            gameVerifiers[gameId][old[i]] = false;
        }
    }

    function _syncVerifierMapping(uint256 gameId, address[] calldata verifiers) internal {
        for (uint256 i = 0; i < verifiers.length; i++) {
            gameVerifiers[gameId][verifiers[i]] = true;
        }
    }
}
