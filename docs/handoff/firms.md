# Firms

Read this only for firm property, production, planning, or `create_orders`.
Field names: `docs/design-vocabulary.md` (firm property row, sell success,
realized profit, AMV bound). Do not copy them here.

## Landed vs stub

| Piece | Status |
|-------|--------|
| `FirmPRow` + helpers | Landed |
| `run_production` | Landed + tests. Tester `day` calls it. Living roster firms work 10 Time for 150 output (cabins 1 Time / 15). PlayState production still `todo!()` |
| Keep-alive | `firm.keep_alive` (default off). Tester `keep_alive on`. Floors collapsed lines at 1 iteration and credits missing inputs plus coin. Credits immediately before each line so a later line still runs after an earlier one consumed stock |
| `plan` | Landed + tests. Called from `record_keeping` |
| `record_keeping` | Rolling average + `FirmRecords`, then `plan` |
| `create_orders` | Landed + tests. Used by `run_market_day`, not PlayState |
| `apply_passive_bonuses` | Stub |
| Re-emit after fill | Stub. Raise reserve toward stock **before** re-calling `create_orders` or merchants dump what they just bought |

World processes load **once** with factuals. Do not reload TOML in
`run_production`. Do not call pop `record_keeping` a second time for planning.

**Day order:** produce, then pop consume, then plan. Vault `Turns.md` disagrees;
call it out, do not "fix" the live order unless asked.

## Invariants

- `reserve` is a stockpile guarantee (`sync_reserve` = `min(quantity, reserve_target)`). Not pop `reserved`.
- Output `stock_target` is decay-adjusted `operations_cover` days (default 5): shrink the hold when a full pile would lose more than one day's output to rot; overshoot when it would not. Remainder leftover uses that fence. Input `stock_target` is decay-adjusted `input_cover` days of use (default 4) and is **not** reduced by output on hand. Wages may raid it down to today's use or sell plan (`wage_fence`). Recap does not fill output stock. Init opening stock is `OPENING_COVER_DAYS` (3) decay-adjusted days of output, not the live fence, plus `OPENING_INPUT_DAYS` (4) of required non-Time inputs. Excess output above `output_cover` days (default 1) is added to `sell_target` and may post even when salability * daily output is smaller.
- Cold / idle `target` 0 stays 0 until something actually sells. Do not treat a dead shop as a restart problem unless asked.
- In-kind remainder/wage transfers record `placed` at market AMV. Sell success and realized profit credit `min(placed, stock_fence)` plus market `sold`. Dump above the fence is not a hit.
- `sellable` = `quantity - max(reserve, reserve_target)`. `free_for_market` adds stock/use fences when `use_target` > 0.
- `clear_day_flows` is day start (totals stay visible overnight). `decay_goods`
  returns `used` then decays `quantity`, then moves `held` into `quantity`.
- **AMV bound** is a planning guidestone, **not** a trade gate. `create_orders` still posts the row's own bid/ask; `buy` still forms a basket when payment AMV is above the bound. Keep ratio can still reject. Later: headroom vs market for shrinking a line.
- `plan` writes residual WTP as the buy cap and consumed-input AMV rollup as the sell floor. Default `None` until `plan` runs.

Helpers: `available`, `sellable`, `free_for_market`, `purchase_qty`, `mid_amv`,
`bid_amv` / `ask_amv`, labor fences `stock_fence` / `wage_spendable` /
`profit_spendable`. `plan` writes `growth_target` as the expansion gap
(quota above aim) on a grow decision, else 0.

## Production

Records `produced` / `consumed` / `used`; returns `Vec<ProcessEffect>` (no
`ProductionReport`). Destroyed and Consumed inputs both go to `consumed`
(on-hand `quantity` first, then `held`); Consumed decay products go to
`produced`; capital goes to `used` only; factors untouched. Process outputs
(and Consumed-input decay products) land in `held`, not `quantity`. Later
lines may spend `held` after on-hand stock. Decay releases `held` after
on-hand rot, so today's output skips tonight's decay. Output `average_cost`
blends consumed-input AMV only — used capital is not in that blend
(amortization later, not v0). Vault `Processes.md` does not describe `held`;
do not invent a third model.

A line starting from 0 snaps to at least 1 iteration. Missing inputs throttle
the run (`last_missing_goods`). `plan` walks quota toward last iterations
on a run miss (floor 1), except a missing-input day or leftover buy demand
for the output keeps aim and quota. An idle `target` 0 restarts at 1 when
leftover buys exist for that output.

## Plan

`plan(&mut self, factuals, history)`. Gather then adjust. Pace is
`planning_lerp_rate` (growth/shrink steps are `growth_rate` / `shrink_rate`).

1. **Gather:** line **productivity** (process AMV-out / AMV-in, peer rank);
   per output **realized profit**, market `sold`, sell-meeting counts, own vs
   market AMV. Competitor quotes are `None` until other firms are passed in.
2. **Adjust:** `aim` lerps toward throughput evidence. Then one walk step
   (raise/cut quote, raise/cut quota, or stay) scored as
   `expected_sold * quote - qty * unit_cost`. Expected sold uses the EMA of
   market `sold` (`sold_avg`, not remainder `placed`) and blended meeting
   counts. Cuts only fire on a blended miss (`sell_success_shrink`); raises
   only on a blended hit. Raise quota is skipped when the line is underwater
   unless extra units are predicted to sell. Near-tied scores pick the step
   closer to a full clear. Stay / quote-only days lerp quota toward `aim`.
   A run miss still walks quota toward last iterations unless the miss is
   missing inputs or the output still has leftover buys (keep operating
   scale). Shrink step matches
   growth (`shrink_rate` 0.10). Orbit live market AMV by ±`quote_orbit`.
   Cold-start keeps the line unless leftover buys exist for an output, in
   which case an idle line snaps to 1 iteration. `target: None` stays None.
3. **Rollup:** input use/stock/purchase/reserve, AMV bounds, merchant restock.
   Output `stock_target` is `operations_cover` days. Input cover is
   `input_cover` days of use, not leftover after output. Excess output above
   `output_cover` days is added to `sell_target`. Does not overwrite output
   AMV from adjust. Merchant-only rows restock
   what sold and keep `amv_bound` None. Till / barter with no recipe role: leave
   alone.

Own `amv_target` is nudged, not lerped onto live market AMV. Market AMV
rescale multiplies `amv_target`, cost basis, and AMV bounds by the same
factor so quotes stay in the current unit.
`record_keeping` snapshots then calls `plan` — do not also call `plan` the same
day. Do not re-add a confidence pace scale. Tunables: `factuals.config.firm`.
Tests: `firm::plan_should`.

## create_orders

Read-only mechanical emitter. Honors current targets and stock. **Does not
replan.** Skip buys in `unavailable`. Posted **goods** are whole units; named
counters ceil; bid/ask AMV stays fractional.

On-hand `free_for_market` is sell / exchange / liquidate:

- **Exchange** if salability >= `0.6`. High-sal leftover with no purchase/sell/use is till money, not a dump.
- **Sell** if posted sell > 0. Posted sell is `min(sell_target, max(max market
  salability * daily output, excess above output_cover days))` for goods this
  firm makes; unconstrained `sell_target` otherwise. Sell-success still uses
  `sell_target`.
- **Both:** lerp 90/10 sell/exchange at 0.6 to 10/90 at 1.0. Exchange rounds half-up; sell is the remainder, capped at posted sell (overflow stays exchange).
- **Liquidate** if free stock, no purchase/sell/use, salability below 0.6. Always **offer**, never priced sells.

Dual buy+sell: producers (`use_target` > 0) buy only the stock-target shortfall
and sell only free excess. Merchants emit full `purchase_target` even above
stock. A sell-plan good can still tender its **exchange** slice.

Until every process input has a day's `use_target` on hand, output (and
other non-use stock) is pulled from sell/liquidate into exchange so it can
pay for that shortfall. After that, selling is unchanged.

Budget is optimistic (last buy may overdraw). No spendable AMV -> no buys.
Non-positive AMV is not spendable and not a legal counter. Production inputs
sort before merchant restock. Sell counter: most valuable still-needed process
input, else the market's most salable money good (even if not on-hand) — so
mine/well ask for coin. Buy counter: an on-hand exchange good. No counter ->
request/offer.

Merchant-like (purchase+sell, no use) -> `FIRM_MERCHANT`; else `FIRM_PRODUCER`.
Matching does **not** use AMV. After an accepted deal, leftover sell/offer
amounts clamp to on-hand so a tender cannot overdraw a later sell of the same
stock.

Stale (notify only): `create_orders` rustdoc still links `market_constants`
(live reads `factuals.config`).

**Code:** `firm.rs`, `firm/plan.rs`, `firm/orders.rs`. Tests:
`create_orders_should`, `plan_should`.
