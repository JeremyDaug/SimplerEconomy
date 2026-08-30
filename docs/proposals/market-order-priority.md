# Market order priority

**Status:** numbers, names, the field on `MarketOrder`, and `Market::match_orders` are in. Receiving-market rank set, offer generation, and deal execution are not.  
**Code:** `MarketOrder.priority`, `config::market_priority`, `StateMarketSlot`, `MarketSlot::priority`, `priority_in_band` / `pop_priority_from_rank` / firm rank helpers, `Market::match_orders`.

Lower number goes first. Equal numbers are meant to be broken at random when matching.

**Wealth used for ranking:** total AMV per household (`records.wealth_amv / household count`). That is **wealth** (passive stock), not **rich** / liquid wealth. Pops, not individuals.

## Timeline (what the numbers mean)

| Priority | Who | Notes |
|----------|-----|--------|
| `0.0` | State `First` | Before everyone |
| `1.0` | Institution `BeforeFirms` | One of three institution options |
| `1.5` | State `BeforeFirms` | After that institution slot, before firms |
| `[2.0, 2.49)` | Merchant / trader firms | Default sits at `2.0` until wealth-ranked. Lerp exclusive end is the state slot |
| `2.49` | State `AfterMerchants` | `FIRM_MERCHANT_END - STATE_FIRM_SLOT_MARGIN` |
| `[2.5, 2.99)` | Producer firms | Default sits at `2.5` until wealth-ranked. Lerp exclusive end is the state slot |
| `2.99` | State `AfterProducers` | `FIRM_PRODUCER_END - STATE_FIRM_SLOT_MARGIN` |
| `3.0` | Institution `BetweenFirmsAndPops` | Not between the two firm bands |
| `3.1` | State `AfterFirms` | After that institution slot, before pops |
| `[4.0, 5.0)` | Pops | Ranked by wealth per household; unranked orders sit at `4.0` |
| `5.0` | Institution `AfterPops` | After the pop band |
| `5.1` | State `Last` | After that institution slot |

There is no state-among-pops slot. A state does not insert inside `[4, 5)` unless it later gains an arbitrary-within-band control.

Restrictions are only forced on **pops** (`[4, 5)`) and **firms** (`[2, 3)`). Institutions and states may insert more freely; the table is the intended menu, not a hard check. The pop/firm band `debug_assert` lives in `assert_priority_for_origin`, which is compiled out of release.

## In now

- `f64` `priority` on every `MarketOrder`.
- Named constants in `market_priority`.
- `StateMarketSlot` for the six player inserts.
- `MarketSlot::priority` maps the existing institution enum (`1` / `3` / `5`).
- Rank helpers are lerps of `unit_rank` in `[0, 1)` (`0` = first / richest in the band).
- `wealth_unit_rank` / `pop_priority_from_wealth`: `1 - wealth / max_wealth` against the richest per-household total AMV.
- Firm rank helpers lerp toward the matching state slot and never reach it, so `AfterMerchants` / `AfterProducers` stay after every ranked firm in that band.
- `Pop::create_orders` writes `POP_START` (`4.0`) as an unranked placeholder.
- `Market::match_orders`: one pass over the **front** buy-priority group (shuffled). At most **one** match so later buyers cannot jump the queue. Every hopeless buy in that group (no other-origin seller) is listed in `unmatched_buys` so the caller can update them while the one deal runs. Coincidence still doubles sell weight for that pick only. Lists are not mutated.
- `compose_sell_priority(actor_band, supply, successful_sells) = 1/max(actor_band, SELL_ACTOR_PRIORITY_FLOOR) + sqrt(supply) + SELL_SUCCESS_BONUS * fills`. Write that on sell/offer `priority`. `add_sell_success_bonus` is the flat add after a fill.

## Not in (on purpose)

- **Setting wealth ranks when a market receives orders.** Pops must not compute their own `[4, 5)` decimal. The market that takes the orders should rank member pops by wealth per household and call `set_priority` / `with_priority`. Same idea for firms if we use the optional firm wealth bands.
- **Merchant vs producer classification.** No firm kind flag yet. Until that exists, a firm that needs a priority should be handed `FIRM_MERCHANT` or `FIRM_PRODUCER` by its caller.
- **Subordinated firms.** Firms owned or directed by a state or institution should have their band chosen for them. Not wired.
- **Institution / state order creation.** `MarketSlot` and `StateMarketSlot` exist; nobody emits buy/sell orders from them yet, and nothing copies `institution.market_slot.priority()` onto an order.
- **State purchase buckets.** Players should be able to split military vs construction vs welfare across different slots. `MarketSlot::Custom` was reserved for split queues and still has no real mapping (placeholder = between firms and pops).
- **Sell-side update after partial fill.** Success bonus is a flat add. Recomputing `sqrt(supply)` when the remaining offer shrinks is the caller's choice. Marketing adds are later.
- **Deal execution.** Matcher only returns indices. Fill, AMV, payment, and book updates are the caller.
- **Culture / rule reversals.** Vault allows reversing rich vs poor, merchants vs producers, or squeezing institutions between the two firm bands. The current numbers do **not** let institutions sit between `2.0` and `2.5`. Do not add that without a new slot.
- **Tighter-than-margin enforcement.** Ranked firms already cannot reach the state slot. If a later insert needs to sit after *unranked* firms packed at band start only, no extra epsilon is required. If we ever lerp firms all the way to `FIRM_*_END` again, revisit `STATE_FIRM_SLOT_MARGIN` or reserve an exclusive tail some other way.

## Rank mapping

Per-household total AMV (`wealth_amv / household count`) against the richest actor in that band:

```text
unit_rank = 1 - wealth / max_wealth    // richest -> 0, poorest -> just below 1
priority  = lerp(start, end, unit_rank)
```

`max_wealth <= 0` (empty or worthless market) -> everyone `0.0`. Curve grading to flatten a skewed wealth spread is later.

- Pops: `pop_priority_from_wealth` -> `[4, 5)`
- Merchants: `firm_merchant_priority_from_rank` -> `[2, 2.49)`
- Producers: `firm_producer_priority_from_rank` -> `[2.5, 2.99)`
