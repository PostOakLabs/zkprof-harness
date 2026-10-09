# AINumbers proving workload — one-page characterization

**Source of every number in this page:** the supplied AINumbers proving-records
facts block (2026-08-30), reproduced verbatim at the bottom and cited block-wide
here as "supplied." Nothing was measured here; nothing was invented. Where the
supplied block is silent, the line is marked as an open item, never a guess.

This is a **workload characterization**, not a benchmark and not a claim: it
takes the numbers a production system already records and lays out what they
mean, so a reader can reason about whether a browser prover could ever be
relevant to this workload.

## System, curve, field

- **Prover stack:** risc0 zkVM, pinned **3.0.5**. Field **BabyBear**; hash
  **Poseidon2**. Pipeline: STARK → succinct receipt → **groth16-bn254** wrap
  (~64–128 s wrap, measured). (supplied: "risc0 zkVM pinned 3.0.5 (BabyBear
  field, Poseidon2 hash) → STARK succinct receipt → groth16-bn254 wrap"). The
  outer wrap curve is bn254 / the groth16 layer.
- **So there are two "curves" in this system.** The SNARK field is BabyBear
  (small-field, is a zkVM-internal field); the wrapping proof is bn254 (the
  curve the outer Groth16 proof is over). The `curve_or_field` a report would
  carry is bn254 for the wrap, BabyBear for the zkVM.

## Constraint-scale analog: it is cycles, not gates

risc0 is a **zkVM, not a circuit**. The natural "size" of a workload is not a
constraint count but **user_cycles** — the instruction-level work the guest
executed. The supplied block measures everything in `user_cycles`, so that is
the quantity we compare. Where a circuit project says "constraints," say
"cycles" here, and do not treat the two as interchangeable: a cycle is not a
gate, and there is no universal cycles↔constraints factor. The supplied
classification rule is stated entirely in cycles.

## Latency distribution

| class | user_cycles | wall time | note |
|---|---|---|---|
| typical FAST kernels | 4.0M – 13.4M | 138–193 s | recent batch: 5.03M / 13.36M / 4.05M / 9.08M, on an RTX 3080-class GPU |
| mid outlier | 102.4M | ~15–25 min | the supplied block gives the time as a range |
| heavy outlier | 839.7M | ~1.5–2 h | in-guest secp256k1 ECDSA recovery — the known crypto-in-guest cost cliff |

(Each row: supplied, "Measured cycle range", "mid outlier", "heavy outlier".)
The wall times are the supplied "each on an RTX 3080-class GPU" / "~15–25 min" /
"~1.5–2h" figures. Every one is a **measured** figure in the supplied block
(noted "measured" for wrap; the cycle range and the three outlier rows are
supplied as observed values).

## The crypto-in-guest cliff

The 839.7M-cycle outlier is a single workload: **in-guest secp256k1 ECDSA
recovery**. It is ~7× the mid outlier and ~60–200× a typical FAST kernel
(83.97M-to-13.4M), and it costs hours wall-time even on an RTX 3080-class GPU.
This is the "known crypto-in-guest cost cliff": a cryptographic operation done
*inside* the proven guest, where every operation becomes cycles that must be
proven. It is the reason the workload is not uniform: the same guest can be
4M cycles or 840M cycles depending on whether it touches ECDSA recovery.

## Classification rule in production

supplied: `<30M cycles = cadence-provable; >30M routed to a downtime lane`.
So there are two tiers — a routinely-provable tier (≤30M) and a
not-routinely-provable tier (>30M, including the ECDSA cliff). The 102.4M mid
outlier and the 839.7M heavy outlier are both in the downtime lane.

## What a browser prover would need to hit to matter (derived, approximate)

This is **arithmetic on the supplied numbers, not a measurement**. Estimate a
cycles/s figure from the supplied batch points and extrapolate; the result is a
sanity bound, not a performance claim. The supplied block gives four cycle
counts and a single time *range* ("5.03M / 13.36M / 4.05M / 9.08M — 138–193 s
wall each"), **without pairing any specific count to any specific time**, so
which count produced which wall time is not stated. The two pairings below are
therefore an assumption, not a fact: the audit of this page flagged that "4.05M
cycles in 138 s" and "5.03M cycles in 193 s" imply a pairing the source never
states. Across all admissible pairings the rate range is 2.10e4–9.68e4 cycles/s
(3.7× wider than the slice used here), and at the high end a 30M-cycle kernel
is ~5 min, not ~19 min.

Fastest observed point (assuming the smallest count paired with the shortest
time): 4.05M cycles in 138 s → 4.05e6 / 138 ≈ **2.93e4 cycles/s**. "Slowest of
the fast four" is a cycles statement (13.36M, by cycles); for the rate bound the
low end used here picks 5.03M cycles in 193 s → 5.03e6 / 193 ≈ 2.61e4 cycles/s.
Using the low end:

- A 30M-cycle kernel (the cadence rule boundary) at 2.61e4 cycles/s → 30e6 /
  2.61e4 ≈ **1.15e3 s ≈ 19 min** (GPU-class). At the observed peak, ≈1.0e3 s
  ≈ 17 min. So the entire **cadence-provable tier** takes on the order of
  **~17–19 min per proof** on the GPU class the supplied numbers were taken
  on — **assuming the low-end pairing**; on a different admissible pairing the
  rate can be ~3.7× higher, and the bound ~5 min. This is a derived sanity
  bound on the cadence-provable tier, not a measured performance figure.

For a **browser** prover to matter for this workload it must at least reach that
same cycles/s on the public tier, long enough to be either faster than the
server path or low-cost enough to be worth the wait — and it must do so on
hardware that is *typically* far below a dedicated RTX 3080 for the
multiply-heavy stages (MSM/FFT) this workload implies. The supplied numbers give
no browser figure, so nothing is claimed; the derived bound is only that the
tier is already tens-of-minutes-per-proof on server GPUs.

The **>30M downtime lane** (and especially the 839.7M ECDSA cliff at ~1.5–2 h on
GPU) is a **factual** out-of-reach for a prover that is slower than the
existing GPU path. A browser prover would not credibly target it. That is
bounded by the supplied numbers, not aspirational.

**Derivations that need a human:**
- Open (not yet supplied): the actual cadence window (what per-proof time
  budget "cadence" means). The 17–19 min figure is derived, not a target.
- Open (not yet supplied): whether browser-side proving is a goal at all. That
  is a human decision, not a supplied fact.

## Other gaps (supplied block is silent)

- Open (not yet supplied): exact witness sizes.
- Open (not yet supplied): current end-to-end p50/p95 from production.
- Open (not yet supplied): whether browser-side proving is a goal (also above).

## Verification, not proving, is browser-feasible

supplied: "BN254 verifySeal in production; receipts served publicly; verification
is browser-feasible today, proving is not attempted client-side." So the honest
statement is the reverse of a browser-proving goal: **verifying** a receipt in a
browser is feasible today (and is what the public server does); **proving** is
not attempted client-side. The characterization above is about proving latency;
verification is a separate, browser-friendly step.

## Round-trip of the numbers

Every figure on this page traces to the supplied block (cited inline as
"supplied"), except the derived cycles/s and the 17–19 min extrapolation, which
are arithmetic on the supplied numbers and labeled derived/approximate. The
**pairing** behind the derived rate (which cycle count matched which wall time)
is an assumption — the source states a range and disjoint counts, not a
mapping — and the rate range for admissible pairings is ~3.7× wider than the
slice used. No number on this page is measured here. No aspirational claim is
made: nothing says a browser prover would work, only that the tier it would have
to reach is bounded by the supplied GPU numbers.

---

## The supplied facts block (verbatim, 2026-08-30)

- System: risc0 zkVM pinned 3.0.5 (BabyBear field, Poseidon2 hash) → STARK
  succinct receipt → groth16-bn254 wrap (~64–128s wrap, measured).
- Guest: a custom JS-execution guest (deterministic kernel VM; guest C +
  toolchain; 6 guest imageIds live across 585 receipts — mixed-image receipts
  are status quo).
- Workload: ~640 deterministic compliance kernels (JS), fixture corpus 2,246
  vectors; per-kernel proof = one vector's execution.
- Measured cycle range: typical FAST kernels 4.0M–13.4M user_cycles (recent
  batch: 5.03M/13.36M/4.05M/9.08M — 138–193s wall each on an RTX 3080-class
  GPU); mid outlier 102.4M (~15–25 min); heavy outlier 839.7M (in-guest
  secp256k1 ECDSA recovery — ~1.5–2h; the known crypto-in-guest cost cliff).
- Wrap + verify: BN254 verifySeal in production; receipts served publicly;
  verification is browser-feasible today, proving is not attempted client-side.
- Classification rule in production: <30M cycles = cadence-provable; >30M routed
  to a downtime lane.
