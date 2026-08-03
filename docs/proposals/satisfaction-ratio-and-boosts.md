# Satisfaction fill, boosts, and wealth (design notes)

**Status:** partially implemented in `Pop::process_satisfaction` / effects; long-term
unit-vs-fill wealth uses still open  
**Date:** 2026-07-31  
**Sources:** design talk on satisfaction boosts, common oversat, fill vs units  
**Terms:** prefer [`docs/design-vocabulary.md`](../design-vocabulary.md) (**tier fill**,
**desire fill**, **satisfaction units**, **fill boost**, **common fill surplus**).
Older “ratio” wording in this file means **tier fill**.

This note records the agreed interpretation so it is not lost. Prefer it when
touching satisfaction boosts, `recorded_tier_sat`, or later wealth metrics.

---

## Core distinction: ratio vs units

Two different measures of “how well a pop is doing”:

| Metric | Meaning | Good for |
|--------|---------|----------|
| **Ratio** | Average fill of desires: `satisfaction / amount` (and boosted tier averages) | Mood, contentment, “is life complete?” |
| **Units** | Absolute goods consumed / satisfaction units / AMV spent | Markets, industry load, tax base, material “weight” |

These can diverge on purpose:

- **Ascetic** culture: few desires, low unit consumption, high ratio → politically calm, light demand.
- **Affluent** culture: many desires, high unit throughput, mediocre ratio → drives the economy while still restless.

Boosts and spiritual surplus should primarily move **ratio**. The goods ledger
(units) stays honest about what was actually bought and consumed.

---

## Satisfaction boost formula

Boosts do **not** rewrite each desire’s `satisfaction`. They change the **tier
result** used for recording and mood:

```text
tier_ratio = ( Σ (desire.satisfaction / desire.amount) + boost ) / desire_count
```

- Empty non-basic tier: treat as fully satisfied (`1.0`); ignore boost.
- **Basic (tier 0):** never receives satisfaction boosts.
- **Common / luxury:** no hard cap at `1.0` on the recorded ratio.

### What `boost` means

`boost` is **ratio-mass** in the same space as a sum of fill ratios—like an
**extra fulfilled desire** (or a fractional one), not “add N units to every line.”

| Source | Scaling | Story |
|--------|---------|--------|
| **Desire** `DesireEffect::Satisfaction` | Sat-scaled via `signed_strength` (and desire amounts already pop-scaled) | Culture/religion: “this need being met grants extra life-completeness” |
| **Stored** `PopEffect::Satisfaction { tier, amount }` | Produced as process/event output, then **shared by pop** when applied (e.g. `cycles * scalar / households`) | Firm ritual/comfort output: absolute comfort pool, not double-scaled by pop size |

Desire boosts scale “innately” with the pop’s satisfaction path. Stored boosts
should be converted to per-pop ratio-mass **before or at** apply time, not treated
as another household-scaled desire amount.

---

## Common ratio above 1.0 (spiritual / meaning surplus)

Allowing **common** `tier_ratio > 1.0` is intentional.

| Range | Reading |
|-------|---------|
| **&lt; 1.0** | Material/social common shortfall |
| **≈ 1.0** | Complete ordinary life (content baseline) |
| **&gt; 1.0** | Extra fulfillment on top (spiritual, ritual, community, meaning) |

Use cases:

1. **Gap-fill:** material common under-consumed, but spiritual weight plugs the hole
   so the pop can still feel whole for mood purposes (basic needs stay separate and hard).
2. **Surplus:** material common fully met **and** extra meaning → “complete but
   non-luxurious” life, not the open luxury ladder.

### Mood weighting for common (implemented draft)

- **`common ≤ 1.0`:** full weight (existing happiness/contentment coeffs).
- **`common > 1.0`:** full weight on the first 1.0, **half weight** on the overflow:

```text
common_mood = common                          if common ≤ 1
            = 1.0 + 0.5 * (common - 1.0)      if common > 1
```

Luxury oversat remains the open-ended **status / excess** channel; common oversat
is **wholeness / meaning**. Diminishing returns above 1.0 can be retuned later.

**Do not** let common spiritual boosts substitute for **basic** needs.

---

## Luxury

Same boost formula as common (ratio-mass + average). No upper clamp. Oversat
raises average luxury satisfaction and can feed hope / elite mood paths separately
from common surplus.

---

## What is recorded vs what desires store

- **`Desire.satisfaction`:** left as consume/ownership left it (units toward that desire).
- **`recorded_tier_sat`:** post-boost **ratio averages** for basic (unboosted), common,
  and luxury (boosted where allowed)—used for mood and inspectability.

Later, consider also recording **unit sums** (and/or AMV consumed) per tier so
wealth/throughput UI does not have to reverse-engineer from ratios.

Suggested future pair (not required yet):

| Field | Content |
|-------|---------|
| `recorded_tier_sat` / ratio | Completeness (with boosts) |
| `recorded_tier_units` (or similar) | `Σ satisfaction` and/or goods used that day |

---

## Day pipeline placement

In the current draft, boosts + mood run in **`process_satisfaction`** (after consume and growth). Growth still uses raw desire satisfaction for its own
terms unless later re-ordered.

Stored effect ownership:

| Arm | Phase |
|-----|--------|
| Birthrate / Mortality | `growth_phase` (apply + remove) |
| Satisfaction (tier boost) + Sentiment | `process_satisfaction` |
| BonusGood | `decay_goods` |

---

## Implemented vs deferred

### Implemented (approx.)

- Ratio formula `(Σ sat/amount + boost) / n` in `tier_satisfaction_with_boost`
- No common hard cap on recorded ratio
- `common_mood_weight` with half effect above 1.0
- Desire + stored satisfaction boost collection in `process_satisfaction`
- Basic forbidden for satisfaction boosts

### Deferred / open

- Explicit **unit** ledger fields for wealth/throughput
- UI or player metrics: ascetic completeness vs affluent volume
- Retune half-overflow coeff; optional soft diminishing returns curve
- Firm/process pipeline that emits stored satisfaction as `output / pop`
- Whether growth-phase common terms should ever see boosted ratio
- Culture tags that weight spiritual common desires more heavily

---

## Design stance (short)

1. **Ratio** = politics and feeling of completeness.  
2. **Units** = material economy and lifestyle scale.  
3. **Boost** = extra ratio-mass (virtual fulfilled desire), not a rewrite of every line.  
4. **Common &gt; 1** = meaning surplus; mood gets a reduced-rate overflow path.  
5. **Luxury oversat** = separate, open-ended excess ladder.
