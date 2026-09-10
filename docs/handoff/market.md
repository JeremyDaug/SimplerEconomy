# Market day, matching, and AMV

Read this only for the intramarket loop, books, matching, order priority, AMV,
or salability. Baskets / keep / tenders / wagon bill: `docs/handoff/deals.md`.
Names: vocabulary (order priority, AMV drift, AMV history, salability update).
Deferred ranking: `docs/proposals/market-order-priority.md`.

## Landed vs stub

| Piece | Status |
|-------|--------|
| `run_market_day` | Live lib loop. Tester `day` calls it. PlayState intramarket is `todo!()` |
| `match_orders` | One success per pass, front buy-priority group only |
| AMV drift + leftover book pressure | Meetings only (accept / reject / no-proposal). Leftover-book blend is **0** (off). The function still exists; volume-scaled leftover collapsed AMV to the bounce floor. Do not turn it back on unless asked. Intra-day evaluate uses frozen `history()` |
| AMV rescale | Each market day, unweighted mean of one unit of each tradeable good is scaled to 10.0 after salability, then the close is recorded. Time skipped. Trail is not rewritten. Firm AMV quotes/cost basis scale with it. Vault does not have this; it is a unit-normalization for readability |
| Salability day-end | Lerp toward `payment / tender` when tender > 0. After decay, cap at `1 - decayed/volume` (consumed is volume, not rot). Does not raise salability |
| AMV history ring | Seed opening AMV; push close after daily rescale. Do not rescale old samples. Cap 16 |
| Time AMV from labor | [`Market::settle_labor`] / [`Market::budget_labor`]. Hours-weighted wage AMV. Tracking only; wages do not follow it yet. Not a goods-book labor market |
| Institution / state orders | Not collected |
| New orders after a fill | Pop `next_shopping_trip` waves. Firms do not re-emit |
| Leftover book carry | Reported then dropped; next day recasts from `create_orders` |
| Multimatch | Later. Do not start |

## Market day

`run_market_day(factuals, pops, firms, rng)`. Only ids in `self.pops` /
`self.firms`. Lookups via `as_deal_maker(_mut)`; member ids are `expect`ed.

1. **Collect** `Pop` / `Firm` `create_orders`. Pop buy priority is **written**
   from per-household total AMV vs market max. `Pop::create_orders` itself still
   writes `POP_START` (4.0) as a placeholder.
2. **Collate** opening supply/demand/buyers/suppliers. Zero day exchange
   counters first (not AMV, salability, average price, stock, production,
   consumption, imports).
3. **Waves:** match until no pair. Hopeless front-group buys are **parked**
   (no fee, not unavailable); later buy bands still match against the sell
   book. Matched: buyer `buy`, seller `evaluate`;
   accept -> `finalize` + wagon bill; leftover orders scale down.
   Then each pop with transport cover for the door runs
   `next_shopping_trip` (open/parked request => offer only). Wash-closed
   goods are skipped that day (not the same as parked/no-seller). If
   anything posted, parked buys return and rematch. If not, parked ->
   `unavailable_goods`. Firms do not re-emit. See `deals.md`.
4. **Cleanup:** clear member pops' `current_orders`; leftover books are
   reported and do **not** move AMV (leftover_blend 0); salability lerps;
   rescale live AMV so one unit of each tradeable good averages 10.0
   (firm quotes scale with it; trail is not rewritten); push each
   good's close. Leftover rot cap (`Market::cap_salability_from_decay`)
   is a later caller after decay, not this method.

After an accepted deal, leftover sell/offer amounts clamp to on-hand.
Trip door cover: unreserved Time plus other transport on-hand. Wash-closed
goods are not re-requested that day.

## Matching

One pass, **does not mutate** the books. Buys by priority (lowest first); sells
by target good id. Only the **front** buy-priority group (shuffled). At most
**one** weighted sell. Coincidence doubles that sell's weight for this pick
only when both named counters match (`SELL_COINCIDENCE_WEIGHT = 2.0`). Pop
request/offer may name a counter **good** without an amount.
Self-trade skipped. No other-origin seller -> `unmatched_buys` (may be several).
Matchable leftovers in the same group stay. Do **not** add AMV into matching.
Do not batch several deals. RNG: `rand` 0.9.

## Order priority

Two uses: buy/request is FCFS (**lower first**, RNG among ties); sell/offer is
weight (**higher more likely**). Buy bands (pops `[4, 5)`, firms `[2, 3)`) are
`debug_assert`ed on **buys** only. Sells only need `priority > 0`.

Pop buy rank: **per household**, **total AMV**, not liquid. Richest -> band
start. Institutions `1` / `3` / `5`. Merchants `[2, 2.5)`, producers `[2.5, 3)`.
No state-among-pops slot. State firm inserts at `2.49` / `2.99`.

Sell compose (write on create, then flat-add fills): `1/band + sqrt(supply) +
0.25 * fills`. Floor band `0.01`. After a **reject**, that sell/offer's
weight is cut by `sell_reject_weight` (default 0.10) for the rest of the
day; books are recast next morning. Do not invert at match time. Marketing later.

Stale (notify only): proposal `compose_sell_priority` comments may lag live
`SELL_*` constants; `match_orders` rustdoc still describes const defaults (live
uses `match_orders_with_coincidence`).

## MarketGood / AMV

Default AMV `1.0`, salability `0.4` (below exchange floor: **not** till money).
AMV / average_price never `0` (bounce). Salability clamp `0..=1`. Volume is
derived (`purchased + payment`). Day logic should go through setters.
`history()` snapshots AMV, salability, `purchased`, and `amv_trails`. Missing
salability `0.4`; missing prices `1.0`. After decay, salability is capped at
`1 - decayed / volume` from aggregated pop/firm `decay_goods` returns.
Eaten stock is volume, not rot, so a fully consumed good is treated as if it
lasts. Leftover that rots pulls the cap down. Does not raise salability.

**Drift:** write live AMV; intra-day `buy` / `evaluate` / orders use frozen
`history()`. Accept: both sides lerp toward basket midpoint. Reject: sought
* 1.1 up, tenders down by tender AMV offered per sought AMV (not raw units).
No-proposal: sought up only. Leftover books do not move AMV
(`amv_leftover_blend` 0). Volume
scaled leftover (10% dry miss) collapsed unsold goods to the bounce floor.
Miss/purchased was tried earlier and exploded. Do not turn leftover-book
AMV back on unless asked. `set_amv` does not push the ring.
Every completed market day, live AMV and average_price are rescaled so
the unweighted mean of one unit of each **tradeable** good is 10.0. Time
is skipped. Period 0 disables. This is a unit change, not a value-theory
pass. Recorded trail samples stay as that day's close. Firm `amv_target`,
cost basis, and AMV bounds use the same scale.

**Code:** `market.rs`, `marketorder.rs`; tunables `factuals.config.market` /
`market_priority`.
