# Primer: Household / population demographics

**Audience:** agents (and humans) working on pops, growth, and demographics.  
**Status:** core model is **in-repo and wired**; tuning, caching, and some call-site docs still lag.  
**TODO entry:** `TODO.md` -> Refactors / household items as listed there  
**Background chat:** https://grok.com/share/c2hhcmQtMw_e2b20412-fa4e-4d6e-ad1e-29cf133c819e  
**Historical sketch (desktop, optional):** `/home/jeremy/Desktop/household_demographics.rs`

Vault: EconCiv `Pops.md` (Household sections). Prefer **this primer + current code** when vault notes still describe a static `HouseholdDef` recipe.

Comments and docs: **ASCII only** (`Sum`, `->`, plain `-`).

---

## One-sentence goal

Store **per-household averages** (adult / child / elder) plus a **household count**, and evolve them each year via a **`DemographicRates`** bundle (births and age-band mortality). Rates are **not** stored on each pop; they are resolved **centrally** (via factuals) and passed into growth when needed.

---

## Current model (landed)

### `Household` (`src/game/household.rs`)

Living composition for a pop's households:

| Field | Meaning |
|-------|---------|
| `count` | Number of households (fractional OK) |
| `adult`, `child`, `elder` | **Averages per household**, not pop-wide totals |
| `adult_mf`, `child_mf`, `elder_mf` | Female fractions in `0.0..=1.0` |
| `adult_labor`, `child_labor`, `elder_labor` | Labor per person-day in each band |
| `partnership_rate` | **Current** adults+elders target used for folding count (lerps toward rate target over turns) |

Helpers: `total_adults` / `total_children` / `total_elders` / `total_count` / `household_size` / `total_labor`.

Invariants (debug): member averages `>= 0`; sex fractions in unit interval; `update` expects living `count >= 1` at entry.

### `DemographicRates` (same module)

Process parameters for one growth tick (1 turn = 1 year):

| Field | Role |
|-------|------|
| `birth_per_woman` | Live-birth attempts per adult woman per year (before infant mortality) |
| `infant_mortality` | Fraction of births that die in infancy (`0..=1` when applied) |
| `maternal_mortality` | Adult-women deaths per live birth |
| `child_mortality` / `adult_mortality` / `elder_mortality` | `(total, male, female)` stacked per sex |
| `partnership_rate` | **Target** adults+elders per household (live value lives on `Household`) |

Helpers: `baseline()`, `zero()`, `add(&self, other)`.

`Household::update(&rates)` applies births, deaths, aging (childhood 20y, adulthood 40y), sex-aware flows, partnership lerp, then rewrites averages and `count`.

### Where rates live (important)

| Place | Holds rates? |
|-------|----------------|
| `DemoRow` / `Pop.demographics` | **No** -- only `household` + species/culture/class/religion ids |
| Species / culture / religion | **Yes** -- delta bundles: `species_demo_eff`, `culture_demo_eff`, `religion_demo_eff` |
| `Factuals::get_demographic_rates(demo_row)` | **Resolve** baseline + those deltas for a pop's demographics (**recompute every call**; no cache today) |
| `Pop::growth_phase(&factuals)` | Fetches rates from factuals, stacks same-day desire/stored mods, calls `household.update` |

Do **not** put a full `DemographicRates` on every pop. Resolve centrally each growth call so parallel `&Factuals` stays lock-free. If recompute-per-pop shows up in profiles at huge scale, prefer a **day-fill cache of unique demographic id keys only** (not full cartesian product); see docs on `get_demographic_rates`.

Same-day mods (basic-sat mortality pressure, desire Birthrate/Mortality, stored `PopEffect` growth arms) are built in `Pop::same_day_growth_rate_mods` and **added onto** the factuals rates for that growth call only.

### `DemoRow` (`pop_property.rs`)

```text
DemoRow {
  household: Household,
  species, culture, class, religion  // ids; 0 = none for culture/religion
}
```

Count is `household.count` (also `DemoRow::count()`). Totals go through household helpers.

---

## Day / growth flow

1. **Demographic phase (playstate):** currently resyncs desires via `update_desires`; does not snap household composition from modifiers.
2. **Growth phase:** `pop.growth_phase(factuals)`:
   - skip if `household.count < 1`
   - `rates = factuals.get_demographic_rates(demographics) + same_day_growth_rate_mods()`
   - `household.update(&rates)`
   - `previous_growth = new_count - old_count`
3. Sentiments / record keeping / decay remain separate phases.

`HouseholdDef` is **gone**. Do not reintroduce it.

---

## Historical note (desktop draft)

Early sketch at `/home/jeremy/Desktop/household_demographics.rs` and older vault "static def" language informed the design. Production code has since added sex fractions, labor fields, partnership lerp, infant/maternal split, and factuals-centric rate resolution. Prefer **`src/game/household.rs` + this primer** over the desktop file when they disagree.

---

## What is still open

| Item | Notes |
|------|--------|
| Rate cache in factuals | **Deferred.** Policy is recompute per pop/day. Day-fill living demographic keys only if profiling demands it. |
| `DemographicEffect` list mapping | Species/culture still have effect vecs; folding every arm into rates is incomplete. |
| Research / culture passive generation | Not on `DemographicRates`; was on old def -- place TBD. |
| Stale call-site comments | Some still mention `Pop::demographic_update` or count-multiplier growth (see playstate, firm, institution, pop docs). |
| Desire growth scaling | Same-day mods map old "net rate on count" style knobs into rate fields; retune if growth feels too strong/weak. |
| Institution D1 household overlays | Still deferred; same rates pipeline, no ad-hoc pop rewrites. |

---

## Vocabulary for agents

| Prefer | Meaning |
|--------|---------|
| household count | `Household.count` |
| average adult/child/elder | per-household means (`adult` / `child` / `elder`) |
| total population / total adults | `count * average` via helpers |
| demographic rates | `DemographicRates` bundle |
| effective rates for a pop | `Factuals::get_demographic_rates(demo_row)` (+ same-day mods in growth only) |
| apply rates / household update | `Household::update` |
| avoid `HouseholdDef` | removed legacy name |
| avoid storing rates on `DemoRow` | rates are central / resolved, not per-pop fields |

Also see `docs/design-vocabulary.md` for broader design terms.

---

## Policy defaults

1. **Composition:** demographic id changes update **rates** (via species/culture/religion deltas), not a one-shot rewrite of averages.
2. **Partnership:** target on `DemographicRates`; live value on `Household`, pulled toward target each `update`.
3. **Dead pops:** `count < 1` before growth skips `update`; full wipe inside `update` can set count/composition to 0 for cleanup.
4. **Vault:** older "we rejected the rates model" lines are superseded; rates model is current.
5. **Do not edit the Obsidian vault** unless explicitly asked.

---

## Agent checklist

1. Read this primer and `src/game/household.rs`.
2. For growth: use `Factuals::get_demographic_rates` + `Household::update`; do not reattach rates onto `DemoRow`.
3. For demographic deltas: edit `*_demo_eff` on species/culture/religion, then resolve through factuals.
4. Match `STYLE.md` / `AGENTS.md`; `cargo test --lib`.
5. Leave institution D1, market day, and full turn polish unless asked.
