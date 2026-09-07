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
| AMV drift + leftover book pressure | Live `MarketGood.amv`; intra-day evaluate uses frozen `history()` |
| Salability day-end | Lerp toward `payment / tender` when tender > 0 |
| AMV history ring | Seed opening AMV; push close after salability. Cap 16 |
| Institution / state orders | Not collected |
| New orders after a fill | **Not** added |
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
3. **Loop** until the buy book is empty: one matched pair plus hopeless
   front-group buys. Hopeless -> `unavailable_goods` (no meeting, no fee, no
   renew). Matched: buyer `buy`, seller `evaluate`; accept -> `finalize` +
   wagon bill; reject / no proposal -> wash. See `deals.md`.
4. **Cleanup:** clear member pops' `current_orders`; leftover books pull AMV;
   salability lerps; push each good's close.

After an accepted deal, leftover sell/offer amounts clamp to on-hand.

## Matching

One pass, **does not mutate** the books. Buys by priority (lowest first); sells
by target good id. Only the **front** buy-priority group (shuffled). At most
**one** weighted sell. Coincidence doubles that sell's weight for this pick
only when both named counters match (`SELL_COINCIDENCE_WEIGHT = 2.0`).
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
0.25 * fills`. Floor band `0.01`. Do not invert at match time. Marketing later.

Stale (notify only): proposal `compose_sell_priority` comments may lag live
`SELL_*` constants; `match_orders` rustdoc still describes const defaults (live
uses `match_orders_with_coincidence`).

## MarketGood / AMV

Default AMV `1.0`, salability `0.4` (below exchange floor: **not** till money).
AMV / average_price never `0` (bounce). Salability clamp `0..=1`. Volume is
derived (`purchased + payment`). Day logic should go through setters.
`history()` snapshots AMV, salability, `purchased`, and `amv_trails`. Missing
salability `0.4`; missing prices `1.0`.

**Drift:** write live AMV; intra-day `buy` / `evaluate` / orders use frozen
`history()`. Accept: both sides lerp toward basket midpoint. Reject: sought
* 1.1 up, tenders down by units offered per unit sought. No-proposal: sought
up only. End of day: leftover buys raise, leftover sells lower, larger leftover
wins, scaled by `unsatisfied / (unsatisfied + purchased)`. `set_amv` does not
push the ring.

**Code:** `market.rs`, `marketorder.rs`; tunables `factuals.config.market` /
`market_priority`.
