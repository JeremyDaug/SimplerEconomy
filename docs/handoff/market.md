# Market day, matching, and AMV

Read this only for the intramarket loop, books, matching, order priority, AMV,
or salability. Baskets / keep / tenders / wagon bill: `docs/handoff/deals.md`.
Names: vocabulary (order priority, AMV drift, AMV history, salability update).
Deferred ranking: `docs/proposals/market-order-priority.md`.

## Landed vs stub

| Piece | Status |
|-------|--------|
| `run_market_day` | Live lib loop. Tester `day` calls it. PlayState intramarket is `todo!()` |
| `match_orders` | One success per pass: random matchable buy, amount-weighted sell |
| AMV drift | **Accept only** plus a **flat ±1** opening demand/supply kick (`amv_imbalance_kick`). Reject and no-proposal do not move AMV. Intra-day evaluate uses frozen `history()`. Accept blend is salability-weighted (more salable goods move less). |
| AMV rescale | Each market day, unweighted mean of one unit of each tradeable good is scaled to 100.0 after salability, then the close is recorded. Time skipped. Trail is not rewritten. Firm AMV quotes/cost basis scale with it. Vault does not have this; it is a unit-normalization for readability |
| Salability | Range `0..=2`. `0..=1` discounted/discovering; `>=1` at-par (no keep haircut); `>=1.8` currency. Reject lowers tender S; **firm** reject uses `salability_firm_reject_scale` (default 0.25) of the pop blend. Day-end lerp toward `(payment/tender)*2` when tender > 0. After decay, cap at `2 * (1 - decayed/volume)`. |
| AMV history ring | Seed opening AMV; push close after daily rescale. Do not rescale old samples. Cap 16 |
| Time AMV from labor | [`Market::settle_labor`] / [`Market::budget_labor`]. Hours-weighted wage AMV. Tracking only; wages do not follow it yet. Not a goods-book labor market |
| Institution / state orders | Not collected |
| New orders after a fill | None. Morning `create_orders` only |
| Leftover book carry | Reported then dropped; next day recasts from `create_orders` |
| Multimatch | Later. Do not start |

Tester calendar (labor, **production**, market, consume, decay, plan)
is **not** vault `Turns.md` (market, then production, then consume).
Today's output sits in `held`, may sell the same afternoon, and skips
tonight's rot. Call the vault conflict; do not invent a third clock.

## Market day

`run_market_day(factuals, pops, firms, rng)`. Only ids in `self.pops` /
`self.firms`. Lookups via `as_deal_maker(_mut)`; member ids are `expect`ed.

1. **Collect** `Pop` / `Firm` `create_orders` once.
2. **Collate** opening supply/demand/buyers/suppliers. Zero day exchange
   counters first (not AMV, salability, average price, stock, production,
   consumption, imports).
3. **Match until quiet.** Random buy among those with an other-origin sell;
   sell picked by listed amount (coincidence multiplies). Same origin never
   pairs. Matched: buyer `buy`, seller `evaluate`; accept -> `finalize` +
   wagon bill; leftover orders scale down. Buys with no seller ->
   `unavailable_goods`. No parking, no shopping-trip re-emit.
4. **Cleanup:** clear member pops' `current_orders`; leftover books are
   reported; **flat ±1 AMV** toward heavier opening
   demand vs supply (deferred: ±1% of |AMV|, or +1 demand / −1% supply);
   salability lerps;
   salability updates (reject already moved tender S; day-end lerps
   payment/tender toward 0..=2); rescale live AMV so one unit of each
   tradeable good averages 100.0
   (firm quotes scale with it; trail is not rewritten); push each
   good's close. Leftover rot cap (`Market::cap_salability_from_decay`)
   is a later caller after decay, not this method.

After an accepted deal, leftover sell/offer amounts clamp to on-hand.
Trip door cover: unreserved Time plus other transport on-hand. Wash-closed
goods are not re-requested that day. After the last wave, each pop gets a
buy stop (`market` / `money` / `transport`) if shop shortfalls remain.

## Matching

One pass, **does not mutate** the books. Pick a buy at random among those
with an other-origin sell of that good. Pick that sell weighted by listed
units; coincidence multiplies when both named counters match
(`SELL_COINCIDENCE_WEIGHT = 2.0`). Same origin never pairs (a merchant may
buy grain and sell bread; they may not fill their own grain book). At most
**one** pair per pass; the day loops until quiet. Do **not** add AMV into
matching. Do not batch several deals. RNG: `rand` 0.9.
Remainder owners tender shop stock above dinner when `buy` would otherwise
have an empty bag (`Pop::buy_with_firm`). No-proposal is still "no tender
named," not a skip of the pair.

## Order priority

Matching does **not** use buy FCFS or firm-before-pop. Sell match weight is
listed units (coincidence may multiply). Offer `priority` is those listed
units so leftover-sell reject cuts still have a number. Buy `priority` is
the create default, not wealth rank.

## MarketGood / AMV

Default AMV `1.0`, salability `0.4` (below exchange floor: **not** till money).
AMV / average_price never `0` (bounce at `amv_min_abs` 1e-7). Salability
clamp `0..=2`. Volume is derived (`purchased + payment`). Day logic should
go through setters. `history()` snapshots AMV, salability, `purchased`, and
`amv_trails`. Missing salability `0.4`; missing prices `1.0`. After decay,
salability is capped at `2 * (1 - decayed / volume)` from aggregated
pop/firm `decay_goods` returns. Eaten stock is volume, not rot. Leftover
that rots pulls the cap down. Does not raise salability.

**Drift:** write live AMV; intra-day `buy` / `evaluate` / orders use frozen
`history()`. Accept: both sides lerp toward basket midpoint, more salable
goods move less. Reject: lower tender salability, **not** AMV. No-proposal:
neither. Leftover books do not move AMV. Then a
**flat ±1 AMV** kick toward heavier opening demand vs supply
(`amv_imbalance_kick`; Time skipped; tie does nothing). Deferred kick
shapes: ±1% of |AMV|, or +1 demand / −1% supply. `set_amv` does not push the ring.
Every completed market day, live AMV and average_price are rescaled so
the unweighted mean of one unit of each **tradeable** good is 100.0. Time
is skipped. Period 0 disables. This is a unit change, not a value-theory
pass. Recorded trail samples stay as that day's close. Firm `amv_target`,
cost basis, and AMV bounds use the same scale.

**Code:** `market.rs`, `marketorder.rs`; tunables `factuals.config.market` /
`market_priority`.
