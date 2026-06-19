# ADR-0008: The oracle is bounded, disclosed trust — k-of-n reputable signers, optimistic dispute, cross-check, equivocation-slash

- **Date**: 2026-06-18
- **Status**: accepted
- **Authority**: `docs/MISSION.md` v3 ("Two eras") via ADR-0001. Refines
  spec 03 §3.3/§5 (the oracle classes + reference marker) and PRD FR-5.
  Pairs with [ADR-0007](ADR-0007-covenant-wall-two-eras.md): transfer and
  the oracle are the two bounded-trust frontiers of the pre-covenant era.
- **Author**: Jeffery (decisions) + AI agent (red-team + research + drafting)
- **Research**: a 9-agent adversarial red-team of a staked-SchellingCoin
  oracle game; a 104-agent cited deep-research of Chainlink/Pyth/CEX oracle
  trust models + dlcspecs-compatibility. Memory: `project_oracle_trust`.

## Context

SatUSD settles every option against a single fact: the BTC/USD price the
oracle attests at maturity. The oracle is therefore the trust root for
**settlement-value correctness**. We asked whether a market-game could make
that oracle *trustless*. It cannot.

**Red-team verdict (staked-SchellingCoin oracle): NEEDS-REDESIGN — all 8
attack vectors broke.** For an *external* price, a permissionless
stake-weighted-median oracle has untunable residuals:

- **The external short is unbounded by stake.** The protocol cannot observe
  an attacker's CEX short `X`; off-protocol profit `X·Δ` scales linearly in
  the unobservable `X` while attacker cost scales only in stake `S`. For
  *any* finite parameter there exists an `X` that makes corruption
  profitable. A notional cap bounds only the in-protocol leg.
- **A patient >50% staker is never slashable** — a true majority *is* the
  consensus median, so its lie is self-consistent (the slash, defined vs the
  median, never fires). One high-notional maturity's payoff exceeds the
  franchise's NPV, so "kill-the-golden-goose" deterrence is empirically false.
- **The honest-stake "flood" defence does not exist**: vesting (needed to
  stop a flash-bought majority) gates out the very fresh capital meant to
  contest a manipulated median.
- **p+ε bribery** selects `P_false` as the focal point at ~zero realized
  cost, independent of `S`; commit-reveal hides the *emerging* median but not
  a *published* target.

**Landscape (cited deep-research), confirming the limit is universal:** no
production oracle crypto-economically slashes *price correctness* — Chainlink
Staking v0.2 slashes only node **liveness** (≈700 LINK/feed); Pyth Oracle
Integrity Staking is DAO-decided and capped at 5%. None is **dlcspecs-native**
(all ECDSA over a message body, no pre-committed nonce `R`, no maturity
attestation), so none is drop-in DLC-compatible; the "wrap their signed feed"
adapter is infeasible without a pre-committed `R`. Coinbase's signed price
oracle attested its own single-exchange price and is **deprecated** (2025).
Chainlink's and Pyth's real security is an **honest reputable committee** plus
public detectability — exactly a bounded trust.

The one borrowable idea is **UMA/Polymarket's optimistic oracle**: a bonded
assertion + a permissionless bonded **dispute window** + a rare escalation
backstop. For an *objective, publicly-observable* fact like BTC/USD the
Schelling point is strong and disputes are rare and easily resolved — it fits
a price far better than subjective prediction questions, and its dispute
window supplies the live, permissionless "anyone can catch a lie" defence the
staked game lacked.

## Decision

**The SatUSD oracle is bounded, DISCLOSED trust — and we engineer that bound
to be as tight as, and tighter than, the rest of the industry. We never
market a "trustless oracle".**

The architecture (composing with M-1's FROST + tlock):

1. **k-of-n INDEPENDENT REPUTABLE FROST signers**, dlcspecs-native: they
   pre-commit nonce points `R` ahead of each maturity event and BIP-340-attest
   the price at maturity (the only construction that yields a usable DLC
   attestation). **Not** a permissionless capital-staked pool — that is
   patient-majority-attackable and out-biddable, i.e. *weaker* for an external
   price; reputation is not for sale. FR-5's FROST is the right *mechanism*
   (aggregate k signers into one DLC key, escape the `C(n,k)` blow-up); the
   *participants* are named independent reputable entities.
2. **Multi-source data + mandatory cross-check.** Each signer prices from
   multiple CEX APIs / Chainlink / Pyth; the attested value must match the
   public reputable feeds for that timestamp (Pyth's confidence interval is a
   usable reliability/band signal). To lie, the committee must collude **and**
   diverge from publicly-observable prices — provable manipulation.
3. **Optimistic bonded dispute (borrowed from UMA), with an OPTIONAL window.**
   A bonded proposer asserts the maturity price; a permissionless bonded
   dispute window opens; undisputed → the committee signs the resolved price;
   disputed → resolve against the public feeds / the committee. The
   delta-neutral **LP is the natural honest proposer/disputer** (its CEX hedge
   settles at the true price). This is the live, permissionless challenge
   window the staked game could not provide.

   **The window is OFF the critical path for the common case.** The hard
   constraint is that the cohort signs exactly ONE price per maturity event
   (signing two = equivocation = key-leak), so it cannot emit both a "fast"
   and a "windowed" attestation. The dispute window therefore gates only the
   **unilateral** settlement path; the holder chooses per-settlement between:
   - **Cooperative (default, instant, no window):** holder + LP co-sign the
     maturity price now — both consent, so there is nothing to dispute. The LP
     wants this (it closes the position at the true price for its hedge). This
     is FR-10's cooperative exit applied at the maturity point. Most
     settlements take this path with zero added latency.
   - **Unilateral windowed (fallback / protected):** the holder settles alone
     against the cohort attestation, which runs the dispute window — for when
     the LP will not cooperate or the holder distrusts the asserted price.
   The holder is never denied the unilateral backstop; the window's hours of
   latency hit only the unilateral-distrust path — exactly when the protection
   is wanted. The fast path trades the dispute backstop for speed and needs LP
   cooperation (the LP can refuse but never steal); the choice is the holder's.
4. **Equivocation-slash** (the one crypto-economic floor: signing two prices
   for one event leaks the key, M-1) + **tlock** (no front-run, FR-5e) +
   **bounded exposure** (a per-position / system notional cap) + reputation
   and franchise as the real-world deterrents.

**The settlement-finality latency** of the dispute window applies ONLY to the
unilateral path (above): the cohort attestation is broadcastable after the
window closes. The common cooperative path is instant. The **offline floor
survives** — an undisputed attestation finalizes after the window regardless
of the holder's liveness, and the holder-only CSV still backstops silence.

## Consequences — the trust-layering (the honest narrative)

SatUSD's trust decomposes into three layers; only one is oracle-bounded:

- **Unbounded / trustless (oracle-independent):** self-custody (coins never
  leave mainnet), unilateral settlement (no counterparty can freeze/seize/
  refuse), the offline floor (collateral recoverable with no oracle). This is
  the genuine differentiator vs USDC/USDT, which always carry a freeze node.
- **The oracle ceiling (settlement-value correctness):** bounded by the
  **collective reputation of k independent reputable signers** + public
  detectability of a BTC/USD lie. This is the **same ceiling Chainlink/Pyth
  secure billions under** — high in practice, reputation-bounded not
  crypto-bounded, and rising over time (more/independent signers; a future
  internal_twap blend as SatUSD's own volume grows, spec 03 §5; the covenant
  era). We sit *above* the industry baseline by adding dlcspecs
  equivocation-slash + an optimistic dispute window + mandatory cross-check.
- **Three honest residuals (disclosed, not eliminated):** (a) a
  reputation-indifferent, very-well-resourced external-short attacker is the
  untunable tail — bounded by k-of-n independence + the notional cap +
  monitoring + disclosure, not zero; (b) assembling a credible k-of-n
  reputable signing committee is a real operational/BD lift, not a design
  detail; (c) the safe notional is capped by the trust and grows with it
  (a bootstrap size limit).

**The project's possibility = this industry-common oracle ceiling × the
orthogonal, unbounded custody/censorship edge that no fiat stablecoin has.**
The oracle is not SatUSD's weakness relative to competitors — it is the
*shared* constraint of every crypto-dollar (USDC/USDT also depend on
oracles/reserve-audits **and** a freeze node); SatUSD removes the extra node.

## Alternatives considered

- **Permissionless staked-SchellingCoin oracle (the v2 we designed).**
  Red-team-killed for an external price (above). Rejected as a trust root;
  weaker than reputable k-of-n.
- **A single reputable oracle / a single CEX signed feed.** A single point of
  trust + failure; Coinbase's is deprecated; Binance Oracle is single-entity /
  non-dlcspecs. Rejected — use k-of-n + cross-check instead.
- **Chainlink / Pyth drop-in.** Not dlcspecs-native (ECDSA, no pre-committed
  `R`, no maturity attestation); usable only as data sources / cross-checks,
  never as the DLC attestation. Rejected as the trust root.
- **internal_twap (no external price to corrupt; spec 03 §5 end state).**
  Closes the external-short residual but is unavailable for a product that
  settles against external BTC/USD by definition; it activates only as a blend
  once SatUSD's own internal volume is deep. Deferred, not abandoned.
