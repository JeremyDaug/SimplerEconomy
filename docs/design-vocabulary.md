# Design vocabulary

**Purpose:** Canonical terms for game-design talk and implementation notes.  
Prefer these names over chat shorthand. When a proposal or old comment disagrees,
**this file wins on naming**; long-form design still lives in the EconCiv vault
and `docs/proposals/`.

**How to use**

1. Prefer the **Preferred** term in new docs, comments, and discussion.
2. Do not invent near-synonyms without updating this file.
3. Put formulas and “not this” here; put essays in proposals / the vault.
4. Code identifiers may lag; when they do, note them under **Code**.

---

## Desire hierarchy

### Platonic desire
**Preferred:** platonic desire  
**Avoid:** template desire, base desire (ambiguous)

**Meaning:** Shared definition of a need type before demographics specialize it.  
**Code:** `PlatonicDesire` (`desire.rs`)

### Demographic desire
**Preferred:** demographic desire, demo desire  
**Avoid:** culture desire alone (culture is one source among several)

**Meaning:** A desire instance owned by species / culture / religion / class,
scaled for **one household**, with tier and priority.  
**Code:** `DemoDesire`

### Desire (pop desire)
**Preferred:** desire, pop desire  
**Avoid:** need (use only in prose when tier is clear: “basic needs”)

**Meaning:** Materialized, pop-scaled want on a living pop; carries current
`satisfaction` and targets.  
**Code:** `Desire` on `Pop.desires[tier]`

### Desire tier
**Preferred:** desire tier, tier  
**Labels:** **basic** (0), **common** (1), **luxury** (2)

**Avoid:** level (conflicts with satisfaction levels / firm org level)

**Meaning:** Priority band for buying and evaluation: all basic before common
before luxury.  
**Code:** index into `Pop.desires` (`0..2`)

### Desire target / bucket
**Preferred:** desire target, target; **bucket** when multiple substitutes  

**Meaning:** A good that can fulfill a desire, with efficiency, cap, consume vs use.  
**Code:** `DesireTarget`, `Desire::target`

---

## Satisfaction and fill

### Desire fill (single desire)
**Preferred:** desire fill, fill  
**Avoid:** ratio (too vague), satisfaction rate (ambiguous)

**Meaning:** How complete **one** desire is:  
`desire.satisfaction / desire.amount`  
Same as number of full **satisfaction levels** met for that desire when not
oversaturated.  
**Code:** `Desire::tiers_satisfied()` (name is historical; means fill levels)

### Tier fill
**Preferred:** tier fill, common fill, basic fill, luxury fill  
**Avoid:** ratio, tier ratio, average satisfaction (say **tier fill** or **units**)

**Meaning:** Average desire fill across a tier, optionally after boosts:

```text
tier_fill = ( Σ (satisfaction / amount) + boost ) / desire_count
```

- Empty non-basic tier: treat as fully filled (`1.0`) when evaluating “no unmet needs.”
- **Basic:** no satisfaction boosts.
- **Common / luxury:** may exceed `1.0` after boosts (no hard cap on recorded fill).

**Code:** `Pop::recorded_tier_sat[tier]` after `process_satisfaction` (boosted for
common/luxury); raw averages via `tier_avg_satisfaction` / similar helpers.

### Satisfaction units
**Preferred:** satisfaction units, fulfilled units  
**Avoid:** just “satisfaction” when units vs fill is unclear

**Meaning:** Absolute amount of fulfillment toward a desire (`Desire.satisfaction`),
in the same units as `amount` (goods-equivalent, not a 0–1 fraction).  
**Not** the same as tier fill.

### Satisfaction boost (fill boost)
**Preferred:** satisfaction boost, fill boost  
**Avoid:** ratio-mass (internal jargon unless defining), extra satisfaction (vague)

**Meaning:** Extra **fill** added into the tier-fill formula (like an extra
fulfilled desire, or a fraction of one)—**not** rewriting every desire’s
`satisfaction` field.

| Source | Preferred phrasing | Notes |
|--------|-------------------|--------|
| On a desire | **desire fill boost** | Scales with that desire’s fill (`signed_strength`) |
| Same-day store | **stored fill boost** | Already scaled (e.g. process output ÷ pop); applied by tier |

**Code:** `DesireEffect::Satisfaction`, `PopEffect::Satisfaction { tier, amount }`;
combined in `process_satisfaction` via `tier_satisfaction_with_boost`.

### Common fill surplus
**Preferred:** common fill surplus, common oversat (ok in code talk)  
**Avoid:** spiritual ratio (unless defining culture/religion content)

**Meaning:** Common **tier fill** above `1.0`—extra completeness beyond a full
ordinary common basket (meaning, ritual, community weight, etc.).

**Mood (draft):** full weight on fill in `[0, 1]`; **half weight** on overflow
above `1.0` (`common_mood_weight`).

**Not:** luxury oversat (open-ended wealth/status ladder).

### Luxury oversat
**Preferred:** luxury oversat, luxury fill above 1  

**Meaning:** Luxury tier fill beyond one full pass; intentional infinite ladder.  
**Code:** luxury consume loops; unclamped luxury tier fill in recording.

---

## Sentiment (population feeling)

### Sentiment
**Preferred:** sentiment  
**Avoid:** mood when meaning the **data structure** (mood is fine in loose prose)

**Meaning:** Partition of a pop’s political/social feeling; shares sum to ~1.  
**Code:** `Sentiment`, `Pop.sentiment`

### Sentiment axes
**Preferred:** use axis names: **happiness**, **contentment**, **anger**, **fear**, **hope**

**Meaning:** Shares of the pop in each state; moving one axis takes mass from others
unless a transfer names both ends.

| Axis | Role (short) |
|------|----------------|
| Happiness | Active positive affect |
| Contentment | Calm status-quo comfort |
| Anger | Hostility / unrest fuel |
| Fear | Anxiety / flight pressure |
| Hope | Aspiration / reform energy |

**Code:** `SentimentKind`, accessors on `Sentiment`

### Sentiment mod
**Preferred:** sentiment mod, flat share mod, relative part mod  

**Meaning:**  
- **Flat share:** absolute fraction of the whole pop into/out of an axis (donors unspecified).  
- **Relative part:** scale one axis by a fraction of itself, then renormalize.

**Code:** `SentimentMod`, `apply_mod` / `apply_mods`, `add_share`

---

## Property and day flow

### Reserve / reserved
**Preferred:** reserve, reserved, earmark  

**Meaning:** Goods set aside for today’s desires; does not remove from `quantity`.  
Savings does not fence reserves or consumption.  
**Code:** `PopPRow.reserved`, `initial_reservations_and_update_satisfaction`

### Consume / consumption
**Preferred:** consume, consumption  

**Meaning:** Apply reserved/on-hand goods to raise desire **satisfaction units**
(and move stock to consumed/used).  
**Code:** `Pop::consume`, `satisfy_one_desire`

### Process satisfaction
**Preferred:** process satisfaction  

**Meaning:** Late-day pass: apply fill boosts → record tier fill → update sentiment
from needs and effects. Not growth, not bonus goods, not goods decay.  
**Code:** `Pop::process_satisfaction`

### Stored effects
**Preferred:** stored effects, same-day effects  

**Meaning:** Ephemeral pop effects applied later the same day.  
**Code:** `Pop.stored_effects`, `PopEffect`

| Kind | Phase that should clear it |
|------|----------------------------|
| Birthrate / Mortality | `growth_phase` |
| Satisfaction (fill boost), Sentiment*, soft Satisfaction mood arms | `process_satisfaction` |
| BonusGood | `decay_goods` |

---

## World data

### Factuals
**Preferred:** factuals  

**Meaning:** Mostly static world definitions (goods, processes, species, cultures,
religions).  
**Not:** current prices, stocks, actors.  
**Code:** `Factuals`

### Game state
**Preferred:** game state, live state  

**Meaning:** Map, markets, actors, prices, property, current pops.  
**Opposite of:** factuals.

### Pop
**Preferred:** pop  

**Meaning:** Cohort of households sharing job/demographics in a market; unit of
desire, labor, and sentiment.  
**Code:** `Pop`, `DemoRow`

### Household
**Preferred:** household, household definition  

**Meaning:** Composition and baseline rates (adults/children/elders, birth/mortality,
passive culture/research).  
**Code:** `HouseholdDef`

---

## Institutions and actors

### Institution
**Preferred:** institution  

**Meaning:** Semi-autonomous org (state branch, religion, guild, …); not the player
shell and not a demographic.  
**Code:** `Institution` — see `docs/proposals/institution-draft.md`

### Actor
**Preferred:** actor  

**Meaning:** Something that can act economically/politically (pop, firm, institution,
state).  
**Code:** `Actor`, `Actors`

---

## Quick ban list (chat drift)

| Avoid in design talk | Prefer instead |
|----------------------|----------------|
| ratio (alone) | **desire fill**, **tier fill** |
| satisfaction rate | **desire fill** or **satisfaction units** (pick one) |
| mood (for the struct) | **sentiment** |
| level (for desire tier) | **tier** (basic/common/luxury) |
| need (when tier/fill matters) | **desire**, **basic/common/luxury desires** |
| spiritual ratio | **common fill surplus** (or name the content source) |

---

## Related docs

| Doc | Role |
|-----|------|
| EconCiv vault (`Pops.md`, `Desires.md`, …) | Long-form design intent |
| `docs/proposals/satisfaction-ratio-and-boosts.md` | Fill boosts, units vs fill, surplus mood |
| `docs/proposals/institution-draft.md` | Institutions v0 |
| `STYLE.md` | Code style |
| `AGENTS.md` | Agent rules + vault paths |

When renaming a concept here, update the proposal’s wording in a follow-up if it
still says “ratio” in the title body—glossary preferred terms still apply.
