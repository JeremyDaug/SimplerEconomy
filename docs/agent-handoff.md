# Agent handoff — EconCiv rework

**Branch:** `EconCiv-Rework-Branch`  
**Handoff date:** 2026-09-07  
**Purpose:** Catch a new agent/session up on recent work and direction. Prefer this plus `AGENTS.md`, `STYLE.md`, `TODO.md`, `reviewlog.md`, and `docs/design-vocabulary.md` over inventing process from scratch.

**Build (as of this wrap-up):** `cargo test --lib` green (**415** tests). CLI smoke: `cargo run --example market_tester` then `day` / `stock` / `orders` / `processes` / `csv` / `day 5`. Tester `day` still uses `pay_wage_shares`, not `settle_labor_contracts`. Tester `day` calls firm `record_keeping` (records + `plan`) after production and pop consume.

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
3. **Turn shell** — `advance_turn` lists many phases; several are orchestrator-wired with stub leaves. **Intramarket day is the active system:** `Market::run_market_day` collects pop/firm orders, collates books, loops match / deal / finalize, and records `MarketGood` stats. PlayState intramarket phase, institution/state orders, and `next_shopping_trip` are not wired. AMV/salability now update on the market.
4. **Household** — averages + count evolved by `DemographicRates`. Rates are **not** stored on each pop; resolve via factuals when growth needs them. Do not reopen that model.
5. **Scale expectation** — potentially thousands to millions of pops (split by demographics and job). Prefer designs that scale with **unique demographic combos**, not full cartesian precompute.

---

## 3. What is true now (2026-09-07)

**Landed this wrap.** Labor contracts **operate** (no hiring/creation, not wired into the tester CLI or PlayState). Do **not** put labor on the goods order book; employment is a `Workforce` roster. [`LaborSettlement::settle`] (thin [`Firm::settle_labor_contracts`] wrapper) pays each basket (scaling terms first, then flat; whole units), moves Time from the pop to the firm, and reserves it for production. Short till: never spend `stock_target`/`reserve_target`; wages may raid `growth_target`; profit share of yesterday `sold_amv - sold_cost_amv` is paid last (cut first). Partial pay withholds Time linearly in AMV. One pop, one employer; a firm may have several worker pops. Hours are Time units. Default `work_time_fraction` 0.5 caps claimed Time (stand-in until culture / class / religion / law supply that cap; prefer **caps** over a fixed grant). Settlement types live in `workforce.rs`. Time is **untradeable** (still transport 1.0). `pay_wage_shares` remains for the tester.

**Time** is good id 0 (transport 1.0, untradeable, decays 100%/day). Pops get 48 * household labor each morning via `Pop::start_day`. Every world process spends a little time. Jeweler and mint are one firm (jewelry + mint coin + idle melt). A production line starting from 0 snaps to at least 1 iteration. Coin AMV opens at 0.21. Coin decays 1%/day. AMV bounds stay on the row as planning guidestones but no longer skip, clamp, or void trades. Tester adds a one-household **Lord** pop (id 4) that owns every firm and wants jewelry. Firm tendering again uses the exchange slice of a sell-plan good (not a blanket ban when `sell_target` > 0). [`Firm::plan`] still rewrites production-line targets, then property buy/sell/use/stock/reserve and AMV. Sell/production grow only on strong sell success; profit is realized (sold vs cost), not made AMV. `Firm::record_keeping` snapshots rolling average and [`FirmRecords`] (including **confidence**), then calls `plan`. Tester CLI `day` calls firm `record_keeping` after consume (not a second pop `record_keeping`). Processes are loaded once with factuals (`Factuals::load_from_path`), not reloaded on each `run_production`. Pop `record_keeping` already rewrites shop/save; do not call it a second time for planning. **Production before pop consume** (so wages/payouts can use today's output, not only post-shopping stock). **Planning after consume** (and growth when wired). Vault `Turns.md` puts firm planning before consume; live intent splits the stub: produce, then consume, then plan. Own `amv_target` is nudged, not lerped onto live market AMV. Confidence scales how fast quotes and production move.

### World data

Split: **world data** (factuals, human-editable) vs **initialization data** (starting pops/firms/prices/markets) vs **save data** (later). Current load is world-data **goods, processes, and gameplay config** (`Factuals::load_from_path` on `data/world/` or a single TOML file; `goods.toml` + `processes.toml` + `config.toml`). Duplicate good or process ids error. Duplicate process input goods error. Missing config keys keep compiled defaults. Species, culture, and religion are still in-code. Init and saves are not started. Species/culture/religion load was explored then deferred. Processes and config are world data, not save data; mid-game edits are invalid.

Live paths with `&Factuals` read `factuals.config` (`GameConfig`): wage shares, deal AMV keep / high salability, exchange classification, transport fee, AMV bounce / salability default, AMV drift / salability blend, pop planning, **firm planning**, sentiments / SOL score, player-resource extract, create_orders priority bands, wash `buy_try_limit`, match coincidence weight, sell-success bonus, institution/state slot helpers (`priority_with`). Load rejects out-of-bound TOML and returns **every** bound failure in one `ConfigLoadError::Invalid` list. Compile-time `*_constants` remain the Default source and unit-test fallback. Buffer sizes (`HISTORY_MAX` / `AMV_HISTORY_MAX`) stay compile-time. Do not add a process-global OnceLock; tests run in parallel.

### FirmPRow / `run_production`

`FirmPRow` is the per-good firm ledger: stock (`quantity`, `reserve`, `rolling_average`), planning (`purchase_target`, `sell_target`, `use_target`, `stock_target`, `reserve_target`, `growth_target`, `amv_bound`), exchange (`average_cost`, `average_price`, `bought`/`bought_amv`, `sold`/`sold_amv`, `amv_target`, `margin`), production (`used`, `consumed`, `produced`). `new()` / `Default` / `with_*`. Helpers: `available()`, `sellable()`, `free_for_market()`, `purchase_qty()`, `mid_amv()`, `bid_amv()` / `ask_amv()` (dual rows: bid = mid * (1 - margin), ask = mid * (1 + margin)), `bought_unit_amv()`, `sold_unit_amv()`, plus labor fences `stock_fence()` / `wage_spendable()` / `profit_spendable()`. `growth_target` is read at labor settle; `Firm::plan` does not write it yet.

`FirmAmvBound` on the row is planning data (default `None`). `Minimum` = sell floor, `Maximum` = buy cap, `MinMax` = in-firm intermediate. [`Firm::plan`] writes residual WTP as the buy cap and consumed-input AMV rollup as the sell floor. Bounds do **not** gate trades: `create_orders` posts the row's own bid/ask even when market AMV is above the cap, and `form_buy_proposal` / `Firm::buy` still form a basket when payment AMV is above the bound or order `amv_target`. Deal evaluate does not read the bound. Keep ratio can still reject a lopsided basket. Later: headroom vs market for shrinking a line.

`Firm::run_production` records `produced` / `consumed` / `used` on those rows and returns `Vec<ProcessEffect>` (no `ProductionReport`). Destroyed and Consumed inputs both go to `consumed`; Consumed decay products go to `produced` on the result good; capital goes to `used` only; factors are untouched. Output `average_cost` blends this run's input AMV (split by each output's share of produced AMV). Used capital is **not** in that blend yet. Later: capital cost / maintenance / amortization so tools wear and are not indestructible; not v0. `reserve` is a stockpile guarantee: `sync_reserve` sets `min(quantity, reserve_target)` after quantity changes. `sellable` = `quantity - max(reserve, reserve_target)`. `free_for_market` is sellable, plus stock/use fences when `use_target` > 0. `decay_goods` returns `used` then decays on-hand stock. `clear_day_flows` zeros produced/consumed/bought/sold (and AMV totals) and is meant for day start so totals stay visible overnight. Production still not wired into the playstate phase.

### Firm `create_orders`

Read-only (`&self`). Signature: `create_orders(&self, history: &MarketHistory, factuals: &Factuals, unavailable: &HashSet<usize>) -> Vec<MarketOrder>`. Mechanical emitter: it honors current row targets and stock; it does **not** replan. [`Firm::plan`] writes the targets. Skips buys for goods in `unavailable` (market-day unmatched).

Posted buy/sell/offer **good** amounts are **whole units**. Named counters ceil to the next whole payment unit. Bid/ask AMV stays fractional (AMV is not a good). Inventory may still hold fractions.

On-hand free stock (`FirmPRow::free_for_market`) is classified as **sell**, **exchange**, and/or **liquidate**:

- **Exchange** if salability >= `EXCHANGE_SALABILITY_MIN` (`0.6`). Leftover with no purchase/sell/use and high salability is till money, not a dump.
- **Sell** if `sell_target` > 0. No salability cap (`SELL_SALABILITY_MAX` was dropped).
- **Both:** salability lerps the free pile from 90% sell / 10% exchange at 0.6 to 10% sell / 90% exchange at 1.0 (`SELL_EXCHANGE_EDGE` = 0.1). Exchange units round half-up to whole numbers; sell is the remainder, then capped at `sell_target` (overflow stays exchange).
- **Liquidate** if free stock, no purchase/sell/use, and salability below the exchange floor. Begrudging barter. Always **offer** orders, never priced sells, even when the firm has a money good.

Dual buy+sell: producer inputs (`use_target` > 0) buy only the stock-target shortfall and sell only free excess. Merchants (no `use_target`) emit the full `purchase_target` even above stock. Buy is incoming, not an on-hand role, so a row may buy and sell the same good.

Budget is optimistic: exchange AMV + expected sell AMV + liquidate AMV. Last buy may overdraw. No spendable AMV -> no buys (empty-till miller). Non-positive AMV is not spendable and is not a legal counter (falls through to the next tender, or to request/offer). Production inputs sort before merchant restock. Sell orders name a barter shortcut: the most valuable process input the firm still needs, else the market's most salable money good (salability at or above the exchange floor), even if not on-hand; tied money salability prefers lower id. Input-free producers (mine, well) therefore ask for coin. Buy orders name an on-hand exchange good. No counter -> request/offer.

Merchant-like (any row with purchase and sell, no use) sets order priority `FIRM_MERCHANT`; otherwise `FIRM_PRODUCER`. Market does not wealth-rank firms yet. Matching does **not** use AMV; bid/ask are written on the order for later settlement. `amv_bound` is a planning guidestone only; posted bid/ask are the row's own quote, and a market AMV above the buy cap does not skip the buy. Does not compute residual WTP. Do not add AMV into matching unless asked (later idea: weight vs average AMV, not total).

`next_shopping_trip` is still a stub. After a buy fills, the caller must raise reserve toward stock target before re-calling `create_orders`, or merchants will immediately re-offer what they just bought. After each accepted deal, leftover sell/offer amounts for both parties are clamped to current on-hand so a tender of a listed good cannot overdraw a later sell of the same stock.

Tunables live in `factuals.config.market` / `market_priority` (defaults from `config::market_constants` / `market_priority`). Tests: `firm::create_orders_should`.

### Firm `plan`

Public rewrite, separate from `run_production`. Signature: `plan(&mut self, factuals: &Factuals, history: &MarketHistory)`.

`Firm::plan` is gather then adjust. Pace is `plan_pace`: mid **confidence** (0.5) uses `planning_lerp_rate`; 0 is half speed, 1 is 1.5x. Daily volume/price pressures add then clamp to one `growth_rate` / `shrink_rate` step (also scaled by confidence).

1. **Gather** (`gather_plan_info`): line **productivity** (process AMV-out / AMV-in, for peer ranking); per output **realized profit** (sold unit AMV / average cost), sell success, stockpile vs `output_cover` days of planned output, decay, own vs market AMV; optional market share (`sold / purchased`), volume, AMV-trail volatility/trend. Competitor quotes are `None` until other firms are passed in.
2. **Adjust** (`apply_plan_adjustments`): from a quiet baseline, nudge sell plan and own quote. **Do not grow sell volume from high profit unless sell success is at least `sell_success_grow`.** Do not grow production toward a larger sell plan unless that same strong-demand gate holds. Then equalize peer lines (productivity) and align total output to the sell plan (more productive grows more; less productive shrinks more, using `shrink_rate`). A line at 0 that is starting snaps to at least 1 iteration. Cold-start keeps the line and the sell plan (no leftover dump). Missing inputs do not shrink. `target: None` stays None.
3. **Rollup** (`rewrite_property_targets`): input use/stock/purchase/reserve, AMV bounds, merchant restock. Does not overwrite output sell/AMV already written in adjust.

`MarketHistory` now also snapshots `purchased` and `amv_trails` when taken from a live `Market`.
4. Merchant-only rows (purchase+sell, no recipe use/make) restock what sold and keep `amv_bound` None. Till / barter rows with no recipe role are left alone.

`record_keeping(factuals, history)` snapshots `rolling_average`, writes [`FirmRecords`] (sold/bought AMV, realized profit, sell success, confidence), then calls `plan`. PlayState record-keeping and tester `day` both call it. Do not also call `plan` on the same day until snapshot work is split out.

Tunables: `factuals.config.firm` (`firm_constants`), including `sell_success_grow` / `shrink`, `output_cover`, `shrink_rate`, `confidence_*`. Tests: `firm::plan_should`.

### MarketGood

`MarketGood` has a real `Default` (AMV `1.0`, salability `SALABILITY_DEFAULT` `0.4`, average price `1.0`, empty `amv_history`, rest `0`) plus `new()` / `with_*` / `set_*`. `0.4` is below `EXCHANGE_SALABILITY_MIN` (`0.6`), so a new or unrecorded good is **not** till money. `amv_history` is a `CircularBuffer` of `AMV_HISTORY_MAX` (16) closes: seeded with opening AMV on the first `run_market_day`, then one close after salability. `record_amv` pushes; `amv_trail` returns oldest-to-newest. Intra-day `set_amv` does not push.

Setter invariants (`src/game/market.rs`, tunables in `config::market_constants`):

- **AMV / average_price:** never `0`. Values with `|x| < AMV_MIN_ABS` (`0.00001`) bounce past 0 from the previous sign (positive -> slightly negative, and vice versa).
- **Salability:** clamp to `0.0..=1.0`.
- **Non-negative** (`debug_assert`): production, consumption, stock, supply, suppliers, demand, buyers, requests, purchased, tender, payment.
- **Volume** is derived: `purchased + payment` (`MarketGood::volume()`). Not stored.
- **Imported** may be negative (exports).

Fields are still `pub`; day logic should go through setters. `Market::history()` snapshots **AMV and salability**. Missing salability on a `MarketHistory` defaults to `SALABILITY_DEFAULT` (`0.4`). Missing prices still default to `1.0`.

### Order priority

Full note: `docs/proposals/market-order-priority.md`. Vocabulary: **order priority**.

`MarketOrder.priority` is used **two ways**:

| Side | Meaning | Direction |
|------|---------|-----------|
| Buy / request | FCFS sort key | **Lower goes first.** RNG only among equal values. |
| Sell / offer | Selection **weight** | **Higher is more likely.** |

Buy-side bands (pops `[4, 5)`, firms `[2, 3)`) are `debug_assert`ed only on **buys**. Sells only need `priority > 0`. `assert_priority_for_origin` is `cfg(debug_assertions)` (release stub).

Buy-side named slots live in `config::market_priority` (`StateMarketSlot`, `MarketSlot::priority` for institutions `1` / `3` / `5`). There is **no** state-among-pops slot. State firm inserts sit at `band_end - STATE_FIRM_SLOT_MARGIN` (`2.49`, `2.99`). Firm rank helpers lerp toward those slots and never reach them.

Wealth rank for pop buys: **per household**, **total AMV** (`property_wealth_amv / household count`), not liquid. `unit_rank = 1 - wealth / max_wealth` (richest -> `0` -> band start). `run_market_day` **writes** `[4, 5)` when it collects pop orders. `Pop::create_orders` still writes `POP_START` (`4.0`) as an unranked placeholder. Offers are not generated yet.

Sell-side compose (write on create, then update after fills):

```text
compose_sell_priority(actor_band, supply, successful_sells)
  = 1 / max(actor_band, SELL_ACTOR_PRIORITY_FLOOR)
    + sqrt(supply)
    + SUCCESSFUL_SELL_BONUS * fills
```

Floor is `0.01` (so `STATE_FIRST` `0.0` is defined). Successful-sell bonus is `0.25`, added with `MarketOrder::add_successful_sell_bonus` after a fill (flat, not recomputed as a product). Marketing adds later.

### Matching (`Market::match_orders`)

One pass, **does not mutate** the books. Caller owns remove / update / reinsert.

- `buys` sorted by buy priority (lowest first). `sells` sorted by target good id.
- Only the **front** buy-priority group is considered (shuffled). Later groups wait for the next call so they cannot jump the queue.
- At most **one** `matched` pair (weighted sell of that good). Coincidence: if both orders have `Some` counter-offer and the goods match, that sell's weight is doubled **for this pick only** (`SELL_COINCIDENCE_WEIGHT = 2.0`). Request/offer with no counters do not get it.
- Self-trade skipped. No other-origin seller of that good -> `unmatched_buys` (may be **several** in the front group). Caller updates/drops those while the one deal runs.
- Matchable leftovers in the same group stay in the book (not failed).
- Empty buy book -> empty batch (`is_empty()`).
- RNG: `rand` `0.9`, `&mut impl Rng`.

Return: `OrderMatchBatch { matched: Option<OrderMatch>, unmatched_buys: Vec<usize> }`.

### Deal making (`DealMaker`)

Trait + types in `src/game/deal.rs`. `buy` / `evaluate` / default identity `sell` are **read-only**. `finalize` applies an accepted basket to inventory (seller adds the map, buyer subtracts it) and does not edit orders.

`ProposedDeal.goods` is the **seller's inventory change**: seller adds the map, buyer subtracts it. Negative qty = sold good; positive = tender.

`buy` (Pop, Firm): ranks the seller's named counter first (any salability), then other live tenders by salability (pop: excess above `shop_target`; firm: `free_for_market` minus units `create_orders` would sell or liquidate). `take_tenders` fills remaining targeted units from those preferred goods plus anything at or above `HIGH_SALABILITY` (`0.8`). Goods below that floor are only added if preferred tenders cannot cover. If everything is still short, targeted units shrink. Fill and payment **goods** are **whole units** (including a transport-tagged good in the deal map): a fractional shortfall is dropped, and payment ceils the AMV (or named-counter) cost of the largest whole fill on-hand can cover (2.5 AMV of value for 1 unit is paid as 3 coins). AMV itself stays fractional. The wagon bill (`transport_needed` / `pay_transport`) may be fractional and may spend a fraction of cargo. `None` if no tender, if targets differ / self-trade, or if a whole unit cannot be bought. Payment AMV above the buyer's `amv_target` or row buy cap does **not** void the basket. Buyer's named counter is no longer a special slot (it sits in live tenders by salability). **Make change** (seller returning excess) is reserved and unused. A sell-plan good can still tender its exchange slice; mid-day salability reclassify vs the morning sell order can overdraw (later: freeze the morning split).

`evaluate`: AMV **keep** = received AMV / given AMV. Given goods are full AMV. Received goods the actor will use (pop shop/desire, firm `use_target`) skip salability; others are `AMV * salability`. Pop min keep `0.25` (75% max loss). Firm min keep `0.50`, with a need-catch to `0.25` when a received good has `purchase_target` or `use_target`. Merchant restock is a need, not a use, so it still takes the haircut. Buyers accept windfalls (`keep >= 1.0`). First pass returns `Accept` or `Reject` only (`AcceptWithChange` / `Counteroffer` / `HardReject` exist unused).

`finalize` (Pop, Firm): quantity follows the signed map. Pop is quantity-only. Firm also records buyer `bought` / `bought_amv`, seller `sold` / `sold_amv`, blends `average_cost` at market AMV on inflows, and `sync_reserve`. Does not raise reserve toward stock target.

Tunables: `config::deal_constants`.

### Whole units

**Preferred term:** whole units / whole-unit exchange (`docs/design-vocabulary.md`).

Market orders and `ProposedDeal.goods` only move whole units of **goods** (including a transport-tagged good being bought, sold, or tendered). Inventory may still hold fractions (decay, consume, leftover crumbs). A shortfall below 1 does not post. Payment ceils the AMV (or named-counter) cost of the largest whole fill on-hand can cover (2.5 AMV of value for 1 unit is paid as 3 coins). Helpers: `util::whole_units` (trunc toward 0), `util::whole_units_up` (away from 0). Leftover order amounts snap to whole units.

**Not whole-unit:** AMV (bid, ask, `amv_target`, keep, payment AMV) is not a good. The wagon bill (`transport_needed` / `pay_transport`) may be fractional and may spend a fraction of cargo.

### Take good

`Pop::take_good` / `Firm::take_good` remove that good's property row and return on-hand quantity (`0` if missing), including leftover fractions that cannot be posted.

### Market day (`Market::run_market_day`)

Signature: `run_market_day(&mut self, factuals, pops: &mut HashMap<usize, Pop>, firms: &mut HashMap<usize, Firm>, rng)`. RNG is required. HashMaps are keyed by actor id; only ids in `self.pops` / `self.firms` are collected. Institutions and states are skipped.

1. **Collect.** `Pop::create_orders` / `Firm::create_orders`. Pop buy/request order priority is **written** from per-household total AMV (`property_wealth_amv / household count` vs market max).
2. **Collate.** Opening `supply` / `demand` / `buyers` / `suppliers` on `MarketGood`. Day exchange counters are zeroed first (not AMV, salability, average price, stock, production, consumption, imports).
3. **Loop** until `match_orders` returns empty (no buys left):
   - One matched pair plus hopeless front-group buys.
   - Hopeless buys: insert the good on `Market.unavailable_goods`. Not a meeting; no transport fee; no renew. `create_orders(..., unavailable)` skips those goods.
   - Matched pair: buyer `buy` (`with_transport_budget` caps fill to a whole unit, then forms the basket so post-exchange cover pays `TRANSACTION_COST + bulk * friction`; efficiency is on `GoodTag::Transport`). Seller `evaluate`. Accept -> `finalize` both, then buyer `pay_transport(transport_needed)` after the map. Leftover order amounts stay whole units. Reject / no proposal -> **wash**: `pay_transport(TRANSACTION_COST)` from on-hand, then `renew_buy` until `BUY_TRY_LIMIT` 2. Worlds with no Transport tag skip the bill.
   - New orders after a fill (`next_shopping_trip`, firm re-emit) are **not** added.
4. **Cleanup.** Clear member pops' `current_orders`. AMV already drifted on `MarketGood` during meetings, then leftover/unmatched books pull AMV (buys up, sells down, larger leftover wins, scaled by leftover vs purchased). Salability lerps toward payment/tender. Empty AMV rings are seeded with the opening AMV at day start; each good's close is pushed after salability. Frozen `MarketHistory` for this day stays the opening snapshot.

Lookups go through `as_deal_maker` / `as_deal_maker_mut` (`&dyn DealMaker`). Member pop/firm ids are `expect`ed present.

PlayState `phase_intra_market_day` is still `todo!()`. Tester `match` is still read-only. Tester `day` settles via `run_market_day`.

### Market tester CLI

Do not add pages, commands, or extra CSV series unless asked. `day` now runs firm `record_keeping` / `plan`.

`cargo run --example market_tester` (`examples/market_tester.rs`). Small living roster. **Home** is a short summary (goods AMV/sal, actor names, book counts, CSV path). Pages: **`stock`** (on-hand + firm AMV bounds and quotes), **`orders`** (buy/sell books), **`processes`** (world recipes + firm records and lines), **`amv`** (trail), **`day`** (full report). `home` / `cls` returns to the summary. `shop` reloads books from `create_orders`. `match` is a read-only matcher pass.

Each **`day`** appends one-row-per-day CSVs under `data/logs/` (gitignored `*.csv`). Default stem `prices`. `csv` shows paths; `csv <name>` changes the stem (same folder); `csv reset` wipes and rewrites headers. Header mismatch (old layout) errors until reset or a new stem.

| File | Layout |
|------|--------|
| `{stem}_market.csv` | One row/day. Per good: `amv`, `salability`, `average_price` (qty-weighted fill price of buys of that good today; blank if no fills). No volume / purchased / payment. |
| `{stem}_firms.csv` | One row/day. Per firm: `confidence`, `profit`, `sell_success`. Then `{firm}_{good}_{field}`: `quantity`, `sell_target`, `amv_target`, `bid`, `ask`, `average_cost`, `average_price`, `sold`, `produced`. Header mismatch (old layout) needs `csv reset` or a new stem. |
| `{stem}_trades.csv` | One row/day. Per good candles: `open`, `high`, `low`, `close`, `volume` (units bought as the sought good; OHLC blank and volume 0 if none). |

**`day`** / **`day N`** calendar loop:

1. [`Pop::start_day`] grants Time (`ScalingFactor::Labor` * 48). Zero `income_amv`, `initial_reservations_and_update_satisfaction`, firm `clear_day_flows`.
2. **Wages:** [`Firm::pay_wage_shares`] (`labor_constants`: living owners 30%, workers 30%, both ceil, owners first). Missing owners do not drain the till. A producer with no process inputs pays the **whole till**, split in that same owner:worker ratio. Mine and well now have Time as an input, so they keep the 40% till. Roster workers: farmers at farm+well, laborers at mine, townsfolk at bakery+jeweler. All five firms set `owners.owner` to the Lord pop (id 4). No wage bargaining. Still a share of on-hand coinage, not [`LaborSettlement::settle`].
3. [`Market::run_market_day`]. Time is untradeable transport (pops hold Time for friction; firms do not buy it). Production Time arrives only if labor settle ran; the tester does not call it yet, so firm recipes spend Time only if it is already on the firm.
4. [`Firm::run_production`] on world-data recipes from `data/world/processes.toml`. Farm: time 3 + 1 water -> 6 grain, target 5. Bakery: time 2 + 1 grain -> 1.2 bread, target 10. Mine: time 4 -> 1 gold, target 8. Jeweler: time 3 + 3 gold -> 5 jewelry (target 1) and time 2 + 1 gold -> 40 coin (target 1), plus idle melt time 2 + 41 coin -> 1 gold. Well: time 1 -> 1 water, target 40. Missing inputs throttle the run (`last_missing_goods`). Coin decays 1%/day; Time decays 100%/day. A line starting from 0 snaps to 1 iteration.
5. Pop `consume`, `update_sentiments`, `record_keeping`, cap coin `save_target` / `shop_target` at 1 unit.
6. Firm `record_keeping` (rolling average, `FirmRecords`, `Firm::plan`) from the closing `MarketHistory`.
7. Pop/firm `decay_goods`.

Prints a [`MarketDayReport`] plus wages, production (did today / want next day), firm plans (confidence, realized profit, sell success, sell/quote), post-consume pop tier sat / SOL / shop_fill / income, and the AMV trail. `day N` adds a one-line digest per day (includes mean firm confidence) and the last day's full report. Books reload from current stock after the loop. Time is transport-tagged; the wagon bill can spend pop Time.

Checked 2026-09-06: coin is 10x units at AMV 0.21 (0.1 * 2.1 so gold/coin ~ 38, near the 40-coin mint recipe) and decays 1%/day. Working pops want grain/water/bread; Lord also wants jewelry. Grain decays 10%/day, bread 20%. Farm grain quote 1.2 / water bid 0.45 (bound still on the row, no longer a trade gate); well ask 0.20 (floor 0.15), draws 40/day. Water market AMV 0.20. Jeweler starts with 8 gold (jewelry + mint) and 300 coin. `match` still finds a deal without moving stock.

- Production lines point at process ids from `data/world/processes.toml`. Bounds are hand-set on the roster, not computed. Time is on every recipe. Labor settle exists in lib tests, not in this CLI loop.
- Working pops share the same desire spread set outright (not from demographics): basic **food** (grain 1.0 or bread 1.5) + water, common bread. Lord (1 household, ~900 coin) has small staples plus luxury jewelry (~2). They emit **requests** only.
- No merchants. Firm default hand-typed buy priority is `FIRM_PRODUCER` (`2.5`).
- TTY: home is the summary; pages replace the screen (`home` back). Piped stdout prints home, then each command's log.

**Goods (id / AMV / sal):** 0 time 1.0 / 0.4 (transport 1.0, **untradeable**, decay 1.0), 1 grain 1.0 / 0.5, 2 water 0.20 / 0.35, 3 bread 2.2 / 0.45, 4 gold 8.0 / 0.7, 5 coin 0.21 / 1.0, 6 jewelry 15.0 / 0.8.

**Roster (intended roles, not live amounts):**

| actor | buying | selling |
|-------|--------|---------|
| farmers | water, bread (food) | - |
| laborers | grain, water, bread (food) | - |
| townsfolk | grain, water, bread (food) | - |
| lord | grain, water, bread (food), jewelry | - |
| farm | water | grain |
| bakery | grain | bread |
| mine | - | gold |
| jeweler | gold | jewelry, coin (melt 41 coin -> gold is idle) |
| well | - | water |

**Not built in the tester:** `LaborSettlement::settle` (still `pay_wage_shares`), hiring/creation, `next_shopping_trip`, cargo goods on this roster. Time is on recipes and granted to pops; it is not a market good. After `day`, AMV, salability, shop targets, and the AMV ring update. `main.rs` is still the Bevy hex stub. The lib loop is `Market::run_market_day`; the tester wraps it in the calendar shortcuts.

### Pop economic day (still closed through record keeping)

Unchanged from 2026-08-18 in substance on shop/save. `Pop::record_keeping` snapshots then rewrites next-day shop/save. Morning `update_desires` does **not** multiply shop/save. Consume need, days-of-buffer savings, reserved never negative, `create_orders` three passes (desire shop, parked non-desire shop, opportunistic extra). Request amounts are whole units; a shortfall below 1 is skipped.

Shop ambition does **not** scale with wealth or shop fill. `Desire.amount` is fixed (tester working pops: 8 food / 6 water / 4 bread; Lord: 1 food / 1 water / 1 bread plus 2 jewelry). Food is grain at 1.0 or bread at 1.5. Next-day `shop_target` is consume need (max of unsatisfied leftover and consumed/used) plus save. Planning only lerps savings ratio / time preference / risk appetite. Tester then clamps coin save to 1 unit. Luxury is the designed leftover-budget ladder; `create_orders` does not loop extra staple buys.

`DemoDesire::create_desire` (the only demo-to-pop path; `derive_desire` was folded in) scales `amount` **and** additive effects (player resources, bonus goods) by `get_scaling_factor`. Birth, mortality, sentiment, and satisfaction arms stay as demo rates. `update_desires` rewrites existing desire effects from the parent demo the same way. Harvest is sat times that baked magnitude; do not multiply by household count again.

`extract_special_resources` first pass exists: demographic rates (species / culture / **religion** via `find_religion`), living-well culture, SOL/mood legitimacy (`FIRST + EXTRA * (n - 1)` over all desire tiers), desire effects, then drain stored player-resource arms. Playstate harvests but does **not** route yield into the owning state's pool. `LUXURY_LEGITIMACY_RATE` is unused.

### Turn wiring

- `phase_intra_market_day` is still `todo!()`. `Market::run_market_day` is a lib function, not wired into PlayState.
- Sentiments after growth, before migration; `MarketLookups` rebuilt at sentiments and record keeping.
- `extract_special_resources` phase is wired (yield discarded).

### Language

- Do **not** use **knob** / **lever** except player-facing UI. Prefer **planning variable**, **tunable**, **constant**.
- Prefer **consume need** over consume-half.
- **Order priority** vs **desire priority** — never say bare "priority" in design talk.
- **Write / set** a field on create; **update / edit** an order in the books. **Stamp** only for a completed deal.
- Function comments: **what it does first**, why second. Knowing the operation often explains why it exists.

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
| **Matching** | One success per pass, front group only. Multiple hopeless buys OK. Do not batch several deals. Later **multimatch** (TODO): same buyer+seller extra goods as one trip; do not start unless asked. |
| **Firm create_orders** | Mechanical. Do not replan (no success-rate / "dump vs mill" logic here). Planning writes the targets. |
| **Firm plan** | `Firm::plan` rewrites line and property targets. Grow sell/production only on strong sell success. Profit is realized (sold vs cost); line peer rank is process productivity. Missing inputs do not shrink the line. Confidence scales lerp/step. Do not call pop `record_keeping` twice for planning. World processes load once with factuals; `run_production` does not reload TOML. Production before consume (wage-from-output). Own quote, not lerp-to-market. |
| **Matching AMV** | Not used yet. Do not add AMV into `match_orders` unless asked. |
| **Deal AMV keep** | Pop min keep `0.25`, firm `0.50`, firm-need catch `0.25`. Use-goods skip salability; other received goods * salability. Buyers accept windfalls. `buy` / `evaluate` do not mutate. `finalize` does. |
| **Market day wash** | Reject or no proposal keeps the sell and charges `TRANSACTION_COST` transport from on-hand. Buyer `renew_buy` may put the buy back (`BUY_TRY_LIMIT` 2). Unmatched = unavailable, no fee, no renew. |
| **Transport / friction** | `GoodTag::Transport(efficiency)` (1.0 = time). Cover is `qty * efficiency`. Success bill is `TRANSACTION_COST + bulk * market.friction`. Cap fill then form the basket. Wash is the flat fee only. Unavailable goods live on the **market**. No Transport tag => bill 0. Bill and spend may be fractional. |
| **Whole units** | Orders and deal-map goods only. AMV stays fractional. Wagon bill stays fractional. Transport *goods in the deal map* are still whole units. |
| **Take good** | Removes the property row, returns quantity (0 if missing). |
| **Buy tenders** | Seller's named counter first (any salability), then live tenders by salability. `take_tenders` covers remaining units from that preferred set plus `HIGH_SALABILITY` (`0.8`). Low-sal only if those cannot cover. Shrink fill only after all tenders. **Make change** is returning excess, not this helper. |
| **Desire effect bake** | Additive arms (player resources, bonus goods) bake in `create_desire` / `update_desires`. Harvest does not multiply by count. |
| **Salability default** | `SALABILITY_DEFAULT` is `0.4` (new goods and missing history). Below exchange floor: not till money. Day-end lerp toward `payment / tender` when tender > 0. |
| **AMV drift** | Write live `MarketGood.amv`. Intra-day `buy` / `evaluate` / orders use frozen `history()`. Accept: both sides lerp toward basket midpoint. Reject: sought * 1.1 edge up, tenders down by units offered per unit sought. No-proposal: sought up only. End of day: leftover/unmatched buys raise AMV, leftover sells lower it, larger leftover wins, scaled by unsatisfied / (unsatisfied + purchased). |
| **AMV history** | `MarketGood.amv_history` ring (`AMV_HISTORY_MAX` 16). Seed opening AMV if empty at day start; push close after salability. Intra-day `set_amv` does not push. Tester `day` / `amv` print the trail. |
| **Counter AMV** | Firm counters and spendable skip non-positive prices (AMV bounce can go slightly negative). |
| **Vocabulary** | Prefer `docs/design-vocabulary.md` over chat shorthand |
| **Comments** | ASCII only; **add, do not edit or replace** existing comments unless asked. New function comments: **what first**, why second |
| **Write / set** | Fill a field on create. **Update / edit** in the books. **Stamp** only a completed deal |
| **Knob** | Player-facing only |
| **Labor** | Roster / `Workforce`, not goods-book labor. No labor-time goods. Time is untradeable. |
| **Work hours** | `work_time_fraction` is a **cap**. Later: culture / class / religion / law. Prefer caps so hours can still move. |

---

## 5. Known debt / next-friendly work

### Natural next system

- **Labor operate wiring** — `LaborSettlement::settle` exists; tester and PlayState still use `pay_wage_shares` / stubs. Hiring/creation skipped on purpose. Work-hours cap from culture / class / religion / law (global fraction is the stand-in). Skill copy / experience at production, make-change on wages, pop payment preferences, and `growth_target` from `plan` are later.
- **Wire PlayState** `phase_intra_market_day` to `Market::run_market_day` (needs an RNG on play state or the phase). Institution / state orders still missing.
- **Leftover book carry** — leftover buys/sells are reported then dropped; next day recasts from `create_orders`.
- **Tester CLI** — `day` runs firm `record_keeping` / `plan` after consume. Home + `stock` (quotes) / `orders` / `processes` (records + lines) / `day` / CSV logs. Do not invent a second shopping model. Do not add pages unless asked.
- **Pop shop ambition** — not started. Staple desire amounts stay fixed; leftover coin does not buy more grain/bread. Vault wants looping luxury (and culture-grown common) when they succeed. Tester Lord has a jewelry luxury desire; working pops still have no luxury.
- **New orders after a fill** — `Pop::next_shopping_trip` (still `todo!()`), firm re-emit. After a buy fills, raise reserve toward stock target before re-calling `create_orders`, or merchants dump what they just bought.
- **Offer generation** — pops still only emit requests.
- **`sell` rewrite / haggling** — identity `sell`; Accept/Reject only.
- **AMV drift** — landed on accept (midpoint lerp) and reject (sought up, tenders down). Salability day-end from payment/tender. AMV history ring landed (open seed + daily close).

### Nearby leftovers (do not start unless asked)

- **Multimatch** — after the first match, same buyer + same seller, other goods at similar buy priority, one `ProposedDeal`. Variety sellers load the cart. Live matcher stays one pair. See `TODO.md`.
- Function comments repo-wide: lead with **what** the function does. Deal/bound helpers were rewritten; rest of `src/game/` is on `TODO.md`.
- Firm `amv_bound` is written by `Firm::plan` (residual WTP / input-cost rollup). Default `None` until `plan` runs. Not applied as a trade gate; later use as headroom vs market for shrinking a line.
- Institution / market / state `record_keeping` bodies still `todo!()`. Firm `record_keeping` is rolling average + `plan`.
- Firm `apply_passive_bonuses` is a stub; region/market bonus apply is unchecked
- `run_production` exists + tests; tester calendar calls it after the market. PlayState production phase still `todo!()`
- `Firm::create_orders` exists + tests; used by `run_market_day`, **not** wired into PlayState
- `Firm::plan` exists + tests; `record_keeping` snapshots rolling average and `FirmRecords` then calls `plan`. Tester `day` and PlayState record-keeping both call it.
- `Pop::start_day` exists; tester `day` grants Time with it. PlayState day-start phase still stub (TODO: "Completed not Connected")
- Migration orchestrator exists; leaves are `todo!()` (wants live sentiment + liquid wealth)
- Class demographics unimplemented (vault: park this)
- `income_amv` is zeroed at tester day start; PlayState still does not
- Player-resource yield not routed onto `State.resources`
- Optional later: spread firm overbuying across other goods (currently optimistic full `purchase_target`)
- Optional later: AMV as a *relative-to-average* sell-weight tweak in matching (not a hard filter; not total AMV)
- Optional later: day-fill rate cache if `get_demographic_rates` shows up at huge pop counts
- Optional later: `Pop.market_id` (update on migrate) instead of rebuilding `pop_to_market`
- Optional later: pace luxury consume so one desire does not empty leftover stock (see `TODO.md`)
- Optional later: marketing add on sell weight; recompute `sqrt(supply)` after partial fill; merchant vs producer firm flag; subordinated-firm priority; state purchase buckets
- Optional later: capital cost / maintenance / amortization into output `average_cost` (used capital currently returns whole; tools should wear)

### Comments still stale (fix only if the user asks)

- PlayState record-keeping phase comment still says the only shared input is factuals (pops and firms also take market history)
- `Pop::update_desires` rustdoc still lists step 3 as scaling `shop_target` / `desire_needs` for growth (that block is gone)
- `PopRecords.savings_ratio` field still says "share of liquid wealth"
- `decay_goods` still calls `saved` a wish target in one place
- Original short `record_keeping` docblock was left as-is
- Playstate / firm / institution docs may still mention `Pop::demographic_update`
- `TODO.md` household-helper bullet lags the landed rates model
- Vault `Pops.md` household section still has a REWORK banner; morning step 3.5 still says resize shopping targets (record keeping owns that now)
- `compose_sell_priority` formula comments in the proposal may lag live `SELL_*` constants — prefer the constants
- `Firm::pay_wage_shares` / `create_orders` rustdoc still links `labor_constants` / `market_constants` (compiled defaults; live reads `factuals.config`)
- `DealMaker::renew_buy` / `Market::match_orders` rustdoc still describe the const defaults; live market day uses `renew_buy_with_limit` and `match_orders_with_coincidence`

### Review log

Open review debt is empty. Second pass 2026-08-27 found no new code issues; handoff refreshed.

---

## 6. Where to look in code

| Concern | Location |
|---------|----------|
| Firm property + production flows | `src/game/firm.rs` → `FirmPRow`, `Firm::run_production`, `decay_goods`, `clear_day_flows` |
| Firm planning | `src/game/firm.rs` → `Firm::plan`, `Firm::record_keeping`, `FirmRecords`; tunables `factuals.config.firm` |
| Firm market orders | `src/game/firm.rs` → `Firm::create_orders`, `classify_on_hand`, `counter_good`, `Firm::take_good` (read-only emit; whole-unit amounts; skip `unavailable`) |
| Record keeping + planning + shop/save | `src/game/pop.rs` → `record_keeping`, `update_planning`, `rewrite_shop_and_save_targets`, `planning_growth_factor` |
| Cheapest tradeable basket | `src/game/pop.rs` → `cheapest_tradeable_cover` |
| Pop request orders | `src/game/pop.rs` → `create_orders` (plan, then parked shop, then extra desires; whole units; skip `unavailable`) |
| Labor settle | `src/game/workforce.rs` → `LaborSettlement::settle`, `Workforce`, `PaymentTerm`; thin `Firm::settle_labor_contracts` |
| Dump a property row | `Pop::take_good`, `Firm::take_good` |
| Whole-unit helpers | `src/game/util.rs` → `whole_units`, `whole_units_up`, `is_whole_unit` |
| Order type + buy/sell priority helpers | `src/game/marketorder.rs` |
| Matching | `src/game/market.rs` → `Market::match_orders`, `OrderMatchBatch` |
| Market day | `src/game/market.rs` → `Market::run_market_day` (collect, collate, match/deal/finalize loop; returns `MarketDayReport`) |
| Deal making | `src/game/deal.rs` → `DealMaker`, `ProposedDeal`; impls on `Pop` / `Firm` (`buy` / `evaluate` / `finalize`) |
| World goods / process / config load | `src/game/factuals.rs` → `Factuals::load_from_path` / `load_from_toml`; `data/world/goods.toml`, `data/world/processes.toml`, `data/world/config.toml` |
| Gameplay tunables | `src/game/config.rs` → `GameConfig` on `Factuals.config`; compiled defaults in `*_constants` |
| Market CLI | `examples/market_tester.rs` — **paused.** Home + `stock` / `orders` / `processes`; `day` / `day N`; CSVs in `data/logs/` |
| Day-end price CSVs | `examples/market_tester.rs` (`append_price_log`); `data/logs/{stem}_{market,firms,trades}.csv` |
| Wage share stand-in (tester) | `src/game/firm.rs` → `Firm::pay_wage_shares`, `WagePayout`; tunables `factuals.config.labor` (`work_time_fraction` too) |
| MarketGood setters / AMV bounce / AMV history | `src/game/market.rs` → `MarketGood`, `record_amv`, `amv_trail` |
| Order-priority tunables | `src/game/config.rs` → `market_priority`, `market_constants` |
| Priority design (deferred too) | `docs/proposals/market-order-priority.md` |
| Pop records / property rows | `src/game/pop_property.rs` |
| Planning tunables | `src/game/config.rs` → `pop_constants`, `firm_constants` |
| Market price snapshot | `src/game/market.rs` → `Market::history`, `MarketLookups` |
| Turn order / wires | `src/playstate.rs` → `advance_turn`, `phase_update_sentiments`, `phase_record_keeping`, `rebuild_market_lookups` |
| Household / rates math | `src/game/household.rs` |
| Rate resolve | `src/game/factuals.rs` → `get_demographic_rates` |
| Sentiment | `src/game/sentiment.rs`, `Pop::update_sentiments` |
| Desire create / effect bake | `src/game/desire.rs` → `DemoDesire::create_desire`, `scaled_effects` |
| Extract player resources | `src/game/pop.rs` → `extract_special_resources` |
| Desire targets | `src/game/desire.rs` → `DesireTarget` |
| Household design depth | `docs/proposals/household-population-refactor-primer.md` |

---

## 7. Suggested first steps for a new agent

1. Read `AGENTS.md` + this handoff + `docs/design-vocabulary.md` + `docs/proposals/market-order-priority.md`.
2. `cargo test --lib`.
3. Intramarket loop is **`Market::run_market_day`**. Tester calendar is `day` / `day N`: Time grant, `pay_wage_shares`, market, `run_production`, pop consume/records, firm `record_keeping` (`plan`), decay. `LaborSettlement::settle` is lib-only for now. `run_production` reads already-loaded factuals; do not reload `processes.toml`. Old `*_firms.csv` headers need `csv reset`.
4. Match `STYLE.md` on any edits; update `reviewlog.md` when doing reviews.
5. Prefer vault **EconCiv** notes for design intent when code and notes disagree — **call out conflicts** rather than silent invention. Vault `Turns.md` sequential shopping walk vs collect-and-match: **match** is the live model.

---

## 8. One-line status

**Pop economic day is closed through record keeping. `Market::run_market_day` is the live intramarket loop. `Firm::plan` rewrites production and property targets from realized profit, sell success, and confidence. Time is good 0 (untradeable, transport 1.0); pops get 48 per household labor at `start_day`. Labor contracts operate via `LaborSettlement::settle` (not wired into tester/PlayState; tester still `pay_wage_shares`). Tester CLI: compact home, `stock` / `orders` / `processes` pages, calendar `day` / `day N` (Time grant, wages 30/30, market, `run_production` from loaded processes, consume, pop and firm records / plan, decay), and one-row-per-day CSVs in `data/logs/`. World goods, processes, and config load from `data/world/`. Coin AMV opens at 0.21. Desire amounts do not rise with success. PlayState intramarket and production phases are not wired.**
