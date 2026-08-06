# Primer: Household / population demographics refactor (next work)

**Audience:** agents (and humans) continuing the author's next task.  
**Status:** planned; first-draft sketch exists outside the repo.  
**TODO entry:** `TODO.md` -> Refactors -> Household / population change helpers  
**Background chat:** https://grok.com/share/c2hhcmQtMw_e2b20412-fa4e-4d6e-ad1e-29cf133c819e  
**First draft (author desktop, not in repo):** `/home/jeremy/Desktop/household_demographics.rs`  

Vault: EconCiv `Pops.md` (Household + Alternative Household sections). The **rates-driven composition** path is what we are implementing next, even if older vault notes also describe a simpler static def model.

Comments and docs: **ASCII only** (`Sum`, `->`, plain `-`).

---

## One-sentence goal

Stop treating household shape as a static member recipe you edit directly. Instead, store **per-household average** adults/children/elders plus a **household count**, and drive change each turn through a **`DemographicRates`** bundle (births and age-specific mortality). Rates flow people between buckets; averages and count update as a result.

---

## Target model (author plan)

### Types to end up with

1. **`DemographicRates`** (new)  
   Demographic process parameters (typically per year / per turn). Not the household body itself.

   Planned fields (names may match or track the draft):

   | Rate | Role |
   |------|------|
   | Birth rate (`births_per_woman` in draft) | Live births drive (per adult woman per turn) |
   | Miscarriage rate | Fraction of pregnancies that fail (draft has field; wiring into flows may still need care) |
   | Maternal mortality | Adult-woman deaths tied to live births |
   | Child mortality | Annual (per-turn) death rate for children |
   | Adult mortality | Annual death rate for adults (non-maternal) |
   | Elder mortality | Annual death rate for elders |

2. **`Household`** (consolidated; **`HouseholdDef` goes away** as a separate concept)  
   One pop's household block:

   | Field | Meaning |
   |-------|---------|
   | `count` | Number of households this pop represents |
   | `adults`, `children`, `elders` | **Averages per household**, not pop-wide totals |

   Derived:

   - `household_size` = adults + children + elders (average people per household)
   - `total_population` = count * household_size
   - `total_adults` = count * adults (same pattern for children/elders)

### What we stop doing

- Defining growth mainly as "set adults/children/elders on a def and multiply by count."
- Manually nudging member slots as the primary way demographics change.
- Keeping parallel `HouseholdDef` (template) + `Household` / `DemoRow.household` that drift apart.

### What we start doing

- Species / culture / religion (and later institutions) contribute **rates** (and maybe labor/culture/research side stats if still needed), not raw "add 0.5 children to the def."
- Each growth/demography tick: apply rates to the **current** average composition + count:
  - births add children (after miscarriage / live-birth accounting as designed)
  - mortality removes from the right age band
  - aging moves children -> adults -> elders over stage lengths
  - recompute averages and/or fold net headcount into `count` so the pop stays consistent
- Shock events (famine, plague, war) can spike rates or directly wound totals; the structure then **evolves** under rates instead of snapping to a new fixed recipe.

### Design properties the author wants

- **Smooth transitions:** change birth/death rates and composition drifts over turns instead of jumping total pop when a culture edit rewrites a static size.
- **Radical shocks still possible:** temporary rate spikes or direct losses reshape the average household over following turns.
- **Averages + count:** mental model is "typical household shape" times "how many households," not one giant pile of people with no structure.
- **Growth rate derivable** from current composition + rates (draft: `growth_rate`), not only from comparing state before/after.

---

## First draft sketch (desktop file)

Path: `/home/jeremy/Desktop/household_demographics.rs`

Treat as **author intent / prototype**, not production code. Integrate into `src/game/` with project style (`STYLE.md`, tests, factuals wiring). Do not assume it is compile-ready against the game crate.

Summary of the sketch:

```text
DemographicRates {
  births_per_woman, miscarriage_rate, maternal_mortality,
  child_mortality, adult_mortality, elder_mortality
}

Household {
  count,           // number of households
  adults, children, elders  // per-household averages
}

Household::update(&mut self, rates)  // one turn, dt = 1 year in the draft
  - work in totals (count * averages)
  - live_births from women ~= total_adults * 0.5 * births_per_woman
  - deaths by band + maternal deaths from births
  - aging: children/20, adults/40 per year
  - apply flows, floor at 0
  - fold new total pop into count using previous average size
  - rewrite averages from new totals / count
```

Notes when porting:

- Draft assumes **1 turn = 1 year**. Confirm against game turn length if that ever differs.
- `miscarriage_rate` is on the struct in the draft; live_births currently uses `births_per_woman` only. Decide whether births are pre- or post-miscarriage when wiring for real.
- Adult sex split is a flat `0.5` in the draft.
- Stage lengths: childhood 20 years, adulthood ~40 years (elderhood open-ended via elder mortality).
- Folding rule: **preserve previous average household size** into `count = new_total_pop / old_size`, then recompute averages. That is intentional draft policy; change only with author approval.
- Labor efficiency, passive research/culture, and desire scaling currently live on old `HouseholdDef` / pop paths. They need a new home (on `Household`, separate modifiers, or rates-adjacent stats) when consolidating.

---

## Current code (what you are replacing)

| Today | Role |
|-------|------|
| `HouseholdDef` | Static adults/children/elders + single birth_rate/mortality_rate + labor eff + passive rates |
| `Household { def, count }` | Wrapper helpers; underused by pop path |
| `DemoRow { count, household: HouseholdDef, ... }` | Pop demographics; count = households |
| `Pop::growth_phase` | Multiplies `count` by net rate from household birth/mortality + desire/stored effects |
| `rebuild_household_from_demographics` | Sums demographic `*_household_modifiers: HouseholdDef` into a new def **without** evolving composition via rates |

Pain today: editing static size without careful count math **jumps total population**. Species/culture/religion TODOs already ask for smoother household change.

After refactor, demographic modifiers should push **`DemographicRates`** (and any remaining non-rate household stats), and the daily/seasonal update should run something like `Household::update`.

---

## Integration points (when implementing in-repo)

Likely touch list:

| Area | Work |
|------|------|
| `src/game/household.rs` | Replace/consolidate types; port draft + tests |
| `src/game/pop_property.rs` | `DemoRow` holds consolidated `Household` (or equivalent) |
| `src/game/pop.rs` | `growth_phase` / demographic update use rates + `update`; desire scaling still sees totals via helpers |
| `species` / `culture` / `religion` | Modifiers become rates (or rates + small side stats); `household_changed` meaning may become "rates dirty" |
| `effects.rs` | Birthrate/Mortality effects may map into rate deltas rather than flat count multipliers |
| `factuals` / institution D1 | Later: same rates pipeline; no ad-hoc pop rewrites |
| `docs/design-vocabulary.md` | Update household / growth terms when names stick |

Suggested test focus:

- Baseline rates roughly stable near default averages (~2 / 2.5 / 0.5) if that is still the tune target
- Rate shock moves composition over several turns without NaNs or negative buckets
- Total pop 0 / count 0 death path
- Totals identity: `count * adults == total_adults` after `update`
- Desire/growth call sites still get coherent `total_population` / labor

---

## In scope vs out of scope

**In scope for this refactor**

- Consolidate `Household` + `HouseholdDef` into one household representation (averages + count).
- Introduce `DemographicRates` and a per-turn (or growth-phase) apply step.
- Rewire pop growth / demographic rebuild to the new model.
- Helpers for totals, size, growth_rate, safe floors.
- Tests for the math and pop integration.

**Out of scope unless author expands**

- Full market day / migration / UI.
- Microsimulation of named individuals.
- Re-opening "only static birth_rate + mortality_rate on a fixed recipe" as the long-term design.
- Perfect historical demography calibration (tune later; draft rates are rough).

---

## Vocabulary for agents

| Prefer | Meaning |
|--------|---------|
| household count | `Household.count` -- number of households |
| average adults/children/elders | per-household means, not pop totals |
| total population / total adults | count * average |
| demographic rates | the `DemographicRates` bundle |
| apply rates / household update | one step of flows (births, deaths, aging) |
| avoid "HouseholdDef" | legacy name; do not reintroduce without reason |

Related design vocab file still documents the **old** household def language until updated.

---

## Policy defaults (ask if unclear)

1. **Folding:** draft keeps previous average household size and absorbs growth into `count`. Keep that unless told otherwise.
2. **Desire/stored growth effects:** today they adjust a net multiplier on count. Map them onto rate deltas or a post-pass count tweak deliberately; do not double-apply.
3. **Vault contradiction:** older "Alternative Household Decision" text preferred the simple model; **author's current plan is the rates model** + desktop draft. Prefer this primer + draft over that outdated decision line.
4. **Do not edit the Obsidian vault** unless explicitly asked.

---

## Agent checklist

1. Read this primer and `/home/jeremy/Desktop/household_demographics.rs`.
2. Skim current `household.rs`, `DemoRow`, `Pop::growth_phase`, `rebuild_household_from_demographics`.
3. Propose a thin PR-sized port: types + `update` + tests, then pop wiring -- or follow author if they already started.
4. Match `STYLE.md` / `AGENTS.md`; `cargo test --lib`.
5. Update design vocabulary when public names stabilize.
6. Leave institution D1 and full turn polish for later unless asked.
