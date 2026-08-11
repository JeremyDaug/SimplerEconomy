# Agent handoff — EconCiv rework

**Branch:** `EconCiv-Rework-Branch`  
**Handoff date:** 2026-08-10  
**Purpose:** Catch a new agent/session up on recent work and direction. Prefer this plus `AGENTS.md`, `STYLE.md`, `TODO.md`, `reviewlog.md`, and `docs/design-vocabulary.md` over inventing process from scratch.

**Build (as of handoff):** `cargo test --lib` was green after household integration and rate-resolution tests. Working tree may still have uncommitted local edits — run check/test before assuming clean.

```bash
cargo check --lib
cargo test --lib
```

---

## 1. Project orientation (stable)

Rust economic / civilization sim (Civ x Victoria style). **Authoritative long-form design** lives in the local Obsidian vault:

| Role | Path |
|------|------|
| Primary (prefer) | `/home/jeremy/Documents/Obsidian Vault/Game Ideas/EconCiv/` |
| Historical | `…/Simlper Economy Simulator/` (prefer EconCiv on conflict) |

In-repo navigation:

| File | Role |
|------|------|
| `AGENTS.md` | Rules, vault paths, code map, build |
| `STYLE.md` | Builders, tests `*_should`, f64, docs tone |
| `docs/design-vocabulary.md` | **Canonical names** (tier sat, desire sat, sentiment, …) |
| `docs/proposals/` | Focused design notes (household, institutions) |
| `TODO.md` | Working turn-pipeline checklist |
| `reviewlog.md` | Open review debt only |

**ASCII only in comments** (`Sum`, `->`, plain `-`). Do not edit vault notes unless the user asks.

---

## 2. Big picture direction

1. **Pop day logic first** — desires, consume, growth, sentiment, decay — implemented largely on `Pop`, then wired into `PlayState::advance_turn` as phases mature.
2. **Factuals vs game state** — definitions (goods, species, culture, religion, processes) vs live map/markets/actors/prices.
3. **Turn shell** — `advance_turn` lists many phases; several are orchestrator-wired with stub leaves; market day and intermarket remain the largest open sim loops.
4. **Household rework (just landed)** — replace static household recipes with **averages + count** evolved by **`DemographicRates`**. Rates are **not** stored on each pop; resolve via factuals when growth needs them.
5. **Scale expectation** — potentially thousands to millions of pops (split by demographics and job). Prefer designs that scale with **unique demographic combos**, not full cartesian precompute of all definitions.

---

## 3. What was done recently (session / household arc)

### Household + demographic rates (main thrust)

- **`HouseholdDef` removed.** Single living type: `Household` in `src/game/household.rs`.
- **Composition:** per-household averages `adult` / `child` / `elder`, `count`, female fractions `*_mf`, labor per band, live `partnership_rate`.
- **`DemographicRates`:** birth / infant / maternal + mortality triples `(total, male, female)` + target `partnership_rate`.
- **`Household::update(&rates)`:** one year of births, deaths, aging (20y childhood, 40y adulthood), partnership lerp, rewrite averages and count. Debug invariants: non-negative members, sex ratios in `0..=1`.
- **Deltas on demographics:** `species_demo_eff`, `culture_demo_eff`, `religion_demo_eff` (zero + add stacking).
- **`DemoRow`:** holds `household` + ids only — **no** rates field.
- **`Factuals::get_demographic_rates(demo)`:** `baseline + species + culture + religion` (skip culture/religion id 0).
  - **Policy: recompute every call** (no cache). Parallel-safe with shared `&Factuals` (e.g. rayon growth).
  - **If too slow later:** day-fill a cache of **living** demographic id keys only (not full product); see comments on `get_demographic_rates` and the household primer.
- **`Pop::growth_phase(&factuals)`:** structural rates from factuals + same-day sat/stored mods → `household.update`; sets `previous_growth` from count delta. Dead pops (`count < 1`) skip.
- Demographic phase in playstate currently runs **`update_desires`** (not a removed `demographic_update` that snapped household defs).

**Deep dive:** `docs/proposals/household-population-refactor-primer.md`.

### Earlier (still relevant)

| Area | Status |
|------|--------|
| Desire hierarchy | Platonic → DemoDesire → pop Desire; tiers basic/common/luxury |
| `create_orders`, consume, satisfy | Implemented + tests |
| Sentiment | Axes + mods; `update_sentiments` (ex-process_satisfaction) records tier sat **sums**, wealth, SOL, mood |
| Pop decay_goods | Implemented + tests |
| Firm `run_production` | Real + tests; **not** wired into turn production phase |
| Institutions v0 | Passive effects; draft in `docs/proposals/institution-draft.md` |
| Design vocabulary | Tier sat is **sum of success rates**, not average |

### Turn pipeline (high level)

| Phase | State |
|-------|--------|
| Start of day / environment / player actions | Largely stub / todo |
| Player bonuses + demos | Partial (institutions/firms passive; pops `update_desires`) |
| Intra / inter market | Stub |
| Production + planning | Stub (`run_production` exists) |
| Pop consumption | Wired |
| Pop growth | Wired (`growth_phase` + new household update) |
| Migration | Orchestrator structure; leaf `todo!`s |
| Record keeping | Wired fan-out; actor methods stub |
| Map changes | Stub |
| Good decay | Wired shell; leaf completeness varies |

Details: `TODO.md`, `src/playstate.rs`.

---

## 4. Design rules agents keep forgetting

| Topic | Rule |
|-------|------|
| **Tier sat** | `records.tier_sat` = **sum** of desire success rates (+ boosts), not average |
| **Mood from tier sat** | May normalize by desire count for sentiment only; do not store that average as tier sat |
| **Rates on pop** | Do **not** re-add `DemoRow.rates` without user direction |
| **Rate resolution** | `Factuals::get_demographic_rates`; recompute-per-call is intentional |
| **Job vs demographics** | Jobs multiply pops; rate keys are demographic ids only (unless rates later depend on job) |
| **Vocabulary** | Prefer `docs/design-vocabulary.md` over chat shorthand |
| **Comments** | ASCII only; prefer `Sum` over summation glyphs |

---

## 5. Known debt / next-friendly work

### Comments still stale (fix carefully; user may want to edit themselves)

- Playstate / firm / institution docs still mention `Pop::demographic_update` in places.
- Some growth docs still describe old count-multiplier model.
- `TODO.md` household refactor bullet may lag the landed rates model.

### Open systems

- **Intramarket day** — orders exist on pops; matching/trading not built.
- **Wire `update_sentiments`** into the turn if not already (after consume/growth).
- **Migration leaves**, record-keeping bodies, production phase call to `run_production`.
- **DemographicEffect** list → rates mapping incomplete; research/culture passives placement TBD.
- **Class** demographics still largely unimplemented.
- **Sentiment / SOL tuning** listed under balancing in TODO.
- Optional later: day-fill rate cache if recompute shows up at huge pop counts.

### Review log

`reviewlog.md` was cleared in an earlier pass; re-file items if they bite again.

---

## 6. Where to look in code

| Concern | Location |
|---------|----------|
| Household / rates math | `src/game/household.rs` |
| Rate resolve | `src/game/factuals.rs` → `get_demographic_rates` |
| Growth + same-day rate mods | `src/game/pop.rs` → `growth_phase`, `same_day_growth_rate_mods` |
| Demo row | `src/game/pop_property.rs` |
| Species/culture/religion deltas | `species_demo_eff` / `culture_demo_eff` / `religion_demo_eff` |
| Sentiment | `src/game/sentiment.rs`, `Pop::update_sentiments` |
| Turn order | `src/playstate.rs` → `advance_turn` |
| Effects catalog | `src/game/effects.rs` |
| Household design depth | `docs/proposals/household-population-refactor-primer.md` |

---

## 7. Suggested first steps for a new agent

1. Read `AGENTS.md` + this handoff + household primer (if touching pops/growth).
2. `cargo test --lib`.
3. Skim `TODO.md` for turn-phase priority; do not invent a third household model.
4. Match `STYLE.md` on any edits; update `reviewlog.md` when doing reviews.
5. Prefer vault **EconCiv** notes for design intent when code and notes disagree — **call out conflicts** rather than silent invention.

---

## 8. One-line status

**Pop economic day is largely implemented; household growth is rates-driven and integrated; market day and most other turn phases remain the open frontier. Rates resolve from factuals per pop/day (no cache); cache living combos only if profiling demands it.**
