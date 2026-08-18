# Agent handoff — EconCiv rework

**Branch:** `EconCiv-Rework-Branch`  
**Handoff date:** 2026-08-18  
**Purpose:** Catch a new agent/session up on recent work and direction. Prefer this plus `AGENTS.md`, `STYLE.md`, `TODO.md`, `reviewlog.md`, and `docs/design-vocabulary.md` over inventing process from scratch.

**Build (as of this wrap-up):** `cargo test --lib` was green (159 tests) after pop record-keeping and the review pass. Record-keeping / wiring / review-log edits are committed on this branch.

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

## 3. What landed this session (2026-08-18)

### Pop `record_keeping` (main thrust)

`Pop::record_keeping` is implemented and tested (`mod record_keeping_should`). End-of-day snapshot **then** rewrite of next-day shop/save targets. Does **not** clean dead pops, does **not** call `update_sentiments`, does **not** decay.

**Snapshot (using still-current `shop_target`):**

- Census: `pop_size`, `labor`, `pop_history`
- Balance: leftover `liquid_wealth`, `saved_amv`, `consumption_amv` (`consumed + used`), `shop_fill`
- `shop_fill` reconstructs post-shop stock as `qty + consumed + used`; extra of one good does not cover a miss on another; empty wants -> `1.0`
- Leaves alone: `tier_sat`, SOL / `wealth_amv`, `income_amv`, `net_migration`, `previous_growth`

**`update_planning`** (not "knobs"): lerps `risk_appetite`, `savings_ratio`, `time_preference` at `PLANNING_LERP_RATE` (0.15).

- Risk mood is **weighted**: hope > happiness raising appetite; fear > anger lowering it. Contentment also lowers risk appetite. Falling SOL raises savings. Unmet basic raises savings (configurable). Caps: savings `0..=5` days, time preference `0..=1`.
- Constants live in `src/game/config.rs` → `pop_constants`.

**Shop / save rewrite:**

- **Consume need** = `max(unsatisfied target units, consumed + used)`. That is the live-use side of shop, not "consume-half".
- `shop_target` (tradeable) = consume need + `save_target`. Untradeable: shop/save stay `0`; `desire_needs` still recorded.
- Unsatisfied units: `(amount - satisfaction).max(0)` added onto **every** target (`sat / efficiency`). Does **not** walk another full level. `DesireTarget.efficiency` must be **positive** (`debug_assert`; zero/negative not accepted).
- **Savings ratio** = days of buffer (`1.0` = one extra day of the cheapest basic+common basket AMV), grown by today's household growth.
- Fear scales **substitutability**: calm may hold the pile as highly salable AMV; afraid parks more of it on the actual basket goods (`SAVINGS_SUBSTITUTABILITY_CALM` / `_FEAR`).

One write pass over property (`entry`); no repeated `ensure_property_row`.

### Turn wiring

- `phase_update_sentiments` runs **after growth, before migration**.
- Pops get `Market::history()` (AMV snapshot from `MarketGood.amv`). Salability is not on `MarketGood` yet (readers default missing to `1.0`).
- Histories are snapshotted **before** the record-keeping rayon scope so pops do not fight market mutation.
- Pops not in any market get an empty history.

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
| **Target efficiency** | Always **positive**. Zero is worthless; negatives not supported. Assert, do not treat 0 as a valid skip-path. |
| **Savings ratio** | **Days of buffer**, not a share of leftover liquid wealth |
| **Consume need** | Live-use restock, then + save. Not "consume-half" |
| **Vocabulary** | Prefer `docs/design-vocabulary.md` over chat shorthand |
| **Comments** | ASCII only; **add, do not edit or replace** existing comments unless asked |
| **Knob** | Player-facing only |

---

## 5. Known debt / next-friendly work

### Natural next system

- **Intramarket day** — pops can `create_orders`; matching / trading / wages / AMV updates are not built. `next_shopping_trip` is still `todo!()`. This is the next closed economic loop.

### Nearby leftovers (do not start unless asked)

- Firm / institution / market / state `record_keeping` bodies still `todo!()`
- `run_production` exists + tests; **not** wired into the production phase
- `Pop::start_day` exists; day-start phase still stub (TODO: "Completed not Connected")
- Migration orchestrator exists; leaves are `todo!()` (wants live sentiment + liquid wealth)
- Class demographics unimplemented (vault: park this)
- MarketGood has no salability field yet
- `income_amv` is not zeroed at day start (harmless until market pays)
- Optional later: day-fill rate cache if `get_demographic_rates` shows up at huge pop counts

### Comments still stale (fix only if the user asks)

- `PopRecords.savings_ratio` field still says "share of liquid wealth"
- `decay_goods` still calls `saved` a wish target in one place
- Original short `record_keeping` docblock was left as-is
- Playstate / firm / institution docs may still mention `Pop::demographic_update`
- `TODO.md` household-helper bullet lags the landed rates model
- Vault `Pops.md` household section still has a REWORK banner

### Review log

Local review 2026-08-18 re-opened items in `reviewlog.md`. Highest: growing-pop `shop_target` is scaled twice (`savings_growth_buffer` then morning `update_desires`). Full notes: `/tmp/grok-1000/grok-review-72fee57c.md`.

---

## 6. Where to look in code

| Concern | Location |
|---------|----------|
| Record keeping + planning + shop/save | `src/game/pop.rs` → `record_keeping`, `update_planning`, `rewrite_shop_and_save_targets` |
| Pop records / property rows | `src/game/pop_property.rs` |
| Planning tunables | `src/game/config.rs` → `pop_constants` |
| Market price snapshot | `src/game/market.rs` → `Market::history` |
| Turn order / wires | `src/playstate.rs` → `advance_turn`, `phase_update_sentiments`, `phase_record_keeping` |
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

**Pop economic day is closed through record keeping: sentiments wired, consume need + days-of-buffer savings (fear-scaled substitutability), efficiency must be positive. Household growth is rates-driven. Intramarket day is the open frontier.**
