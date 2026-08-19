# Agent handoff — EconCiv rework

**Branch:** `EconCiv-Rework-Branch`  
**Handoff date:** 2026-08-18  
**Purpose:** Catch a new agent/session up on recent work and direction. Prefer this plus `AGENTS.md`, `STYLE.md`, `TODO.md`, `reviewlog.md`, and `docs/design-vocabulary.md` over inventing process from scratch.

**Build (as of this wrap-up):** `cargo test --lib` green (**169** tests) after closing the 2026-08-18 review-log items and thinning growth-factor NaN fallbacks to `debug_assert`.

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
| `docs/design-vocabulary.md` | **Canonical names** (tier sat, desire sat, consume need, …) |
| `docs/proposals/` | Focused design notes (household, institutions) |
| `TODO.md` | Working turn-pipeline checklist |
| `reviewlog.md` | Open review debt only |

**ASCII only in comments** (`Sum`, `->`, plain `-`). Do not edit vault notes unless the user asks. **Add comments, do not edit or replace existing ones** unless the user asks.

---

## 2. Big picture direction

1. **Pop day logic first** — desires, consume, growth, sentiment, record keeping, decay — implemented largely on `Pop`, then wired into `PlayState::advance_turn` as phases mature.
2. **Factuals vs game state** — definitions (goods, species, culture, religion, processes) vs live map/markets/actors/prices.
3. **Turn shell** — `advance_turn` lists many phases; several are orchestrator-wired with stub leaves; **intramarket day is the next real system**.
4. **Household** — averages + count evolved by `DemographicRates`. Rates are **not** stored on each pop; resolve via factuals when growth needs them. Do not reopen that model.
5. **Scale expectation** — potentially thousands to millions of pops (split by demographics and job). Prefer designs that scale with **unique demographic combos**, not full cartesian precompute.

---

## 3. What is true now (2026-08-18)

### Pop economic day (closed through record keeping)

`Pop::record_keeping` is implemented and tested (`mod record_keeping_should`). End-of-day snapshot **then** rewrite of next-day shop/save targets. Does **not** clean dead pops, does **not** call `update_sentiments`, does **not** decay.

**Snapshot (using still-current `shop_target`):**

- Census: `pop_size`, `labor`, `pop_history`
- Balance: leftover `liquid_wealth`, `saved_amv`, `consumption_amv` (`consumed + used`), `shop_fill`
- `shop_fill` reconstructs post-shop stock as `qty + consumed + used`; extra of one good does not cover a miss on another; empty wants -> `1.0`
- Leaves alone: `tier_sat`, SOL / `wealth_amv`, `income_amv`, `net_migration`, `previous_growth`

**`update_planning`** (not "knobs"): lerps `risk_appetite`, `savings_ratio`, `time_preference` at `PLANNING_LERP_RATE` (0.15).

- Risk mood is **weighted**: hope > happiness raising appetite; fear > anger lowering it. Contentment also lowers risk appetite. Falling SOL raises savings. Unmet basic raises savings (configurable). Caps: savings `0..=5` days, time preference `0..=1`.
- Constants live in `src/game/config.rs` → `pop_constants`.

**Shop / save rewrite (record keeping owns tomorrow's plan):**

- **Consume need** = `max(unsatisfied target units, consumed + used)`.
- Consume need and `desire_needs` are written already scaled by `planning_growth_factor()` (post-growth, pre-migration: `(count - net_migration) / (that - previous_growth)`).
- `shop_target` (tradeable) = scaled consume need + `save_target`. Untradeable: shop/save stay `0`; `desire_needs` still recorded (also scaled).
- **Morning `update_desires` does not multiply shop/save.** It still resyncs desire amounts from demographics. A mid-day definition change that shifts demand scale is allowed to roll in at the next rewrite (gentle first-day miss is accepted).
- Unsatisfied units: `(amount - satisfaction).max(0)` added onto **every** target (`sat / efficiency`). Does **not** walk a full extra level.
- **Savings ratio** = days of the cheapest **tradeable** basic+common cover (skip untradeable, respect `cap`). Shared helper: `cheapest_tradeable_cover`. Fear parking uses that same basket (substitutes do not each claim a full desire).
- The save pile **inflates** with household growth and **does not shrink** on decline.
- Fear scales **substitutability**: calm may hold the pile as highly salable AMV; afraid parks more of it on the actual basket goods.

`planning_growth_factor` divides and `debug_assert`s a finite positive result. Do **not** add runtime `is_finite` fallbacks around it. `savings_growth_buffer` still has `growth_f <= 1.0 -> 1.0`; that is the no-shrink rule, not a NaN guard.

### `create_orders`

1. Desire-order planned shop shortfalls (`shop_target - quantity`).
2. Remaining planned shop shortfalls that are not desire targets (parked savings / gold).
3. One opportunistic extra pass over desire goods that were not in the plan.

Pass 2 is part of the shop **plan**, so it runs before leftover-budget luxury/extra buys. There is no infinite luxury shopping loop; luxury looping is only in `consume`.

### Consume / reserved

`satisfy_one_desire` floors `reserved` at 0. `PopPRow::saved()` treats negative reserved as 0 (`quantity - reserved.max(0)`). Reserved must never go negative. Extra luxury consume may still empty leftover stock; that is allowed. Pacing extra luxury passes is **deferred** (`TODO.md`).

### Turn wiring

- `phase_player_bonuses_and_demographic_updates` is wired: institutions `apply_passive_effects`, firms `apply_passive_bonuses` (stub body), pops `update_desires`.
- `phase_update_sentiments` runs **after growth, before migration**.
- `PlayState.market_lookups` (`MarketLookups`): one `MarketHistory` per market plus `pop_id -> market_id`. Rebuilt at sentiments and again at record keeping (membership may change once migration writes). Pops not in any market get an empty history.
- Salability is not on `MarketGood` yet (readers default missing to `1.0`).
- Histories are snapshotted **before** the record-keeping rayon scope so pops do not fight market mutation.

### Language

- Do **not** use **knob** / **lever** in code, comments, or design talk except player-facing UI. Prefer **planning variable**, **tunable**, **constant**.
- Prefer **consume need** over consume-half.

---

## 4. Design rules agents keep forgetting

| Topic | Rule |
|-------|------|
| **Tier sat** | `records.tier_sat` = **sum** of desire success rates (+ boosts), not average |
| **Mood from tier sat** | May normalize by desire count for sentiment only; do not store that average as tier sat |
| **Rates on pop** | Do **not** re-add `DemoRow.rates` without user direction |
| **Rate resolution** | `Factuals::get_demographic_rates`; recompute-per-call is intentional |
| **Job vs demographics** | Jobs multiply pops; rate keys are demographic ids only (unless rates later depend on job) |
| **Target efficiency** | Always **positive**. Zero is worthless; negatives not supported. `debug_assert` only; do not also `continue` on `<= 0` |
| **Shop / save owner** | Record keeping writes next-day shop/save (post-growth). Morning does not re-scale them for `previous_growth` |
| **Savings ratio** | **Days of buffer**, not a share of leftover liquid wealth. Save pile does not shrink on decline |
| **Consume need** | Live-use restock, then + save. Not "consume-half" |
| **Reserved** | Never negative. Extra luxury consume eats unreserved stock |
| **NaN / inf** | `debug_assert` if it must never happen. Do not sprinkle runtime `is_finite` fallbacks on the hot path |
| **Vocabulary** | Prefer `docs/design-vocabulary.md` over chat shorthand |
| **Comments** | ASCII only; **add, do not edit or replace** existing comments unless asked |
| **Knob** | Player-facing only |

---

## 5. Known debt / next-friendly work

### Natural next system

- **Intramarket day** — pops can `create_orders`; matching / trading / wages / AMV updates are not built. `next_shopping_trip` is still `todo!()`. This is the next closed economic loop.

### Nearby leftovers (do not start unless asked)

- Firm / institution / market / state `record_keeping` bodies still `todo!()`
- Firm `apply_passive_bonuses` is a stub; region/market bonus apply is unchecked
- `run_production` exists + tests; **not** wired into the production phase
- `Pop::start_day` exists; day-start phase still stub (TODO: "Completed not Connected")
- Migration orchestrator exists; leaves are `todo!()` (wants live sentiment + liquid wealth)
- Class demographics unimplemented (vault: park this)
- MarketGood has no salability field yet
- `income_amv` is not zeroed at day start (harmless until market pays)
- Optional later: day-fill rate cache if `get_demographic_rates` shows up at huge pop counts
- Optional later: `Pop.market_id` (update on migrate) instead of rebuilding `pop_to_market`
- Optional later: pace luxury consume so one desire does not empty leftover stock (see `TODO.md`)

### Comments still stale (fix only if the user asks)

- `Pop::update_desires` rustdoc still lists step 3 as scaling `shop_target` / `desire_needs` for growth (that block is gone)
- `PopRecords.savings_ratio` field still says "share of liquid wealth"
- `decay_goods` still calls `saved` a wish target in one place
- Original short `record_keeping` docblock was left as-is
- Playstate / firm / institution docs may still mention `Pop::demographic_update`
- `TODO.md` household-helper bullet lags the landed rates model
- Vault `Pops.md` household section still has a REWORK banner; morning step 3.5 still says resize shopping targets (record keeping owns that now)

### Review log

Open review debt is empty. 2026-08-18 items are closed in `reviewlog.md` (owner A, reserved floor, cheapest cover, MarketLookups, `create_orders` planned-shop pass). Luxury leveling and `Pop.market_id` are deferred.

---

## 6. Where to look in code

| Concern | Location |
|---------|----------|
| Record keeping + planning + shop/save | `src/game/pop.rs` → `record_keeping`, `update_planning`, `rewrite_shop_and_save_targets`, `planning_growth_factor` |
| Cheapest tradeable basket | `src/game/pop.rs` → `cheapest_tradeable_cover` |
| Orders | `src/game/pop.rs` → `create_orders` (plan, then parked shop, then extra desires) |
| Pop records / property rows | `src/game/pop_property.rs` |
| Planning tunables | `src/game/config.rs` → `pop_constants` |
| Market price snapshot | `src/game/market.rs` → `Market::history`, `MarketLookups` |
| Turn order / wires | `src/playstate.rs` → `advance_turn`, `phase_update_sentiments`, `phase_record_keeping`, `rebuild_market_lookups` |
| Household / rates math | `src/game/household.rs` |
| Rate resolve | `src/game/factuals.rs` → `get_demographic_rates` |
| Sentiment | `src/game/sentiment.rs`, `Pop::update_sentiments` |
| Desire targets | `src/game/desire.rs` → `DesireTarget` |
| Household design depth | `docs/proposals/household-population-refactor-primer.md` |

---

## 7. Suggested first steps for a new agent

1. Read `AGENTS.md` + this handoff + `docs/design-vocabulary.md`.
2. `cargo test --lib`.
3. Skim `TODO.md` for turn-phase priority. Next system is **intramarket day**, not a third household model and not class/graphics.
4. Match `STYLE.md` on any edits; update `reviewlog.md` when doing reviews.
5. Prefer vault **EconCiv** notes for design intent when code and notes disagree — **call out conflicts** rather than silent invention.

---

## 8. One-line status

**Pop economic day is closed through record keeping: next-day shop/save written there (not re-scaled in the morning), cheapest tradeable cover, reserved never negative, MarketLookups for evening phases. Review log is empty. Intramarket day is the open frontier.**
