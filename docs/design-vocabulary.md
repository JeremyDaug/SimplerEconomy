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
**Efficiency** must be **positive** (`> 0`). Zero is a worthless target; negatives
are not accepted (no malus-via-efficiency).  
**Code:** `DesireTarget`, `Desire::target`

---

## Pop Consumption Patterns

A collection of names for consumption patterns to be aware of.

### Ascetic
**Preferred:** Ascetic
**Avoid:** Poverty, Poor, Low-Impact

**Meaning:** A pop with few desires and low total-unit consumption. Should result in relatively calm, easy to satisfy population, with little consumptive demand.

### Affluent
**Preferred:** Affluent, Rich
**Avoid:** Wealthy

**Meaning:** A pop with a large number of desires and sizeable total-unit consumption. Should be more restless and difficult to satisfy at the benefit of more culture production and demand placed on the market.

---

## Satisfaction and fill

### Desire Satisfaction (single desire)
**Preferred:** desire Satisfaction, Desire Sat.  
**Avoid:** ratio (too vague), satisfaction rate (ambiguous)

**Meaning:** How complete **one** desire is:  
`desire.satisfaction / desire.amount`  
Same as number of full **satisfaction levels** met for that desire when not
oversaturated.  
**Code:** `Desire::tiers_satisfied()` (name is historical; means fill levels)

### Tier Satisfaction
**Preferred:** tier sat, common sat, basic sat, luxury sat  
**Avoid:** ratio, tier ratio, tier fill, average satisfaction (say **tier sat**)

**Meaning:** **Sum** of desire success rates across a tier, optionally after boosts
(not an average). One full desire contributes `1.0`; three fully met desires →
`3.0`. Boosts add success-rate mass in the same units.

```text
tier_sat = Sum(satisfaction / amount) + boost
```

- Empty tier: treat as fully satisfied (`1.0`) when recording / evaluating “no unmet needs.”
- **Basic:** no satisfaction boosts.
- **Common / luxury:** no hard cap on recorded tier sat (can exceed desire count after boosts).
- **Mood / sentiment:** may normalize by desire count (`sum / count`) when a 0–1-ish
  completeness is needed; that average is **not** what `records.tier_sat` stores.
- **Growth / other:** `tier_avg_satisfaction` is a separate average helper; do not
  confuse it with recorded tier sat.

**Code:** `Pop::records.tier_sat[tier]` after `update_sentiments` (boosted for
common/luxury); helpers `tier_satisfaction`, `tier_sat_with_boost`.
Wealth: `Pop::records.wealth_amv` (AMV of on-hand property).
Liquid wealth: `Pop::records.liquid_wealth` (AMV times salability, tradeable only).
Satisfaction units total: `Pop::records.satisfaction_units_total`.

### Standard of Living
**Preferred:** Standard of Living, SOL, Wealth

**Meaning:** The abstract quality of life for a pop. High standards are preferreble to lower standards, and raising or falling standards results in larger sentimental shifts than staying still. Roughly measured by a total Tiers of Satisfaciton satisfied (Basic + Common + Luxury) and overall wealth (Wealth owned + Goods Consumed/Used in Satisfaction).

### Satisfaction units
**Preferred:** satisfaction units, sat units, fulfilled units  
**Avoid:** just “satisfaction” when units vs fill is unclear

**Meaning:** Absolute amount of fulfillment toward a desire (`Desire.satisfaction`),
in the same units as `amount` (goods times efficiency, not a 0–1 fraction).  
**Not** the same as tier satisfaction.

### Satisfaction boost
**Preferred:** satisfaction boost, Sat Boost, Bonus Satisfaction
**Avoid:** ratio-mass (internal jargon unless defining), extra satisfaction (vague)

**Meaning:** Extra **Satisfaction** added into the tier-satisfaction formula (like an extra
fulfilled desire, or a fraction of one)—**not** added to every desire’s
`satisfaction` field.

| Source | Preferred phrasing | Notes |
|--------|-------------------|--------|
| On a desire | **desire satisfaction boost** | Scales with that desire’s satisfaction (`signed_strength`) |
| Same-day store | **stored satisfaction boost** | Already scaled (e.g. process output ÷ pop); applied by tier |

**Code:** `DesireEffect::Satisfaction`, `PopEffect::Satisfaction { tier, amount }`;
combined in `update_sentiments` via `tier_sat_with_boost`.

### Common Satisfaction surplus
**Preferred:** common Satisfaction surplus, common oversaturation
**Avoid:** spiritual ratio (unless defining culture/religion content)

**Meaning:** Common **tier sat** above `1.0`—extra completeness beyond a full
ordinary common basket (meaning, ritual, community weight, etc.).

**Sentiment (draft):** full weight on sat in `[0, 1]`; **half weight** on overflow
above `1.0` (`common_sat_mood_weight`).

**Not:** luxury oversat (open-ended wealth/status ladder).

### Luxury oversat
**Preferred:** luxury oversaturation, luxury oversat

**Meaning:** Luxury tier sat beyond one full pass; intentional infinite ladder.  
**Code:** luxury consume loops; unclamped luxury tier sat in recording.

---

## Sentiment (population feeling)

### Sentiment
**Preferred:** sentiment
**Avoid:** mood when meaning the **data structure** (mood is fine in loose prose)

**Meaning:** Partition of a pop’s political/social feeling; shares sum to ~1.  
**Code:** `Sentiment`, `Pop.sentiment`

### Sentiment axes
**Preferred:** use axis names: **happiness**, **contentment**, **anger**, **fear**, **hope**
For a non-specific axis Mood is preferred and distinguishes from the combined sentiment.

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

### Sentiment Shift
**Preferred:** sentiment Shift, flat share Shift, relative Shift  

**Meaning:**  
- **Flat share:** absolute fraction of the whole pop into/out of an axis (donors unspecified).  
- **Relative part:** scale one axis by a fraction of itself, then renormalize.

**Code:** `SentimentMod`, `apply_mod` / `apply_mods`, `add_share`

---

## Property and day flow

### Reserve / reserved
**Preferred:** reserve, reserved

**Meaning:** Goods set aside for today’s desires; does not remove from `quantity`.  
Savings does not fence reserves or consumption.  
**Code:** `PopPRow.reserved`, `initial_reservations_and_update_satisfaction`

### Consume / consumption
**Preferred:** consume, consumption  

**Meaning:** Goods that have been used to satisfy desires and have been moved into the consumed category.
**Code:** `Pop::consume`, `satisfy_one_desire`

### Liquid wealth
**Preferred:** liquid wealth, mobile wealth

**Meaning:** Spendable stock: `Sum(quantity * price * salability)` for tradeable goods.
Missing prices and salability default to `1.0`. Untradeable goods are skipped.
**Code:** `PopRecords::liquid_wealth`, `Pop::property_liquid_wealth`

### Save target
**Preferred:** save target

**Meaning:** Soft between-days target on a property row. Not a hard fence and not
on-hand savings. After shopping, `shop_target` should be `reserved + save_target`.
After record keeping, `shop_target` is **consume need + save target**
for tradeable goods.
**Avoid:** wish (too emotive for a planning target)
**Code:** `PopPRow::save_target`

### Saved (derived units)
**Preferred:** saved, saved units

**Meaning:** Actual units held for between days: `quantity - reserved`.
Consume draws quantity and reserved together, so the same formula holds after consume.
**Not** `save_target`.
**Code:** `PopPRow::saved`, `Pop::property_saved_wealth_amv`

### Consume need
**Preferred:** consume need  
**Avoid:** consume-half (internal draft jargon)

**Meaning:** Units of a good planned for tomorrow's consume/use, **before**
savings. Hybrid of remaining unsatisfied desire units and what was actually
consumed/used today:

```text
consume_need = max(unsatisfied target units, consumed + used)
```

Shop target for a tradeable good is `consume_need + save_target`.

**Code:** local map in `Pop::rewrite_shop_and_save_targets`

### Savings ratio
**Preferred:** savings ratio, days of buffer

**Meaning:** Target **days of the cheapest basic+common basket** to hold
overnight (`1.0` = one extra day's living cost). Not a share of leftover
liquid wealth. Fear scales **substitutability**: a calm pop may hold that
AMV in highly salable goods; a fearful pop wants more of it as the specific
basket goods.
**Code:** `PopRecords::savings_ratio`

### Time preference
**Preferred:** time preference

**Meaning:** The pop's personal interest rate. Higher means consume now and demand
a larger return to save or invest.
**Code:** `PopRecords::time_preference`

### Risk appetite
**Preferred:** risk appetite, fear/greed (planning)

**Meaning:** Slow planning variable in `-1.0..=1.0` (fear to greed). Not the Fear
sentiment axis. Nudged from weighted sentiment (hope > happiness, fear > anger)
and SOL trend.
**Code:** `PopRecords::risk_appetite`

### Used
**Preferred:** Used

**Meaning:** Goods that have been used 

### Update sentiments
**Preferred:** update sentiments  

**Meaning:** Late-day pass that records tier sat (with boosts), wealth, and applies
sentiment shifts from tier sat and stored mood effects. After consume and growth;
does not apply growth arms or bonus goods.
**Code:** `Pop::update_sentiments`

### Stored effects
**Preferred:** stored effects

**Meaning:** Effects a pop has gained throughout the day and which are applied by the end of the day.
**Code:** `Pop.stored_effects`, `PopEffect`

| Kind | Phase that should clear it |
|------|----------------------------|
| Birthrate / Mortality | `growth_phase` |
| Satisfaction (sat boost), Sentiment* | `update_sentiments` |
| BonusGood | `decay_goods` |

---

## World data

### Factuals
**Preferred:** factuals  

**Meaning:** Mostly static world definitions (goods, processes, species, cultures,
religions). The "Facts" of the world the game is taking place in.
**Not:** current prices, stocks, actors.  
**Code:** `Factuals`

### Game state
**Preferred:** game state, play state
**Meaning:** The Map, markets, actors, prices, property, current pops of the game. Expected to change constantly.

### Actor
**Preferred:** actor  

**Meaning:** Something that can act economically/politically (pop, firm, institution,
state).  
**Code:** `Actor`, `Actors`

### Pop
**Preferred:** pop, Population

**Meaning:** Cohort of households sharing job/demographics in a market; unit of
desire, labor, and sentiment.  
**Code:** `Pop`, `DemoRow`

### Household
**Preferred:** household, household definition  

**Meaning:** Composition and baseline rates (adults/children/elders, birth/mortality,
passive culture/research).  
**Code:** `Household`, `DemographicRates` (`household.rs`)

### Institution
**Preferred:** institution  

**Meaning:** Semi-autonomous org (state branch, religion, guild, …); modified by owning player and events.
**Code:** `Institution` — see `docs/proposals/institution-draft.md`

### Firm
**Preferred:** Firm, Business, Company, Corporation, Corp

**Meaning:** A semi-autonomous org which handles and focuses on productive economic activity.
**Code:**  `Firm`

### State
**Preferred:** State, Player

**Meaning:** A state is the required aspects of a player, existing even when operated by a human player. State and Player should be treated as mostly synonymous. A State is the mechanisms of control and management, while the Player is the controller and decision making of the state.

---

## Quick ban list (chat drift)

| Avoid in design talk | Prefer instead |
|----------------------|----------------|
| ratio (alone) | **desire sat**, **tier sat** |
| tier fill / fill (for completeness) | **tier sat** / **desire sat** |
| satisfaction rate | **desire sat** or **satisfaction units** (pick one) |
| mood (for the struct) | **sentiment** |
| level (for desire tier) | **tier** (basic/common/luxury) |

---

## Related docs

| Doc | Role |
|-----|------|
| EconCiv vault (`Pops.md`, `Desires.md`, …) | Long-form design intent |
| `docs/proposals/institution-draft.md` | Institutions v0 |
| `STYLE.md` | Code style |
| `AGENTS.md` | Agent rules + vault paths |
| `TODO.md` | Working focus list |
| `reviewlog.md` | Open review debt only |
