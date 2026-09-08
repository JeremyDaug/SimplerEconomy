# Firms

Read this only for firm property, production, planning, or `create_orders`.
Field names: `docs/design-vocabulary.md` (firm property row, sell success,
realized profit, confidence, AMV bound). Do not copy them here.

## Landed vs stub

| Piece | Status |
|-------|--------|
| `FirmPRow` + helpers | Landed |
| `run_production` | Landed + tests. Tester `day` calls it. PlayState production still `todo!()` |
| Keep-alive | `firm.keep_alive` (default off). Tester `keep_alive on`. Floors collapsed lines at 1 iteration and credits missing inputs plus coin. Credits immediately before each line so a later line still runs after an earlier one consumed stock |
| `plan` | Landed + tests. Called from `record_keeping` |
| `record_keeping` | Rolling average + `FirmRecords` (incl. **confidence**), then `plan` |
| `create_orders` | Landed + tests. Used by `run_market_day`, not PlayState |
| `apply_passive_bonuses` | Stub |
| Re-emit after fill | Stub. Raise reserve toward stock **before** re-calling `create_orders` or merchants dump what they just bought |

World processes load **once** with factuals. Do not reload TOML in
`run_production`. Do not call pop `record_keeping` a second time for planning.

**Day order:** produce, then pop consume, then plan. Vault `Turns.md` disagrees;
call it out, do not "fix" the live order unless asked.

## Invariants

- `reserve` is a stockpile guarantee (`sync_reserve` = `min(quantity, reserve_target)`). Not pop `reserved`.
- `sellable` = `quantity - max(reserve, reserve_target)`. `free_for_market` adds stock/use fences when `use_target` > 0.
- `clear_day_flows` is day start (totals stay visible overnight). `decay_goods` returns `used` then decays stock.
- **AMV bound** is a planning guidestone, **not** a trade gate. `create_orders` still posts the row's own bid/ask; `buy` still forms a basket when payment AMV is above the bound. Keep ratio can still reject. Later: headroom vs market for shrinking a line.
- `plan` writes residual WTP as the buy cap and consumed-input AMV rollup as the sell floor. Default `None` until `plan` runs.

Helpers: `available`, `sellable`, `free_for_market`, `purchase_qty`, `mid_amv`,
`bid_amv` / `ask_amv`, labor fences `stock_fence` / `wage_spendable` /
`profit_spendable`. `growth_target` is read at labor settle; `plan` does not
write it yet.

## Production

Records `produced` / `consumed` / `used`; returns `Vec<ProcessEffect>` (no
`ProductionReport`). Destroyed and Consumed inputs both go to `consumed`;
Consumed decay products go to `produced`; capital goes to `used` only; factors
untouched. Output `average_cost` blends consumed-input AMV only — used capital
is not in that blend (amortization later, not v0).

A line starting from 0 snaps to at least 1 iteration. Missing inputs throttle
the run (`last_missing_goods`); they do **not** shrink the line in `plan`.

## Plan

`plan(&mut self, factuals, history)`. Gather then adjust. Pace is `plan_pace`:
confidence 0.5 uses `planning_lerp_rate`; 0 is half speed, 1 is 1.5x.

1. **Gather:** line **productivity** (process AMV-out / AMV-in, peer rank);
   per output **realized profit** (sold unit AMV / average cost), sell success,
   stockpile vs `output_cover`, decay, own vs market AMV. Competitor quotes are
   `None` until other firms are passed in.
2. **Adjust:** from a quiet baseline, nudge sell plan and own quote. **Do not
   grow sell or production unless sell success >= `sell_success_grow`.** Then
   equalize peer lines and align output to the sell plan. Cold-start keeps the
   line and the sell plan. `target: None` stays None.
3. **Rollup:** input use/stock/purchase/reserve, AMV bounds, merchant restock.
   Does not overwrite output sell/AMV from adjust. Merchant-only rows restock
   what sold and keep `amv_bound` None. Till / barter with no recipe role: leave
   alone.

Own `amv_target` is nudged, not lerped onto live market AMV.
`record_keeping` snapshots then calls `plan` — do not also call `plan` the same
day. Tunables: `factuals.config.firm`. Tests: `firm::plan_should`.

## create_orders

Read-only mechanical emitter. Honors current targets and stock. **Does not
replan.** Skip buys in `unavailable`. Posted **goods** are whole units; named
counters ceil; bid/ask AMV stays fractional.

On-hand `free_for_market` is sell / exchange / liquidate:

- **Exchange** if salability >= `0.6`. High-sal leftover with no purchase/sell/use is till money, not a dump.
- **Sell** if `sell_target` > 0. No salability cap.
- **Both:** lerp 90/10 sell/exchange at 0.6 to 10/90 at 1.0. Exchange rounds half-up; sell is the remainder, capped at `sell_target` (overflow stays exchange).
- **Liquidate** if free stock, no purchase/sell/use, salability below 0.6. Always **offer**, never priced sells.

Dual buy+sell: producers (`use_target` > 0) buy only the stock-target shortfall
and sell only free excess. Merchants emit full `purchase_target` even above
stock. A sell-plan good can still tender its **exchange** slice.

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
