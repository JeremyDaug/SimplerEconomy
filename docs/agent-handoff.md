# Agent handoff — EconCiv rework

**Branch:** `EconCiv-Rework-Branch`  
**Handoff date:** 2026-09-03  
**Purpose:** Catch a new agent/session up on recent work and direction. Prefer this plus `AGENTS.md`, `STYLE.md`, `TODO.md`, `reviewlog.md`, and `docs/design-vocabulary.md` over inventing process from scratch.

**Build (as of this wrap-up):** `cargo test --lib` green (**355** tests). CLI smoke: `cargo run --example market_tester` then `shop` / `day` / `amv` / `match`.

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

## 3. What is true now (2026-09-03)

**Landed this wrap.** Tester `day` runs `Market::run_market_day` and prints a compact `MarketDayReport` (trades, grouped washes, leftovers, outcomes with live AMV/salability). AMV drifts as meetings resolve (accept pulls baskets together; reject raises the sought good and lowers tenders). Salability updates at day end from payment/tender. Intra-day `MarketHistory` stays frozen; the tester copies the new snapshot after `day`. `MarketGood.amv_history` records the opening AMV then each close; CLI `day` / `amv` / legend show the trail. World-data goods load from `data/world/goods.toml`.

### World data

Split: **world data** (factuals, human-editable) vs **initialization data** (starting pops/firms/prices/markets) vs **save data** (later). Current load is world-data **goods only** (`Factuals::load_from_path` / `load_from_toml`; tester uses `data/world/goods.toml` instead of hard-coded `Good` literals). Duplicate good ids error. Processes, species, culture, and religion are still in-code. Init and saves are not started. Species/culture/religion load was explored then deferred.

### FirmPRow / `run_production`

`FirmPRow` is the per-good firm ledger: stock (`quantity`, `reserve`, `rolling_average`), planning (`purchase_target`, `sell_target`, `use_target`, `stock_target`, `reserve_target`, `amv_bound`), exchange (`average_cost`, `average_price`, `bought`/`bought_amv`, `sold`/`sold_amv`, `amv_target`, `margin`), production (`used`, `consumed`, `produced`). `new()` / `Default` / `with_*`. Helpers: `available()`, `sellable()`, `free_for_market()`, `purchase_qty()`, `mid_amv()`, `bid_amv()` / `ask_amv()` (dual rows: bid = mid * (1 - margin), ask = mid * (1 + margin)), `bought_unit_amv()`, `sold_unit_amv()`.

`FirmAmvBound` on the row is planning data (default `None`). `Minimum` = sell floor, `Maximum` = buy cap, `MinMax` = in-firm intermediate. `create_orders` clamps bid/ask written on the order and skips buys when market AMV is above the cap. `form_buy_proposal` returns `None` if payment unit AMV is above the buyer's order `amv_target` (the clamped bid). Request orders with no `amv_target` still honor the row buy cap in `Firm::buy`. Planning does not compute the numbers yet; deal evaluate does not read the bound. Later: residual WTP / input-cost rollup, then headroom vs market for growth.

`Firm::run_production` records `produced` / `consumed` / `used` on those rows and returns `Vec<ProcessEffect>` (no `ProductionReport`). Destroyed and Consumed inputs both go to `consumed`; Consumed decay products go to `produced` on the result good; capital goes to `used` only; factors are untouched. Output `average_cost` blends this run's input AMV (split by each output's share of produced AMV). Used capital is **not** in that blend yet. Later: capital cost / maintenance / amortization so tools wear and are not indestructible; not v0. `reserve` is a stockpile guarantee: `sync_reserve` sets `min(quantity, reserve_target)` after quantity changes. `sellable` = `quantity - max(reserve, reserve_target)`. `free_for_market` is sellable, plus stock/use fences when `use_target` > 0. `decay_goods` returns `used` then decays on-hand stock. `clear_day_flows` zeros produced/consumed/bought/sold (and AMV totals) and is meant for day start so totals stay visible overnight. Production still not wired into the playstate phase.

### Firm `create_orders`

Read-only (`&self`). Signature: `create_orders(&self, history: &MarketHistory, factuals: &Factuals, unavailable: &HashSet<usize>) -> Vec<MarketOrder>`. Mechanical emitter: it honors current row targets and stock; it does **not** replan. Planning / record keeping still `todo!()`. Skips buys for goods in `unavailable` (market-day unmatched).

Posted buy/sell/offer **good** amounts are **whole units**. Named counters ceil to the next whole payment unit. Bid/ask AMV stays fractional (AMV is not a good). Inventory may still hold fractions.

On-hand free stock (`FirmPRow::free_for_market`) is classified as **sell**, **exchange**, and/or **liquidate**:

- **Exchange** if salability >= `EXCHANGE_SALABILITY_MIN` (`0.6`). Leftover with no purchase/sell/use and high salability is till money, not a dump.
- **Sell** if `sell_target` > 0. No salability cap (`SELL_SALABILITY_MAX` was dropped).
- **Both:** salability lerps the free pile from 90% sell / 10% exchange at 0.6 to 10% sell / 90% exchange at 1.0 (`SELL_EXCHANGE_EDGE` = 0.1). Exchange units round half-up to whole numbers; sell is the remainder, then capped at `sell_target` (overflow stays exchange).
- **Liquidate** if free stock, no purchase/sell/use, and salability below the exchange floor. Begrudging barter. Always **offer** orders, never priced sells, even when the firm has a money good.

Dual buy+sell: producer inputs (`use_target` > 0) buy only the stock-target shortfall and sell only free excess. Merchants (no `use_target`) emit the full `purchase_target` even above stock. Buy is incoming, not an on-hand role, so a row may buy and sell the same good.

Budget is optimistic: exchange AMV + expected sell AMV + liquidate AMV. Last buy may overdraw. No spendable AMV -> no buys (empty-till miller). Non-positive AMV is not spendable and is not a legal counter (falls through to the next tender, or to request/offer). Production inputs sort before merchant restock. Counter-offer is the highest-salability exchange good that is not the target. No counter -> request/offer.

Merchant-like (any row with purchase and sell, no use) sets order priority `FIRM_MERCHANT`; otherwise `FIRM_PRODUCER`. Market does not wealth-rank firms yet. Matching does **not** use AMV; bid/ask are written on the order for later settlement. `amv_bound` clamps that AMV (bid never above the buy cap, ask never below the sell floor) and skips a buy/request when market AMV is already above the cap. The clamped bid is also a unit-AMV ceiling at proposal time. `None` is unchanged. Does not compute residual WTP. Do not add AMV into matching unless asked (later idea: weight vs average AMV, not total).

`next_shopping_trip` is still a stub. After a buy fills, the caller must raise reserve toward stock target before re-calling `create_orders`, or merchants will immediately re-offer what they just bought.

Tunables live in `config::market_constants`. Tests: `firm::create_orders_should`.

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
    + SELL_SUCCESS_BONUS * fills
```

Floor is `0.01` (so `STATE_FIRST` `0.0` is defined). Success bonus is `0.25`, added with `MarketOrder::add_sell_success_bonus` after a fill (flat, not recomputed as a product). Marketing adds later.

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

`buy` (Pop, Firm): ranks the seller's named counter first (any salability), then other live tenders by salability (pop: excess above `shop_target`; firm: `free_for_market` minus units `create_orders` would sell or liquidate). `take_tenders` fills remaining targeted units from those preferred goods plus anything at or above `HIGH_SALABILITY` (`0.8`). Goods below that floor are only added if preferred tenders cannot cover. If everything is still short, targeted units shrink. Fill and payment **goods** are **whole units** (including a transport-tagged good in the deal map): a fractional shortfall is dropped, and payment ceils the AMV (or named-counter) cost of the largest whole fill on-hand can cover (2.5 AMV of value for 1 unit is paid as 3 coins). AMV itself stays fractional. The wagon bill (`transport_needed` / `pay_transport`) may be fractional and may spend a fraction of cargo. `None` if no tender, if targets differ / self-trade, if a whole unit cannot be bought, or if payment unit AMV is above the buyer's `amv_target` (or the firm row buy cap on a request). Buyer's named counter is no longer a special slot (it sits in live tenders by salability). **Make change** (seller returning excess) is reserved and unused.

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
4. **Cleanup.** Clear member pops' `current_orders`. AMV already drifted on `MarketGood` during meetings. Salability lerps toward payment/tender. Empty AMV rings are seeded with the opening AMV at day start; each good's close is pushed after salability. Frozen `MarketHistory` for this day stays the opening snapshot.

Lookups go through `as_deal_maker` / `as_deal_maker_mut` (`&dyn DealMaker`). Member pop/firm ids are `expect`ed present.

PlayState `phase_intra_market_day` is still `todo!()`. Tester `match` is still read-only. Tester `day` settles via `run_market_day`.

### Market tester CLI

`cargo run --example market_tester` (`examples/market_tester.rs`). Small living roster. `shop` loads books from `create_orders`. `match` is a read-only matcher pass. **`day`** runs [`Market::run_market_day`] on the current pops/firms and prints a [`MarketDayReport`]: unmatched buys (no seller), each meeting (trade basket + transport, or wash reason / renewed / closed), leftover books, `MarketGood` outcomes (demand, supply, purchased, payment, volume, AMV, salability), and an AMV trail (old -> new plus today's delta). **`amv`** reprints that trail. Legend goods also show the trail. On-screen books are cleared after a day; `shop` reloads from post-day stock. This roster has no transport-tagged goods, so the wagon bill is 0.

Checked 2026-09-03: startup `shop` loads **10 pop + 9 firm orders** (13 buys, 6 sells). `day` settles (seen: bakery grain vs farm; mint gold vs mine; pop requests vs well/bakery/jeweler). Washes close after `BUY_TRY_LIMIT` 2. Jeweler gold buy is skipped (`max 7` vs market 8). Order AMV shows clamps (bakery grain buy 1.5 from target 2.0 / cap 1.5; farm grain sell 1.2 from target 1.0 / floor 1.2; well water sell 0.4 from target 0.3 / floor 0.4). After `day`, AMV/salability move and the AMV trail grows (seeded `seed 1`: grain 1 -> 1.0279, coin 1 -> 0.9016). `amv` reprints the trail. `match` still finds a deal without moving stock.

- Header lists **firm bounds** (`min` sell floor / `max` buy cap) and books have a **bound** column.
- Dummy production lines only set `target` / `inputs` so `create_orders` can rank buys. Bounds are hand-set on the roster, not computed. No processes run.
- Pops share the same desire spread set outright (not from demographics): basic grain+water, common bread, luxury jewelry. They emit **requests** only.
- No merchants. Firm default hand-typed buy priority is `FIRM_PRODUCER` (`2.5`).
- On a TTY the screen clears and redraws after each command (goods with AMV/sal/trail, roster, firm bounds, books, last log). Piped stdout prints the same then the last log. `match` is read-only. `day` settles. `amv` prints the trail.

**Goods (id / AMV / sal):** 1 grain 1.0 / 0.5, 2 water 0.3 / 0.35, 3 bread 2.2 / 0.45, 4 gold 8.0 / 0.7, 5 coin 1.0 / 1.0, 6 jewelry 15.0 / 0.8.

**Roster (intended roles, not live amounts):**

| actor | buying | selling |
|-------|--------|---------|
| farmers | water, bread, jewelry | - |
| laborers | grain, water, bread, jewelry | - |
| townsfolk | grain, water, bread, jewelry | - |
| farm | water | grain |
| bakery | grain | bread |
| mine | - | gold |
| mint | gold | coin |
| jeweler | gold | jewelry |
| well | - | water |

**Not built in the tester:** `next_shopping_trip`, cargo/transport goods on this roster. After `day`, AMV and salability on the market (and session history) update, and the AMV ring grows by one close. `main.rs` is still the Bevy hex stub. The lib loop is `Market::run_market_day`.

### Pop economic day (still closed through record keeping)

Unchanged from 2026-08-18 in substance on shop/save. `Pop::record_keeping` snapshots then rewrites next-day shop/save. Morning `update_desires` does **not** multiply shop/save. Consume need, days-of-buffer savings, reserved never negative, `create_orders` three passes (desire shop, parked non-desire shop, opportunistic extra). Request amounts are whole units; a shortfall below 1 is skipped.

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
| **Matching AMV** | Not used yet. Do not add AMV into `match_orders` unless asked. |
| **Deal AMV keep** | Pop min keep `0.25`, firm `0.50`, firm-need catch `0.25`. Use-goods skip salability; other received goods * salability. Buyers accept windfalls. `buy` / `evaluate` do not mutate. `finalize` does. |
| **Market day wash** | Reject or no proposal keeps the sell and charges `TRANSACTION_COST` transport from on-hand. Buyer `renew_buy` may put the buy back (`BUY_TRY_LIMIT` 2). Unmatched = unavailable, no fee, no renew. |
| **Transport / friction** | `GoodTag::Transport(efficiency)` (1.0 = time). Cover is `qty * efficiency`. Success bill is `TRANSACTION_COST + bulk * market.friction`. Cap fill then form the basket. Wash is the flat fee only. Unavailable goods live on the **market**. No Transport tag => bill 0. Bill and spend may be fractional. |
| **Whole units** | Orders and deal-map goods only. AMV stays fractional. Wagon bill stays fractional. Transport *goods in the deal map* are still whole units. |
| **Take good** | Removes the property row, returns quantity (0 if missing). |
| **Buy tenders** | Seller's named counter first (any salability), then live tenders by salability. `take_tenders` covers remaining units from that preferred set plus `HIGH_SALABILITY` (`0.8`). Low-sal only if those cannot cover. Shrink fill only after all tenders. **Make change** is returning excess, not this helper. |
| **Desire effect bake** | Additive arms (player resources, bonus goods) bake in `create_desire` / `update_desires`. Harvest does not multiply by count. |
| **Salability default** | `SALABILITY_DEFAULT` is `0.4` (new goods and missing history). Below exchange floor: not till money. Day-end lerp toward `payment / tender` when tender > 0. |
| **AMV drift** | Write live `MarketGood.amv`. Intra-day `buy` / `evaluate` / orders use frozen `history()`. Accept: both sides lerp toward basket midpoint. Reject: sought * 1.1 edge up, tenders down by units offered per unit sought. No-proposal: sought up only. Unmatched: no AMV move. |
| **AMV history** | `MarketGood.amv_history` ring (`AMV_HISTORY_MAX` 16). Seed opening AMV if empty at day start; push close after salability. Intra-day `set_amv` does not push. Tester `day` / `amv` / legend print the trail. |
| **Counter AMV** | Firm counters and spendable skip non-positive prices (AMV bounce can go slightly negative). |
| **Vocabulary** | Prefer `docs/design-vocabulary.md` over chat shorthand |
| **Comments** | ASCII only; **add, do not edit or replace** existing comments unless asked. New function comments: **what first**, why second |
| **Write / set** | Fill a field on create. **Update / edit** in the books. **Stamp** only a completed deal |
| **Knob** | Player-facing only |

---

## 5. Known debt / next-friendly work

### Natural next system

- **Wire PlayState** `phase_intra_market_day` to `Market::run_market_day` (needs an RNG on play state or the phase). Institution / state orders still missing.
- **Leftover book carry** — leftover buys/sells are reported then dropped; next day recasts from `create_orders`.
- **Tester `day` command** — landed. `MarketDayReport` is printed; do not invent a second shopping model in the example.
- **New orders after a fill** — `Pop::next_shopping_trip` (still `todo!()`), firm re-emit. After a buy fills, raise reserve toward stock target before re-calling `create_orders`, or merchants dump what they just bought.
- **Offer generation** — pops still only emit requests.
- **`sell` rewrite / haggling** — identity `sell`; Accept/Reject only.
- **AMV drift** — landed on accept (midpoint lerp) and reject (sought up, tenders down). Salability day-end from payment/tender. AMV history ring landed (open seed + daily close).

### Nearby leftovers (do not start unless asked)

- **Multimatch** — after the first match, same buyer + same seller, other goods at similar buy priority, one `ProposedDeal`. Variety sellers load the cart. Live matcher stays one pair. See `TODO.md`.
- Function comments repo-wide: lead with **what** the function does. Deal/bound helpers were rewritten; rest of `src/game/` is on `TODO.md`.
- Firm `amv_bound` is read by `create_orders` (clamp / skip-buy-if-market-above-cap) and by `buy` / `form_buy_proposal` as a unit-AMV ceiling. Planning does not write residual WTP / sell floor yet; default `None` keeps old behavior.
- Firm / institution / market / state `record_keeping` bodies still `todo!()`
- Firm `apply_passive_bonuses` is a stub; region/market bonus apply is unchecked
- `run_production` exists + tests; **not** wired into the production phase
- `Firm::create_orders` exists + tests; used by `run_market_day`, **not** wired into PlayState
- `Pop::start_day` exists; day-start phase still stub (TODO: "Completed not Connected")
- Migration orchestrator exists; leaves are `todo!()` (wants live sentiment + liquid wealth)
- Class demographics unimplemented (vault: park this)
- `income_amv` is not zeroed at day start (harmless until market pays)
- Player-resource yield not routed onto `State.resources`
- Optional later: spread firm overbuying across other goods (currently optimistic full `purchase_target`)
- Optional later: AMV as a *relative-to-average* sell-weight tweak in matching (not a hard filter; not total AMV)
- Optional later: day-fill rate cache if `get_demographic_rates` shows up at huge pop counts
- Optional later: `Pop.market_id` (update on migrate) instead of rebuilding `pop_to_market`
- Optional later: pace luxury consume so one desire does not empty leftover stock (see `TODO.md`)
- Optional later: marketing add on sell weight; recompute `sqrt(supply)` after partial fill; merchant vs producer firm flag; subordinated-firm priority; state purchase buckets
- Optional later: capital cost / maintenance / amortization into output `average_cost` (used capital currently returns whole; tools should wear)

### Comments still stale (fix only if the user asks)

- `Pop::update_desires` rustdoc still lists step 3 as scaling `shop_target` / `desire_needs` for growth (that block is gone)
- `PopRecords.savings_ratio` field still says "share of liquid wealth"
- `decay_goods` still calls `saved` a wish target in one place
- Original short `record_keeping` docblock was left as-is
- Playstate / firm / institution docs may still mention `Pop::demographic_update`
- `TODO.md` household-helper bullet lags the landed rates model
- Vault `Pops.md` household section still has a REWORK banner; morning step 3.5 still says resize shopping targets (record keeping owns that now)
- `compose_sell_priority` formula comments in the proposal may lag live `SELL_*` constants — prefer the constants

### Review log

Open review debt is empty. Second pass 2026-08-27 found no new code issues; handoff refreshed.

---

## 6. Where to look in code

| Concern | Location |
|---------|----------|
| Firm property + production flows | `src/game/firm.rs` → `FirmPRow`, `Firm::run_production`, `decay_goods`, `clear_day_flows` |
| Firm market orders | `src/game/firm.rs` → `Firm::create_orders`, `classify_on_hand`, `counter_good`, `Firm::take_good` (read-only emit; whole-unit amounts; skip `unavailable`) |
| Record keeping + planning + shop/save | `src/game/pop.rs` → `record_keeping`, `update_planning`, `rewrite_shop_and_save_targets`, `planning_growth_factor` |
| Cheapest tradeable basket | `src/game/pop.rs` → `cheapest_tradeable_cover` |
| Pop request orders | `src/game/pop.rs` → `create_orders` (plan, then parked shop, then extra desires; whole units; skip `unavailable`) |
| Dump a property row | `Pop::take_good`, `Firm::take_good` |
| Whole-unit helpers | `src/game/util.rs` → `whole_units`, `whole_units_up`, `is_whole_unit` |
| Order type + buy/sell priority helpers | `src/game/marketorder.rs` |
| Matching | `src/game/market.rs` → `Market::match_orders`, `OrderMatchBatch` |
| Market day | `src/game/market.rs` → `Market::run_market_day` (collect, collate, match/deal/finalize loop; returns `MarketDayReport`) |
| Deal making | `src/game/deal.rs` → `DealMaker`, `ProposedDeal`; impls on `Pop` / `Firm` (`buy` / `evaluate` / `finalize`) |
| World goods load | `src/game/factuals.rs` → `Factuals::load_from_path` / `load_from_toml`; file `data/world/goods.toml` |
| Market CLI | `examples/market_tester.rs` — `cargo run --example market_tester` (`shop`, read-only `match`, settling `day`, `amv` trail; goods from world file) |
| MarketGood setters / AMV bounce / AMV history | `src/game/market.rs` → `MarketGood`, `record_amv`, `amv_trail` |
| Order-priority tunables | `src/game/config.rs` → `market_priority`, `market_constants` |
| Priority design (deferred too) | `docs/proposals/market-order-priority.md` |
| Pop records / property rows | `src/game/pop_property.rs` |
| Planning tunables | `src/game/config.rs` → `pop_constants` |
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
3. Intramarket loop is **`Market::run_market_day`**. Natural next: PlayState wire, leftover book carry, `next_shopping_trip` / firm re-emit. Probe with `cargo run --example market_tester` (`shop` / `day` / `amv` / `match`). Do not invent a second shopping model in the example.
4. Match `STYLE.md` on any edits; update `reviewlog.md` when doing reviews.
5. Prefer vault **EconCiv** notes for design intent when code and notes disagree — **call out conflicts** rather than silent invention. Vault `Turns.md` sequential shopping walk vs collect-and-match: **match** is the live model.

---

## 8. One-line status

**Pop economic day is closed through record keeping. `Market::run_market_day` collects pop/firm orders, collates books, matches, deals (`buy` / `evaluate` / `finalize` / transport / wash), and records `MarketGood` stats. Tester `day` runs that loop and prints a `MarketDayReport` plus an AMV trail. Offers and proposals are whole-unit goods; AMV and the wagon bill stay fractional. PlayState intramarket phase and new orders after a fill are not wired. AMV drifts on accept/reject; salability updates from payment/tender at day end; AMV history records the open and each close.**
