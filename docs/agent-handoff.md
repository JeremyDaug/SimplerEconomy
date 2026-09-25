# Agent handoff — EconCiv rework

**Branch:** `EconCiv-Rework-Branch`  
**Updated:** 2026-09-24

**Router, not a dump.** Read **Status** + **Routing**. Open **one** topic file
and the listed code. Session order and "do not open" list: `AGENTS.md`.

This file is **what is landed** and **where to look**. Topic files are
invariants and traps, not a substitute for the code.

---

## Status

- Pop economic day is closed through record keeping.
- Pop `create_orders` posts ceil'd shop-plan requests, freezes tender cover,
  then floor'd offers of leftover surplus. Offers/requests may name a
  `counter_offer` good (no AMV or amount). Shop need spends on-hand then
  spreads leftover sat across buyable substitutes. Morning `create_orders`
  posts a higher tier only when the wallet covers the lower one. Save AMV
  scales with durability (decay 1.0 => no save). Consume runs a higher
  tier only after every lower tier is complete. An empty tier counts
  as complete.
  `run_market_day` posts once, then matches until quiet: random buy among
  those with an other-origin sell, sell weighted by listed amount
  (coincidence multiplies). Same origin never pairs. No leftover-book AMV.
  Until a day's
  process inputs are on hand, firm output is held as exchange (payment)
  instead of posted sell. Listed offer units and
  `reserved` are not tenderable. World goods use per-good `decay_rate` in
  `goods.toml` (Time 1.0; other rates pulled back so a few days of stock
  survive; food still rots faster than metal).
  Luxury leftover shop is capped at one extra luxury level of the cheapest
  luxury good. After decay, salability is capped at `2 * (1 - decayed/volume)`.
  AMV drifts on accept (more salable goods move less) and a ±1 kick toward
  heavier opening demand vs supply. Reject lowers
  tender salability, not AMV. Salability is 0..=2 (par at 1, currency at 1.8).
  Each market day AMV is rescaled so one unit of
  each tradeable good averages 100.0 (after salability, then the close).
  Firm AMV quotes scale with it; recorded trail samples do not.
  After the market day each pop records a buy stop (`market` / `money` /
  `transport`) if shop shortfalls remain.
  Pop keep: incoming bag uses consume / save / extra-desired / unused
  (0 / 25 / 50 / 100 salability penalty, best category lifts the bag).
  Outgoing units peel extra → save → consume at those same factors. The
  0.50 floor always applies (no floor-drop).
- Live intramarket loop: `Market::run_market_day`. Tester day is labor,
  produce onto `held`, market (may sell `held`), pop consume from the
  bag, decay (`held` skips tonight), plan. Vault `Turns.md`
  is market then production.
  PlayState intramarket and production phases are stubs.
- `Firm::plan` rewrites production and property targets. Line `aim` lerps
  toward throughput. Then one walk step (raise/cut quote or quota, or stay)
  scored as predicted `sold * quote - qty * cost` from the EMA of market
  sold and meeting mix (remainder `placed` is not demand). Cuts only on a
  blended miss; raises only on a blended hit. Shrink step matches growth
  (0.10). Stay / quote-only days lerp quota toward aim. Quotes orbit live
  market AMV by ±`quote_orbit` (default 10%). Recipe `amv_bound` is a cost
  floor, not the quote. Firm keep uses the quote as bid/ask.
  Output `stock_target` is decay-adjusted `operations_cover` days of `aim`
  (remainder fence). Input `stock_target` is decay-adjusted `input_cover`
  days of use and is not reduced by output on hand. Excess output above
  `output_cover` days is posted for sale even when salability * daily is
  smaller. Remainder leftover uses the output fence; wages may raid it down
  to today's use/sell plan. Init opening stock is `OPENING_COVER_DAYS` (3)
  decay-adjusted days of output, not the live five-day fence, plus
  `OPENING_INPUT_DAYS` (4) of required non-Time inputs. Recap does not fill
  output stock.
  In-kind remainder/wage transfers record `placed` at market AMV; sell
  success credits `min(placed, stock_fence)` plus sold. Leftover buys
  and in-shop input need are the same kind of demand as a sale. A quota
  cut stops at the owner's unmet output and does not raise a line to it.
  Revenue below unit cost cuts quota above that floor. A run miss walks quota toward
  last iterations unless the miss is missing materials or leftover buys
  / in-shop input still want the output. Missing Time is a scale miss.
  Idle `target` 0 restarts at 1 when that demand exists and the line is
  the best recipe for the good. Weaker
  duplicate recipes (lower AMV profit) walk down. Lines idle
  `abandon_idle_days` (5) without demand are dropped. Empty firms remain;
  tester tables print `dead/abandoned`. Production pays the firm-wide
  complexity Time tax first, then input-feeding lines, then higher
  recipe AMV profit.
  Remainder
  recap fills recipe inputs, wages, and the input stock fence. It does
  not pull owner dinner. Owners eat from their own bag and may still
  tender shelf above the stock reserve. Production spendable stock
  is on-hand plus held. Finished output can tender for inputs the shop
  cannot make.
  `growth_target` is the expansion gap on a grow, else 0.
- Time is good id 0 (untradeable, transport 1.0, bulk 0). Pops get 64 * household labor
  at `Pop::start_day`. Live intramarket friction is 1 (`TRANSACTION_COST + bulk`).
  Goods have per-unit mass/volume in `goods.toml`.
- Remainder owner-operators always give claimed hours (even unpaid) and
  top up remaining recipe Time. Hired workers still withhold Time when
  unpaid.
- Firm reject lowers tender salability at `salability_firm_reject_scale`
  (default 0.25) of the pop blend. Firm received units peel need → stock →
  growth → unused (no bag sweetener). After Accept, firms make change
  (return unused tenders until keep ~ 1). Pop leftover offers keep a 25%
  wallet floor.
- Labor **operates**. Tester `day` calls [`Market::settle_labor`] then
  [`Market::budget_labor`]. Each settle is a signed goods map on the
  workforce contract; the market records it as an accept when Time was
  given and goods were received. Daily rescale skips Time. Firms do **not**
  rewrite wages from Time AMV yet (pops cannot move or resize). PlayState
  labor fire is still a stub.
- World goods, processes, and config load from `data/world/`. Specialized
  recipes: one per good (Time is process 28). Subsistence farm / water /
  forage are processes 29–31, tagged weight 0.25 (untagged 1.0, weight > 0).
  Raw extracts take Time only as required; grain and wood may take optional
  boosters. Crafted recipes take Time plus a destroyed material. Init firms
  keep the one process named in their file. Hours are that line's Time.
  One-line shops pay no complexity tax.
- Tester CLIs are **paused** unless asked. `market_tester` `solo` is one
  remainder pair (default id 1) for internal plan. Living roster loads from `data/init/`
  (five pops: bread, gold, the second grain shop, the second well, and
  cabins. Grain, water, and wood firms remain without those pops). Load
  drops unused world goods so CLI/CSV only show the village catalog.
  `pop_tester` is the same pops with no firms; each morning the matching init firm's process outputs (`amount * target`) are a stock cap (add the shortfall only). Opening AMV 100.0 / salability 0.1 on every good.
  Village consume desires (basic food/hydration/wood heat, common one cabin
  per household, extra bread, extra storage on gold / gold_token, luxury
  gold_token / jewelry, and rest on Time) are duplicated onto every pop.
  Food/water/heat/bread/gold are 1 unit per member (5 units). **No** opening 1-of-each kit
  (init starter empty; `DAILY_ENDOWMENT` 0). Pops open with one day of the
  matching firm's output (Time skipped). Each morning: `start_day` Time, then specialty
  grant is 0 (`DAILY_OUTPUT` in `roster.rs`). Each firm is that pop's remainder
  owner-operator: hours = target * Time input (grain/water/wood 8, bread/gold 5,
  cabins 2), no wage basket. Crafted lines list material inputs;
  raw extracts list Time only. Opening stock is three decay-adjusted days of
  process output plus four days of required non-Time inputs (`use_target` =
  one day's recipe use). Posted firm sells cap at max market salability times
  daily output; remainder leftover is extra above that. Remainder owners
  cover an AMV shortfall on a loss. Coin is `gold_token`; iron ore is `iron`.
  `keep_alive on` is an emergency firm subsidy (1-iteration floor +
  coin/inputs); default off.
  Desire amounts do not rise with success.
  Luxury leftover shop is capped at one extra luxury level of the cheapest
  luxury good.
  AMV moves on accept (salability-weighted) plus a flat ±1 demand/supply
  kick. Reject lowers tender salability, not AMV. Leftover book blend is 0.
  Volume-scaled leftover collapsed AMV to the bounce floor; do not turn it
  back on unless asked.
  CSV is market + trades always; pops/firms only when flagged (`csv on`).
  Tester captures live in `data/logs/` (gitignored). Keep at most three local
  files (reference / current / spare); do not commit run dumps.
  **Checkpoint:** with mean 100 and ±1 imbalance kick, a 180-day pop_tester
  run held AMVs off the bounce (gold ~12, tools ~300). Do not retune leftover
  AMV or re-add the 1-of-each grant unless asked.
  Init firms are specialty-only. The household basket is off the shop.
  Owners eat from their own bag. A pop's morning work shares one output
  floor per good. The higher profit ratio takes that floor, and its
  stored cap when the output is worth selling. A worse recipe for the
  same good stays at 0. Time the morning shop needs for the wagon stays
  in the bag. Time still left is spent on the best recipe that can
  still run. A firm quota cut stops at
  the owner's unmet output; revenue below unit cost walks extra output
  down. Split of a divided multi-pop shop is landed
  ([`creation.md`](handoff/creation.md)). The eight 1-pop shops cannot
  split. Savings founding waits on a money good.

**Next (named):** savings founding is parked. Split is landed
(`Firm::split`): one workforce pop leaves a shop of two or more, lines
scale by `1/n`, whole-unit stock goes with them, and the child may add
one line and/or remove one. Do not split the eight 1-pop shops. Do not
re-attach subsistence lines onto firms. Do not put the dinner fence,
shelf-eating, or a raise-to-hunger rule back. Do not call `Firm::plan`
from a pop. Do not retune remainder plan.

**Vault conflict:** `Turns.md` puts firm planning before consume. Live order is
produce, then consume, then plan. Call it out; do not silently "fix" either side.

**Live day (tester / intended lib order):**
`start_day` -> `Market::settle_labor` -> `run_market_day` -> `run_production`
(outputs go to `held`) -> pop consume / sentiments / decay -> firm decay
(`quantity` rots, then `held` joins `quantity`) -> salability rot cap ->
pop `record_keeping` -> firm `record_keeping` (`plan`) ->
`Market::budget_labor`.
`held` skips tonight's rot and joins `quantity` after on-hand decay, so
today's output is on the next market day. Planning **after** decay is still
required so shop_targets do not fence stock that will rot overnight.

---

## Where truth lives

| Question | Look in |
|----------|---------|
| What is this called? | `docs/design-vocabulary.md` |
| Is it landed, stubbed, or later? | Matching topic file, **Landed vs stub** |
| How does it actually work? | The listed code |
| Turn-pipeline checklist | `TODO.md` (only if wiring a phase) |
| Open review debt | `reviewlog.md` |
| Should behavior change? | Matching EconCiv vault note, then say so |

Do not implement from a topic file. Do not open the vault for a bugfix, wire-up,
or refactor that follows existing behavior.

---

## Routing

Match the user's task. Stay in those files.

| Task | Topic file | Code |
|------|------------|------|
| Labor, wages, Time, workforce | [`labor.md`](handoff/labor.md) | `workforce.rs`, `Firm::settle_labor_contracts`, `Firm::pay_wage_shares` |
| Firm plan, production, `FirmPRow`, records | [`firms.md`](handoff/firms.md) | `firm.rs`, `firm/plan.rs` |
| Firm `create_orders` | [`firms.md`](handoff/firms.md) | `firm/orders.rs` |
| Found / split / savings founding | [`creation.md`](handoff/creation.md) | none yet; do not found inside `create_orders` |
| Market day, matching, AMV, salability, order priority | [`market.md`](handoff/market.md) | `market.rs`, `marketorder.rs` |
| Deals, tenders, keep, transport, whole units | [`deals.md`](handoff/deals.md) | `deal.rs`, `pop/deal.rs` |
| Pop consume, shop/save, desires, sentiment | [`pops.md`](handoff/pops.md) | `pop.rs`, `pop/orders.rs`, `desire.rs`, `pop_property.rs` |
| Tester CLI, `day`, CSV | [`tester.md`](handoff/tester.md) | `examples/market_tester/`, `examples/pop_tester/` |
| World data, config, factuals | [`world.md`](handoff/world.md) | `factuals.rs`, `config.rs`, `data/world/` |
| Init pops/firms (scenario) | [`world.md`](handoff/world.md) | `init.rs`, `data/init/` |
| PlayState / turn wiring | [`turns.md`](handoff/turns.md) | `playstate.rs` |
| Order-priority numbers (deferred ranking too) | `docs/proposals/market-order-priority.md` | `config::market_priority` |

If nothing matches, stay in the files the user named. If the task is unclear,
ask one question instead of opening more topics.

---

## Do not start unless asked

Nearby leftovers are traps, not implied scope: multimatch; `sell` rewrite /
haggling / make-change; PlayState intramarket or production wire (unless that
**is** the task); tester pages / extra CSV; species-culture-religion TOML;
init/save data; class demographics; capital amortization; AMV as a matching
weight; intra-day luxury loop; leftover-book AMV (off).

If the user did not name a task, **ask**. Do not pick a next system on your own.
If they ask "what's next": savings founding is parked until a money
good ([`creation.md`](handoff/creation.md)). Split is landed. Ask before
starting another system. The thin plan is landed
([`firms.md`](handoff/firms.md)).
PlayState `phase_intra_market_day` is still unwired. Hiring classes and
savings founding wait. The eight 1-pop shops are specialty-only and
cannot split.

---

## Cross-cutting traps

These are the ones agents keep "fixing." Style, comments, and names are in
`AGENTS.md` / `STYLE.md` / vocabulary — do not restate them here.

- Household rates are **not** on each pop. `Factuals::get_demographic_rates`.
  Do not re-add `DemoRow.rates`.
- `create_orders` (pop or firm) is mechanical. Do not replan there.
- Do not add AMV into `match_orders`.
- Target efficiency is always **positive** (`debug_assert` only).
- Whole-unit **goods** on orders and deal maps. AMV and the wagon bill stay
  fractional.
- `debug_assert` for "must never happen." No hot-path `is_finite` fallbacks.
- **Write / set** on create; **update / edit** in the books; **stamp** only a
  completed deal. Never bare "priority", "ratio", or "knob" (knob is player-UI).

---

## How to refresh

Update the **matching topic file** and **Status** here. Do not paste essays
into this router. Do not copy vocabulary or `TODO.md` into topic files.
