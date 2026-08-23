# Agent handoff — EconCiv rework

**Branch:** `EconCiv-Rework-Branch`  
**Handoff date:** 2026-08-22  
**Purpose:** Catch a new agent/session up on recent work and direction. Prefer this plus `AGENTS.md`, `STYLE.md`, `TODO.md`, `reviewlog.md`, and `docs/design-vocabulary.md` over inventing process from scratch.

**Build (as of this wrap-up):** `cargo test --lib` green (**217** tests).

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
| `docs/design-vocabulary.md` | **Canonical names** (tier sat, desire sat, consume need, **order priority**, …) |
| `docs/proposals/` | Focused design notes (household, institutions, **market-order-priority**) |
| `TODO.md` | Working turn-pipeline checklist |
| `reviewlog.md` | Open review debt only |

**ASCII only in comments** (`Sum`, `->`, plain `-`). Do not edit vault notes unless the user asks. **Add comments, do not edit or replace existing ones** unless the user asks.

---

## 2. Big picture direction

1. **Pop day logic** — desires, consume, growth, sentiment, record keeping, decay — implemented largely on `Pop`, wired into `PlayState::advance_turn` as phases mature.
2. **Factuals vs game state** — definitions (goods, species, culture, religion, processes) vs live map/markets/actors/prices.
3. **Turn shell** — `advance_turn` lists many phases; several are orchestrator-wired with stub leaves. **Intramarket day is the active system:** order priority and matching exist; deals / settlement / PlayState wiring do not.
4. **Household** — averages + count evolved by `DemographicRates`. Rates are **not** stored on each pop; resolve via factuals when growth needs them. Do not reopen that model.
5. **Scale expectation** — potentially thousands to millions of pops (split by demographics and job). Prefer designs that scale with **unique demographic combos**, not full cartesian precompute.

---

## 3. What is true now (2026-08-22)

### MarketGood

`MarketGood` has a real `Default` (AMV `1.0`, salability `0.6`, average price `1.0`, rest `0`) plus `new()` / `with_*` / `set_*`.

Setter invariants (`src/game/market.rs`, tunables in `config::market_constants`):

- **AMV / average_price:** never `0`. Values with `|x| < AMV_MIN_ABS` (`0.00001`) bounce past 0 from the previous sign (positive -> slightly negative, and vice versa).
- **Salability:** clamp to `0.0..=1.0`.
- **Non-negative** (`debug_assert`): production, consumption, stock, supply, suppliers, demand, buyers, volume, requests, purchased, tender, payment.
- **Imported** may be negative (exports).

Fields are still `pub`; day logic should go through setters. `Market::history()` still snapshots **AMV only**; salability on `MarketGood` is not copied into `MarketHistory` yet (readers still default missing salability to `1.0`).

### Order priority

Full note: `docs/proposals/market-order-priority.md`. Vocabulary: **order priority**.

`MarketOrder.priority` is used **two ways**:

| Side | Meaning | Direction |
|------|---------|-----------|
| Buy / request | FCFS sort key | **Lower goes first.** RNG only among equal values. |
| Sell / offer | Selection **weight** | **Higher is more likely.** |

Buy-side bands (pops `[4, 5)`, firms `[2, 3)`) are `debug_assert`ed only on **buys**. Sells only need `priority > 0`. `assert_priority_for_origin` is `cfg(debug_assertions)` (release stub).

Buy-side named slots live in `config::market_priority` (`StateMarketSlot`, `MarketSlot::priority` for institutions `1` / `3` / `5`). There is **no** state-among-pops slot. State firm inserts sit at `band_end - STATE_FIRM_SLOT_MARGIN` (`2.49`, `2.99`). Firm rank helpers lerp toward those slots and never reach them.

Wealth rank for pop buys: **per household**, **total AMV** (`wealth_amv / household count`), not liquid. `unit_rank = 1 - wealth / max_wealth` (richest -> `0` -> band start). `wealth_unit_rank` / `pop_priority_from_wealth` exist; the **market** must stamp `[4, 5)` when it receives orders. `Pop::create_orders` still writes `POP_START` (`4.0`) as an unranked placeholder. Offers are not generated yet.

Sell-side compose (stamp on create, then mutate after fills):

```text
compose_sell_priority(actor_band, supply, successful_sells)
  = 1 / max(actor_band, SELL_ACTOR_PRIORITY_FLOOR)
    + sqrt(supply)
    + SELL_SUCCESS_BONUS * fills
```

Floor is `0.01` (so `STATE_FIRST` `0.0` is defined). Success bonus is `0.25`, added with `MarketOrder::add_sell_success_bonus` after a fill (flat, not recomputed as a product). Marketing adds later.

### Matching (`Market::match_orders`)

One pass, **does not mutate** the books. Caller owns remove / restamp / reinsert.

- `buys` sorted by buy priority (lowest first). `sells` sorted by target good id.
- Only the **front** buy-priority group is considered (shuffled). Later groups wait for the next call so they cannot jump the queue.
- At most **one** `matched` pair (weighted sell of that good). Coincidence: if both orders have `Some` counter-offer and the goods match, that sell's weight is doubled **for this pick only** (`SELL_COINCIDENCE_WEIGHT = 2.0`). Request/offer with no counters do not get it.
- Self-trade skipped. No other-origin seller of that good -> `unmatched_buys` (may be **several** in the front group). Caller restamps/drops those while the one deal runs.
- Matchable leftovers in the same group stay in the book (not failed).
- Empty buy book -> empty batch (`is_empty()`).
- RNG: `rand` `0.9`, `&mut impl Rng`.

Return: `OrderMatchBatch { matched: Option<OrderMatch>, unmatched_buys: Vec<usize> }`.

### Market tester CLI

`cargo run --example market_tester` (`examples/market_tester.rs`). Intended to grow into a full intramarket-day loop. **Today it only drives `match_orders`.**

- **No factuals, no living actors, no settlement.** Prefab names are labels on ids so humans can talk about the same goods and actors. They are not a goods catalog.
- Hand-typed `request` / `offer` / `buy` / `sell` into two in-memory books. Feels arbitrary because it is: you are authoring `MarketOrder`s directly, not shopping from pops or firms. That is expected at this stage.
- On a TTY the screen clears and redraws after each command (header, prefabs, buy table, sell table, last log). Empty enter or `cls` refreshes. Piped stdout skips ANSI and only prints the last log.
- `match` is read-only. Books are not dropped or restamped. `clear` empties the books, not the screen. `drop buy N` / `drop sell N` use the table `#` column.
- Names or raw ids both work (`request farmers grain 3` or `request pop 1 1 3`).

**Prefab goods (id / name):** 1 grain, 2 bread, 3 timber, 4 tools, 5 cloth, 6 iron, 7 fish, 8 pottery, 9 meat, 10 coin.

**Prefab actors:** state 1 crown; inst 1 guild, inst 2 temple; firm 1 farm, firm 2 mill, firm 3 trader; pop 1 farmers, pop 2 millers, pop 3 laborers, pop 4 townsfolk.

Default buy order priority by kind: state `0`, inst `1`, firm `2` (merchant), pop `4`. Sell default is `compose_sell_priority`. Buy/sell tables columns: `#`, kind, actor, good, amt, prio, amv, counter.

**Not built:** deal execution, inventory transfer, AMV drift, `MarketGood` volume/purchased/tender updates, `next_shopping_trip`, PlayState intramarket phase. `main.rs` is still the Bevy hex stub.

### Pop economic day (still closed through record keeping)

Unchanged from 2026-08-18 in substance. `Pop::record_keeping` snapshots then rewrites next-day shop/save. Morning `update_desires` does **not** multiply shop/save. Consume need, days-of-buffer savings, reserved never negative, `create_orders` three passes (desire shop, parked non-desire shop, opportunistic extra). `extract_special_resources` first pass exists; playstate harvests but does **not** route yield into the owning state's pool.

### Turn wiring

- `phase_intra_market_day` is still `todo!()`. Matcher is a lib function, not wired.
- Sentiments after growth, before migration; `MarketLookups` rebuilt at sentiments and record keeping.
- `extract_special_resources` phase is wired (yield discarded).

### Language

- Do **not** use **knob** / **lever** except player-facing UI. Prefer **planning variable**, **tunable**, **constant**.
- Prefer **consume need** over consume-half.
- **Order priority** vs **desire priority** — never say bare "priority" in design talk.

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
| **Buy order priority** | Lower number first. RNG among ties only. |
| **Sell order priority** | Higher number = more weight. Compose then flat-add; do not invert at match time. |
| **Matching** | One success per pass, front group only. Multiple hopeless buys OK. Do not batch several deals. |
| **Vocabulary** | Prefer `docs/design-vocabulary.md` over chat shorthand |
| **Comments** | ASCII only; **add, do not edit or replace** existing comments unless asked |
| **Knob** | Player-facing only |

---

## 5. Known debt / next-friendly work

### Natural next system

- **Deal / settlement** — matcher returns indices only. Next closed loop: move goods between actors, apply AMV/salability rules, update `MarketGood` deal stats, `add_sell_success_bonus`, shrink/remove orders, maybe restamp unmatched buys. Then a caller loop around `match_orders`. The CLI can grow around that loop once a deal function exists.
- **Stamp pop wealth ranks** when a market receives orders (`wealth_amv / household count` vs market max).
- **Offer generation** — pops still only emit requests.
- Market tester is a hand-typed order book, not actor-driven shopping. Do not invent a second shopping model in the example; keep feeding `MarketOrder`s until settlement exists.

### Nearby leftovers (do not start unless asked)

- Firm / institution / market / state `record_keeping` bodies still `todo!()`
- Firm `apply_passive_bonuses` is a stub; region/market bonus apply is unchecked
- `run_production` exists + tests; **not** wired into the production phase
- `Pop::start_day` exists; day-start phase still stub (TODO: "Completed not Connected")
- Migration orchestrator exists; leaves are `todo!()` (wants live sentiment + liquid wealth)
- Class demographics unimplemented (vault: park this)
- `Market::history()` does not copy `MarketGood.salability`
- `income_amv` is not zeroed at day start (harmless until market pays)
- Player-resource yield not routed onto `State.resources`
- Optional later: day-fill rate cache if `get_demographic_rates` shows up at huge pop counts
- Optional later: `Pop.market_id` (update on migrate) instead of rebuilding `pop_to_market`
- Optional later: pace luxury consume so one desire does not empty leftover stock (see `TODO.md`)
- Optional later: marketing add on sell weight; recompute `sqrt(supply)` after partial fill; merchant vs producer firm flag; subordinated-firm priority; state purchase buckets

### Comments still stale (fix only if the user asks)

- `Pop::update_desires` rustdoc still lists step 3 as scaling `shop_target` / `desire_needs` for growth (that block is gone)
- `PopRecords.savings_ratio` field still says "share of liquid wealth"
- `decay_goods` still calls `saved` a wish target in one place
- Original short `record_keeping` docblock was left as-is
- Playstate / firm / institution docs may still mention `Pop::demographic_update`
- `TODO.md` household-helper bullet lags the landed rates model
- `Market::history` rustdoc still says salability is not on `MarketGood`
- Vault `Pops.md` household section still has a REWORK banner; morning step 3.5 still says resize shopping targets (record keeping owns that now)
- `compose_sell_priority` formula comments in the proposal may lag live `SELL_*` constants — prefer the constants

### Review log

Open review debt is empty. Last close-out was 2026-08-18. This market slice has not had a `/review` pass.

---

## 6. Where to look in code

| Concern | Location |
|---------|----------|
| Record keeping + planning + shop/save | `src/game/pop.rs` → `record_keeping`, `update_planning`, `rewrite_shop_and_save_targets`, `planning_growth_factor` |
| Cheapest tradeable basket | `src/game/pop.rs` → `cheapest_tradeable_cover` |
| Pop request orders | `src/game/pop.rs` → `create_orders` (plan, then parked shop, then extra desires) |
| Order type + buy/sell priority helpers | `src/game/marketorder.rs` |
| Matching | `src/game/market.rs` → `Market::match_orders`, `OrderMatchBatch` |
| Market CLI (no factuals) | `examples/market_tester.rs` — `cargo run --example market_tester` (prefabs, TTY redraw, order tables; matcher only) |
| MarketGood setters / AMV bounce | `src/game/market.rs` → `MarketGood` |
| Order-priority tunables | `src/game/config.rs` → `market_priority`, `market_constants` |
| Priority design (deferred too) | `docs/proposals/market-order-priority.md` |
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

1. Read `AGENTS.md` + this handoff + `docs/design-vocabulary.md` + `docs/proposals/market-order-priority.md`.
2. `cargo test --lib`.
3. Next closed loop is **deal/settlement** around `match_orders`, not a third household model and not class/graphics. Probe matching with `cargo run --example market_tester` (TTY redraw + order tables; no deals yet).
4. Match `STYLE.md` on any edits; update `reviewlog.md` when doing reviews.
5. Prefer vault **EconCiv** notes for design intent when code and notes disagree — **call out conflicts** rather than silent invention. Vault `Turns.md` sequential shopping walk vs collect-and-match: **match** is the live model.

---

## 8. One-line status

**Pop economic day is closed through record keeping. Market orders have dual-use priority; `Market::match_orders` returns one front-group deal plus any hopeless buys. Market tester CLI exists (`examples/market_tester.rs`): prefab ids, TTY redraw, buy/sell tables, hand-typed orders, matcher only. No settlement, no intramarket PlayState wire. Next: execute a deal from a match.**
