#set page(
  paper: "a4",
  margin: (top: 1.8cm, bottom: 1.8cm, left: 1.6cm, right: 1.6cm),
  header: context {
    if counter(page).get().first() > 1 {
      text(size: 7.5pt, fill: rgb("#475569"))[
        *Avalanche Matchmaking Protocol (AMP v2)* --- Algorithmic Foundations & Protocol Invariants
        #h(1fr)
        September 2026
      ]
      v(0.2em)
      line(length: 100%, stroke: 0.4pt + rgb("#cbd5e1"))
    }
  },
  footer: context {
    line(length: 100%, stroke: 0.4pt + rgb("#cbd5e1"))
    v(0.2em)
    text(size: 7.5pt, fill: rgb("#64748b"))[
      AMP v2 Protocol Specification --- *DRAFT* internal snapshot (`5bbc816`)
      #h(1fr)
      Page #counter(page).display() of #counter(page).final().first()
    ]
  },
)

#set text(size: 8.6pt, lang: "en")
#set par(justify: true, leading: 0.52em)
#set heading(numbering: "1.")

// Heavy identities never wrap across columns/pages.
#show math.equation: set block(breakable: false)

#show heading.where(level: 1): it => block(above: 1.1em, below: 0.5em)[
  #text(weight: "bold", size: 9.3pt, fill: rgb("#0f172a"))[#it]
  #v(-3pt)
  #line(length: 100%, stroke: 0.5pt + rgb("#94a3b8"))
]

// --- Atomic Callout Blocks ---
#let defn(num, title, body) = block(
  width: 100%,
  stroke: (left: 2.5pt + rgb("#0284c7")),
  fill: rgb("#f8fafc"),
  inset: (left: 7pt, right: 7pt, top: 5pt, bottom: 5pt),
  radius: (right: 2pt),
  above: 0.65em,
  below: 0.65em,
  breakable: false,
  [
    #text(weight: "bold", fill: rgb("#0369a1"))[Definition #num (#title).] \
    #body
  ],
)

#let thm(num, title, body) = block(
  width: 100%,
  stroke: (left: 2.5pt + rgb("#e11d48")),
  fill: rgb("#fffafb"),
  inset: (left: 7pt, right: 7pt, top: 5pt, bottom: 5pt),
  radius: (right: 2pt),
  above: 0.65em,
  below: 0.65em,
  breakable: false,
  [
    #text(weight: "bold", fill: rgb("#9f1239"))[Theorem #num (#title).] \
    #text(style: "italic")[#body]
  ],
)

#let inv(num, title, body) = block(
  width: 100%,
  stroke: (left: 2.5pt + rgb("#059669")),
  fill: rgb("#f0fdf4"),
  inset: (left: 7pt, right: 7pt, top: 5pt, bottom: 5pt),
  radius: (right: 2pt),
  above: 0.65em,
  below: 0.65em,
  breakable: false,
  [
    #text(weight: "bold", fill: rgb("#047857"))[Protocol Invariant #num (#title).] \
    #body
  ],
)

#let remark(title, body) = block(
  width: 100%,
  stroke: (left: 2pt + rgb("#64748b")),
  fill: rgb("#f1f5f9"),
  inset: (left: 6pt, right: 6pt, top: 4pt, bottom: 4pt),
  radius: (right: 2pt),
  above: 0.5em,
  below: 0.5em,
  breakable: false,
  [
    #text(weight: "bold", fill: rgb("#334155"))[Remark (#title).] \
    #text(size: 8pt)[#body]
  ],
)

#let proof(body) = block(
  width: 100%,
  above: 0.3em,
  below: 0.65em,
  [
    #text(style: "italic", weight: "bold")[Proof.] #body #h(1fr) $qed$
  ],
)

// ==========================================
// TITLE & METADATA (FULL WIDTH)
// ==========================================
#align(center)[
  #v(-0.2cm)
  #block(width: 90%)[
    #text(size: 14pt, weight: "bold", fill: rgb("#0f172a"), hyphenate: false)[
      Algorithmic Foundations and Protocol Invariants for the Avalanche Matchmaking Protocol (AMP v2)
    ]
  ]

  #v(0.08cm)
  #text(size: 8.8pt, style: "italic", fill: rgb("#334155"))[Quorum Intersection Proofs, State-Exhaustive Value
    Conservation, and Game-Theoretic Settlement Analysis]

  #v(0.12cm)
  #text(size: 7.6pt, fill: rgb("#b45309"))[*DRAFT --- internal spec snapshot. Not for external distribution.*]

  #v(0.1cm)
  #text(size: 8.8pt, weight: "bold")[Brad Myrick] \
  #text(size: 7.8pt, fill: rgb("#475569"))[Avalanche Matchmaking Protocol --- #link(
      "https://playwithamp.xyz",
    )[playwithamp.xyz]]
  #v(0.2cm)
]

// ==========================================
// ABSTRACT (FULL WIDTH)
// ==========================================
#align(center)[
  #block(
    width: 98%,
    fill: rgb("#f8fafc"),
    stroke: 0.5pt + rgb("#cbd5e1"),
    inset: 6pt,
    radius: 3pt,
    [
      #align(center)[#text(weight: "bold", size: 8.2pt, fill: rgb("#0f172a"))[Abstract]]
      #v(1pt)
      #align(left)[#text(size: 7.8pt, fill: rgb("#1e293b"))[
        This paper establishes the mathematical specification and algorithmic invariants for the Avalanche Matchmaking
        Protocol (AMP v2). We analyze the safety, economic finality, and queue dynamics of decentralized, oracle-free
        $N$-player match settlement. Specifically, we prove: (1) Byzantine fault-tolerant $K$-of-$N$ quorum intersection
        under non-equivocation assumptions across all residue classes ($N >= 3f + 1$); (2) state-exhaustive integer
        conservation of pooled stakes down to 1 wei across all terminal match states (`Settled`, `TimeoutClaim`,
        `FundingExpired`, `SilenceRefund`, `DisputeResolved`); (3) permutation invariance of multi-opponent Glicko-2
        field updates over $bb(R)$ with a canonically ordered computational model; (4) game-theoretic dominance of
        dual-deposit reporting bonds in pivotal dropout scenarios; and (5) conditional uniformity bounds of candidate
        lobby assignment.
      ]]
      #v(1pt)
      #align(left)[#text(size: 6.8pt, fill: rgb("#64748b"))[
        *Keywords:* Byzantine Quorum Systems, Mechanism Design, Decentralized Matchmaking, Glicko-2, Value Conservation,
        Avalanche L1 Subnets.
      ]]
    ],
  )
]

#v(0.15cm)

// ==========================================
// TWO-COLUMN TECHNICAL BODY
// ==========================================
#show: columns.with(2, gutter: 14pt)

= Byzantine Quorum Intersection

Let an $N$-player match session be represented by participants $cal(P) = cal(H) union cal(B)$, where $cal(H)$ denotes
honest participants, $cal(B)$ denotes Byzantine or colluding participants, and $|cal(H) inter cal(B)| = 0$. We bound the
Byzantine population by $|cal(B)| <= f$.

#defn(1, "Quorum Threshold")[
  For a lobby of size $N$, canonical ladder settlement requires a concordant quorum of $K$ participant signatures:
  $ K = floor(2N \/ 3) + 1, quad K = N "for" N <= 3 $
]

Each participant holds an ECDSA keypair $(text("sk")_i, text("pk")_i)$. An attestation commits to the EIP-712 session
digest over the structured message
$ m = "keccak256"("matchId" ‖ "chainId" ‖ "sessionNonce" ‖ arrow(R) ‖ H_"transcript") $
where $arrow(R)$ is the ranked placement vector and $H_"transcript"$ is the deterministic execution trace hash.

#thm(1, "Quorum Non-Equivocation")[
  For any $N >= 3f + 1$, two conflicting match states $m != m'$ cannot both achieve a valid quorum of size $K$ without
  at least one Byzantine participant producing two distinct signatures.
]

#proof[
  Let $Q_1, Q_2 subset.eq cal(P)$ be two valid quorums supporting conflicting states $m != m'$, with $|Q_1| >= K$ and
  $|Q_2| >= K$. By inclusion-exclusion:
  $ |Q_1 inter Q_2| = |Q_1| + |Q_2| - |Q_1 union Q_2| >= 2K - N $

  We evaluate $2K - N$ across all residue classes $N = 3f + r$ where $r in {1, 2, 3}$:

  - *Case 1* ($r = 1, N = 3f + 1$): $K = floor((6f + 2) \/ 3) + 1 = 2f + 1$, hence
    $ |Q_1 inter Q_2| >= 2(2f + 1) - (3f + 1) = f + 1. $

  - *Case 2* ($r = 2, N = 3f + 2$): $K = floor((6f + 4) \/ 3) + 1 = 2f + 2$, hence
    $ |Q_1 inter Q_2| >= 2(2f + 2) - (3f + 2) = f + 2 >= f + 1. $

  - *Case 3* ($r = 3, N = 3f + 3$): $K = floor((6f + 6) \/ 3) + 1 = 2f + 3$, hence
    $ |Q_1 inter Q_2| >= 2(2f + 3) - (3f + 3) = f + 3 >= f + 1. $

  Thus, $|Q_1 inter Q_2| >= f + 1$ holds for all $N >= 3f + 1$. Because the total Byzantine population is bounded by
  $|cal(B)| <= f$, the intersection contains at least one honest participant:
  $ |(Q_1 inter Q_2) inter cal(H)| >= (f + 1) - f = 1 $

  Let $p^* in (Q_1 inter Q_2) inter cal(H)$. By definition of an honest node, $p^*$ signs at most one digest per session
  nonce:
  $ sigma_(p^*)(m) => not exists sigma_(p^*)(m') quad "for" m' != m $

  Therefore, two conflicting quorums cannot both exist unless at least one Byzantine node $p in cal(B)$ equivocally
  signs both $m$ and $m'$. Submitting both signatures on-chain constitutes non-repudiable proof of fraud, slashing $p$.
]

#remark("Liveness Bound & Threat Boundary")[
  At $N = 3f + 1$, honest participant count $|cal(H)| = 2f + 1 = K$. A single honest crash-fault ($|cal(H)| - 1$)
  starves immediate concordant quorum, triggering the optimistic grace timeout.

  Furthermore, Theorem 1 guarantees *consensus non-equivocation*, not game correctness: if $K$ Byzantine players collude
  to sign a fraudulent ladder with matching $H_"transcript"$, the contract accepts it unless an honest minority
  escalates inputs to the optimistic dispute challenge window.
]

= State-Exhaustive Value Conservation

Match escrow must satisfy exact mathematical conservation across all terminal execution states to prevent protocol
insolvency.

#defn(2, "Dual-Deposit Escrow and Per-State Inflow")[
  Every participant $i in cal(P)$ deposits stake $S$ and reporting bond $B_"rep"$, so $D_i = S + B_"rep"$ and the
  deposit base is
  $ cal(E)_"deposits" = sum_(i=1)^N D_i = N S + N B_"rep". $
  If a dispute is opened, each challenging faction additionally escrows a challenge stake $C_c$ at challenge time;
  the *inflow to a terminal state* $s$ is the value actually escrowed entering that state:
  $ cal(E)_"in" (s) = cases(N S + N B_"rep" + sum_c C_c & "if a dispute was opened", N S + N B_"rep" & "otherwise.") $
  Conservation claims are always relative to $cal(E)_"in" (s)$ --- never to a fixed constant.
]

Let $Omega_"gross" = N S$. Gross protocol fee at basis points $beta_"rake" in [0, 10000]$ is:
$ F_"gross" = floor((Omega_"gross" beta_"rake") / 10000), quad Omega_"net" = Omega_"gross" - F_"gross" $
$ F_"studio" = floor((F_"gross" beta_"studio") / 10000), quad F_"protocol" = F_"gross" - F_"studio" $

#inv(1, "Zero-Loss Division Remainder")[
  For $T$ prize tiers (${t_1, dots, t_T}$, $sum t_j = 10000$), preliminary payouts
  $P_j = floor((Omega_"net" dot t_j) / 10000)$ leave remainder $rho = Omega_"net" - sum_(j=1)^T P_j$. By construction:
  $ 0 <= rho < T "wei" $
  The implementation routes $rho$ to the protocol reserve as a fee, so
  $ sum_(j=1)^T P_j + rho = Omega_"net" $ holds exactly and no value is stranded.
]

#thm(2, "State-Exhaustive Conservation")[
  In every terminal match state
  $s in cal(T)_"states" = {"Settled", "TimeoutClaim", "FundingExpired", "SilenceRefund", "DisputeResolved"}$,
  total ledger credits equal the per-state inflow:
  $ sum "Outflows"(s) = cal(E)_"in" (s) quad "to 1 wei." $
]

#proof[
  We evaluate outflows across all five terminal states. Payout delivery is lazy (prove-your-payout claims); the
  identities hold for the *ledger* the moment the terminal state is recorded, since every credit below is either
  written at resolution or is a deterministic function of the recorded ladder and immutable tier profile.

  - *State 1 (`Settled`):* Quorum $|cal(S)| >= K$ reached during the quorum window. Net pool distributes via normalized
    tiers (plus remainder $rho$ to the reserve); signers receive $B_"rep"$ back; non-signers
    $cal(M) = cal(P) \ cal(S)$ have their bonds slashed and split 50/50 between the relayer fund and Rank 1:
    $
      sum "Out" & = (Omega_"net" - rho) + rho + F_"gross" + |cal(S)| B_"rep" \
                 & quad + floor((|cal(M)| B_"rep")/2) + (|cal(M)| B_"rep" - floor((|cal(M)| B_"rep")/2)) \
                 & = N S + (|cal(S)| + |cal(M)|) B_"rep" = N S + N B_"rep" = cal(E)_"in" (s).
    $

  - *State 2 (`TimeoutClaim`):* The quorum window lapses; the ladder's Rank-1 participant posts a unilateral claim,
    unchallenged through the grace window. *Slash rule (as shipped):* every participant _except the claimant_ forfeits
    their bond --- the claimant's signature is the only one recorded in this state, so "all bonds but the claimant's"
    is exactly the non-signer set of the recorded settlement. The claimant recovers their own bond plus the winners'
    half of the $N - 1$ slashed bonds (the relayer takes the other half):
    $
      sum "Out" & = Omega_"net" + F_"gross" + B_"rep" + (N - 1) B_"rep" \
                 & = N S + N B_"rep" = cal(E)_"in" (s).
    $

  - *State 3 (`FundingExpired`):* The lobby fails to fill before the escrow deadline; only the set
    $J$ of joined participants ($|J| < N$) ever deposited, so the per-state inflow is
    $cal(E)_"in" (s) = |J| (S + B_"rep")$. Each joined participant claims a full refund:
    $ sum "Out" = sum_(j in J) (S + B_"rep") = |J| (S + B_"rep") = cal(E)_"in" (s). $

  - *State 4 (`SilenceRefund`):* The lobby filled but the quorum and grace windows lapse with *no* claim at all
    (mass disconnect). No rake, no slash; every deposit refunds:
    $ sum "Out" = sum_(i in cal(P)) (S + B_"rep") = N S + N B_"rep" = cal(E)_"in" (s). $

  - *State 5 (`DisputeResolved`):* A $>= 2$-signer concordant challenge contested the grace claim; the bonded verifier
    ruled, and the challenge stakes $sum_c C_c$ are part of the inflow per Definition 2. Disputes take no rake; the
    valid ladder's tiers are paid over the gross stake pool $N S$ (losers' stakes are thereby consumed by the ladder
    itself); non-losers recover bonds; the losers' bonds plus all challenge stakes form the slash pool, split 70% to
    the winning faction and 30% to the protocol reserve (rounding dust to the reserve):
    $
      sum "Out" & = underbrace(N S, "tiers over gross") + underbrace((N - ell) B_"rep", "non-loser bonds") \
                 & quad + underbrace((ell B_"rep" + sum_c C_c), "slash pool, fully split") \
                 & = N S + N B_"rep" + sum_c C_c = cal(E)_"in" (s),
    $
    where $ell$ is the losing faction's size.

  Conservation holds to 1 wei across all terminal states, relative to the value actually escrowed in each.
]

= Multi-Opponent Rating Invariance

To update skill ratings in fields of $N$ players, `amp-match-core` executes `glicko2_update_vs_many` across all $N-1$
opponents within a single rating period.

#defn(3, "Field Outcomes & Glicko-2 Formulation")[
  For player $i$ facing opponent $j != i$, outcome score $s_(i j)$ is:
  $
    s_(i j) = cases(
      1.0 & "if" "rank"(i) < "rank"(j),
      0.5 & "if" "rank"(i) = "rank"(j),
      0.0 & "if" "rank"(i) > "rank"(j)
    )
  $
  With scale transformations $mu = (r - 1500) \/ 173.7178$ and $phi = "RD" \/ 173.7178$ (where $"RD"$ is the rating
  deviation), let $E_(i j) = (1 + exp(-g(phi_j)(mu_i - mu_j)))^(-1)$ and $g(phi) = (1 + 3 phi^2 \/ pi^2)^(-1\/2)$:
  $
    v_i = (sum_(j != i) g(phi_j)^2 E_(i j) (1 - E_(i j)))^(-1), quad Delta_i = v_i sum_(j != i) g(phi_j) (s_(i j) - E_(i j))
  $
]

#thm(3, [Permutation Invariance over $bb(R)$])[
  Let $pi: {1, dots, N-1} -> {1, dots, N-1}$ be any permutation of opponent processing order. The mathematical tuple
  $(mu_i', phi_i', sigma_i') in bb(R)^3$ is invariant under $pi$.
]

#proof[
  Functions $psi(i, j) = g(phi_j)^2 E_(i j) (1 - E_(i j))$ and $omega(i, j) = g(phi_j)(s_(i j) - E_(i j))$ depend
  strictly on the unordered pair $\{i, j\}$. Over the reals, addition is commutative and associative:
  $
    sum_(k=1)^(N-1) psi(i, pi(k)) = sum_(j != i) psi(i, j), quad sum_(k=1)^(N-1) omega(i, pi(k)) = sum_(j != i) omega(i, j)
  $

  Thus, scalar field parameters $v_i$ and $Delta_i$ are invariant under $pi$. Volatility $sigma_i'$ is the unique zero
  of the objective function:
  $ f(x) = (e^x (Delta_i^2 - phi_i^2 - v_i - e^x)) / (2(phi_i^2 + v_i + e^x)^2) - (x - ln sigma_i^2) / tau^2 $

  Because $f(x)$ is parameterized entirely by invariant scalars, its root $x^*$ is invariant under $pi$. Derived
  quantities $phi_i'$ and $mu_i' = mu_i + (phi_i')^2 sum omega(i, j)$ are identical over $bb(R)$.
]

#remark("Computational Model & Modeling Assumptions")[
  Floating-point addition (IEEE-754) is non-associative. To guarantee bit-identical updates across different node
  architectures, `amp-match-core` enforces a *canonical accumulation order*: opponent contributions are sorted by the
  opponent tuple's IEEE-754 bit patterns (rating, then deviation, then score) before summation, making the floating-
  point sum a deterministic function of the opponent *multiset*. This property is fuzz-gated: any permutation of an
  opponent field must yield bit-identical $(mu', phi', sigma')$.

  *Modeling Note:* Treating an $N$-player ranking as $N-1$ independent pairwise matches is a standard Glickman
  rating-period approximation. Because tournament ranks are dependent, this models directional skill delta accurately
  but contracts $phi$ faster than a joint multi-variate model.
]

= Game-Theoretic Settlement Analysis

We evaluate the strategic incentives of an eliminated player $k$ who placed outside the payout tiers ($r_k > T$) and
considers withholding their signature to cause a match timeout.

#defn(4, "Pivotal Dropout Game Parameters")[
  Let $S > 0$ be the match stake, $B_"rep" > 0$ be the reporting bond, and $C_"sign" approx 0$ be client signing gas. A
  player is *pivotal* if their refusal to sign reduces total signatures below quorum ($|cal(S)| = K - 1$).
]

#thm(4, "Dominance of Cooperation under Timeout Slashing")[
  Under the protocol's *TimeoutSlash* rule, signing the exit attestation strictly dominates defecting whenever player
  $k$'s cooperation is pivotal or quorum would otherwise be reached, and weakly dominates in the residual case where
  quorum fails regardless of $k$'s action.
]

#proof[
  Under protocol rules, a match timeout does *not* refund stake $S$ to uncooperative participants. If quorum fails:
  1. The match enters the `TimeoutClaim` state post-grace.
  2. The highest-attested valid survivor (Rank 1) claims net escrow $Omega_"net"$.
  3. Per the State-2 slash rule, *every participant except the claimant* forfeits $B_"rep"$ --- including any player
     who signed but whose signature did not enter a recorded settlement. (Equivalently: bond recovery requires being
     the recorded signer of a *settling* ladder; a signature that never settles recovers nothing.)

  We evaluate player $k !=$ claimant's terminal payoff matrix:

  - *Scenario A (quorum reached without $k$, $|cal(S) without {k}| >= K$):*
    $ U_k (text("Sign")) - U_k (text("Defect")) = (-S + B_"rep" - C_"sign") - (-S - B_"rep") = 2 B_"rep" - C_"sign" > 0. $

  - *Scenario B ($k$ strictly pivotal, $|cal(S) without {k}| = K - 1$):* If $k$ cooperates the ladder settles with $k$
    in the signer set:
    $ U_k (text("Sign")) = -S + B_"rep" - C_"sign". quad $
    If $k$ defects, quorum starves, the match settles via `TimeoutClaim`, and $k$ (a non-claimant) is slashed:
    $ U_k (text("Defect")) = -S - B_"rep". quad U_k (text("Sign")) - U_k (text("Defect")) = 2 B_"rep" - C_"sign" > 0. $

  - *Scenario C (quorum fails regardless of $k$):* `TimeoutClaim` slashes every non-claimant whether or not they
    signed, so $U_k (text("Sign")) = U_k (text("Defect")) = -S - B_"rep"$ and signing is weakly dominant
    (the $C_"sign" approx 0$ gap is one-directional only through the claimant's tip, which $k$ forgoes either way).

  Because defection never triggers a stake refund and always forfeits $B_"rep"$ whenever it matters, cooperation
  dominates defection across all game states.
]

#remark("Scope of Theorem 4")[
  Theorem 4 is a corollary of the State-2 slash rule, not the protocol's economic centerpiece: it states that a
  reporting bond of any positive size makes exit-signing incentive-compatible for losing, non-pivotal players. It does
  not model collusion among $>= K$ signers (covered by Theorem 1's fraud evidence) or verifier capture in disputes
  (covered by the bonded-verifier stake).
]

= Queue Allocation & Entropy Bounds

To prevent pre-coordinated collusion in open staked FFA queues, AMP partitions the matchmaking queue using a
commit-reveal shuffle.

#defn(5, "Commit-Reveal Shuffle")[
  Candidates submit blinded commitments $H_i = "keccak256"(p_i ‖ S ‖ "salt"_i)$. At candidate pool size $M >= N$,
  the coordinator commits to target block height $H_"target" = H_"current" + Delta$ ($Delta >= 2$). After salts are
  revealed, the assignment seed is
  $ Xi = "keccak256"(cal(B)_"target" ‖ "salt"_1 ‖ "salt"_2 ‖ dots ‖ "salt"_R) $
  over the $R$ revealed salts, where $cal(B)_"target"$ is the Avalanche blockhash at $H_"target"$.
]

#thm(5, "Conditional Allocation Uniformity")[
  Conditioned on a seed $Xi$ drawn uniformly, $Xi tilde "Uniform"({0, 1}^256)$, a deterministic Fisher-Yates shuffle
  achieves uniform distribution over candidate permutations. For any two candidates $i != j$, lobby co-location
  probability is:
  $ bb(P)("Lobby"(i) = "Lobby"(j)) = binom(M-2, N-2) / binom(M-1, N-1) = (N - 1) / (M - 1) $
]

#proof[
  Under a uniform random permutation of $M$ candidates into a lobby of size $N$, the number of subsets of size $N$
  containing both $i$ and $j$ is $binom(M-2, N-2)$. Total possible size-$N$ lobbies from pool $M$ is
  $binom(M-1, N-1)$ (fixing candidate $i$, choose the remaining $N - 1$ from $M - 1$). The ratio yields
  $(N-1)/(M-1)$.

  For a colluding cartel of size $c$, co-locating all $c$ members in a single lobby decays as:
  $ bb(P)("Cartel Co-Location") = product_(k=1)^(c-1) (N - k) / (M - k) in cal(O)((N / M)^(c-1)) $
]

#remark("Proposer Discretion & Threat Boundaries")[
  Avalanche Snow consensus provides sub-second finality, but single-block proposers retain transaction-ordering and
  block-withholding discretion. True unbiasability of $cal(B)_"target"$ is therefore an engineering approximation:
  Theorem 5 is *conditional* on the seed's uniformity, which is assumed, not proven.

  *Mitigations:* (1) Reveal deadlines slash deposits of unrevealed commitments, preventing selective aborts; (2) The
  production roadmap transitions $Xi$ to threshold BLS beacons via Avalanche Warp Messaging (AWM).
]

= Machine-Checked Invariants & Fuzz Gates

Protocol invariants are continuously asserted via stateful property testing and fuzzing:

#block(
  width: 100%,
  stroke: 0.5pt + rgb("#cbd5e1"),
  radius: 2pt,
  inset: 1pt,
  breakable: false,
  [
    #set text(size: 7.2pt)
    #table(
      columns: (1.1fr, 0.9fr, 1.1fr),
      stroke: 0.3pt + rgb("#e2e8f0"),
      fill: (col, row) => if row == 0 { rgb("#f1f5f9") } else { none },
      inset: 3.5pt,
      align: (left, left, left),
      table.header([*Protocol Invariant*], [*Formal Bound*], [*Automated Test Gate*]),
      [Quorum Non-Equiv.], [$|Q_1 inter Q_2| >= f+1$], [Conflicting-quorum terminal test],
      [State Conservation], [$sum "Out" = cal(E)_"in" (s)$ to 1 wei], [Foundry invariant fuzz (256 runs x 128k calls)],
      [Glicko $pi$-Invariance], [Bit-identical $(mu', phi', sigma')$], [`proptest` permutation suite (10k cases)],
      [Pull Accounting], [Zero external calls in resolution], [Reverting-receiver isolation test],
      [Shuffle Uniformity], [Lemire-debiased bounded draws], [Distribution spread test (7k draws/bucket)],
    )
  ],
)
#text(size: 7pt, fill: rgb("#64748b"))[
  These gates *test* the invariants; they are not proofs of Theorems 2 or 4. The conservation fuzz enforces the
  Theorem-2 identities mechanically across randomized op sequences; the quorum and payoff identities are proven above.
]

= Gas Execution Bounds

On-chain verification in `AMPMultiplayer.sol` records the settlement (constant storage) and verifies $K$ packed
signatures; per-player payouts are claimed lazily, so settlement gas scales with signatures, not recipients:

#block(
  width: 100%,
  stroke: 0.5pt + rgb("#cbd5e1"),
  radius: 2pt,
  inset: 1pt,
  breakable: false,
  [
    #set text(size: 7.2pt)
    #table(
      columns: (0.8fr, 0.8fr, 1.4fr),
      stroke: 0.3pt + rgb("#e2e8f0"),
      fill: (col, row) => if row == 0 { rgb("#f1f5f9") } else { none },
      inset: 3.5pt,
      align: (left, left, left),
      table.header([*Lobby ($N$)*], [*Quorum ($K$)*], [*Measured settlement gas*]),
      [8 players], [6 sigs], [152,962 gas (spec gate: under 250k)],
      [16 players], [11 sigs], [195,844 gas],
      [64 (BR)], [43 sigs], [404,021 gas],
    )
  ],
)

By restricting balance distribution to internal ledger credit (`claimable[player] += reward`), 64-player Battle Royale
settlements avoid iterative external call overhead, remaining well within Avalanche block gas limits (30,000,000 gas).

= Deployments and Verification Status

All contract deployments live on the Avalanche *Fuji testnet*. Nothing is deployed to mainnet.

- *v1 1v1 settlement stack (live on Fuji, Sourcify exact-match verified):*
  `AMPRegistry` at `0xf6B0eA6c88c574c4BbEAdC186AAfe72C43C2cDc2` and `AMPSettlement` at
  `0x78ec93e66255a74873d20DD62C6595A389272126`.
- *v2 N-player engine (live on Fuji, Sourcify full-match verified):*
  `AMPMultiplayer` at `0xcabf7b626172fE55d54f03c346563671AbcC77f7`, branch `n-player-multiplayer` (`5bbc816`).
  Implements dual-deposit escrow, bitmask quorum verification, and immutable payout profiles with lazy claims.
- *Match core library (`amp-match-core`):*
  Pure Rust core implementing `party.rs`, team/FFA/BR queue topologies, commitments and the blockhash shuffle, and
  canonical Glicko-2 field updates.

#v(0.2cm)
#block(
  width: 100%,
  fill: rgb("#f8fafc"),
  stroke: 0.5pt + rgb("#cbd5e1"),
  inset: 5pt,
  radius: 2pt,
  [
    #set text(size: 6.8pt, fill: rgb("#475569"))
    *Engineering Sign-Off (draft):* \
    The invariants specified here are implemented in `AMPMultiplayer.sol` and `amp-match-core` and exercised by 91
    Foundry tests (including the two stateful conservation invariants) and 106 Rust tests (including the permutation
    and party-graph fuzz gates). Fuzz results are empirical evidence, not proof; the proofs in this document stand on
    the stated assumptions. External audit and timelock governance precede any mainnet deployment.
  ],
)
