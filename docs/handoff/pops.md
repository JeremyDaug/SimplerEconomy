# Pops

Read this only for consume, shop/save, desires, sentiment, or player-resource
extract. Names: vocabulary (desire hierarchy, tier sat, consume need, savings
ratio, reserved, sentiment). Household primer:
`docs/proposals/household-population-refactor-primer.md`.

## Landed vs stub

| Piece | Status |
|-------|--------|
| Consume / growth / sentiments / record keeping / decay | Closed on `Pop`. Consume eats the pop bag only. A higher tier waits until every lower tier is complete. An empty tier counts as complete. A firm's shelf is not dinner. PlayState wires these as they mature |
| Household work | `Pop::run_household_work`. Recipes are `(process id, iteration cap)` on `household_work`, basket then specialty. Not a firm line. Spends unreserved quantity and Time. Outputs land in `quantity`. Reserved stock is not an input. Basket cap is `pop_constants::HOUSEHOLD_BASKET_CAP` (3). Specialty cap is the init firm's target. Init attaches subsistence 29/30/31 then that firm's process |
| `create_orders` | Shop-plan **requests** (ceil) then leftover **offers** (floor). Higher consume tier only if the wallet covers the lower one. Tender freeze covers posted request AMV. After cover, at least `TENDER_WALLET_FLOOR` (0.25) of leftover free units stay unlisted |
| `start_day` | Exists. Tester uses it. PlayState day-start still stub |
| `extract_special_resources` | First pass exists. Yield is **not** routed onto `State.resources` |
| Extra desire buys | Wait for a later morning `create_orders`. No intra-day shopping trip |
| Pop offers | Morning leftover after tender cover. Named `counter_offer` good only (no AMV, no amount) |
| Shop ambition / looping luxury | Record keeping adds one extra luxury level. Leftover liquid above save may top up the cheapest luxury shop, capped at one extra level of that good. Staple `Desire.amount` stays fixed. `create_orders` does not extra-walk luxury |
| Class demographics | Unimplemented (vault: park this) |
| Migration leaves | Orchestrator exists; leaves are `todo!()` |

Do **not** re-add `DemoRow.rates`. Resolve via `Factuals::get_demographic_rates`
(recompute-per-call is intentional). Job multiplies pops; rate keys are
demographic ids only. Do not reopen the household-rates model.

## Traps

- `record_keeping` writes next-day shop/save from **post-decay** stock
  (tester day order). Planning before 100% decay fences leftover as need and
  kills the sell book. Morning `update_desires` does **not** re-scale shop
  for `previous_growth`.
- Consume need spends on-hand along `ordered_targets`, then **splits** leftover
  sat equally across remaining buyable substitutes (caps still apply). Shop
  target (tradeable) = consume need + save target.
- Savings ratio is **days of buffer**, not a share of leftover liquid wealth.
  Save pile does not shrink on decline.
- Reserved is never negative. Extra luxury consume eats unreserved stock.
- Household work runs after the morning reservation, and
  `reserve_for_desires` runs again after each recipe. New output covers
  one desire level before the next recipe or the market can take it.
  A good has one output floor, shared by every recipe that makes it.
  The higher profit ratio takes that floor. If that recipe's output is
  worth more than its inputs, it also runs its stored cap for sale.
  A worse recipe for the same good stays at 0. Inputs of a planned
  recipe raise the floor of the good they consume. Time the morning
  shop needs for the wagon stays in the bag: one transaction cost per
  good still short of its shop target, plus the bulk of those units and
  of the surplus that pays for them. Time still left, which would decay
  in full, is spent on the best recipe that can still run.
  Surplus above the reserve is what `create_orders` can sell or tender.
  It does not call `Firm::plan`.
- Consume runs a tier only after every lower tier is complete. An empty
  tier counts as complete. The morning earmark stops at the first tier
  stock cannot fill. Shop and save stop at the first tier that is not
  satisfied, judged after consume, so eaten staples still restock and the
  next open tier is still bought. A higher tier is not earmarked, shopped,
  or used as the savings pile.
- Shop ambition: the open tier and every lower one get spread consume
  need. Luxury leftover dump only when luxury is that open tier, and only
  if leftover AMV remains (none after 100% decay). `Desire.amount` does
  not rise.
- The market day posts firm orders once. A pop offers surplus above
  savings and the consume reserve, then one buy for an open-tier good
  somebody is already selling. After that buy fills or closes, the pop
  looks again while the flat door fee is still payable. Savings is not
  tendered for that loop.
- `create_orders`: basic shop, parked save, then common, then luxury.
  A higher consume tier is posted in full only when remaining budget covers
  **all** of that tier; otherwise walk until overdraw and skip the next.
  Skip `unavailable` on requests. Extra desire buys wait for a later morning
  book.
- Buy stop (`PopRecords::buy_stop`) after the market day: `market` if
  remaining shop is unavailable, `money` if shop remains and free AMV is
  gone, `transport` if the door cannot be paid. `None` if shop is filled.
- Tender freeze: salability first, then lowest desire importance. Listed
  offer units (and `reserved`) are not tenderable. Sell size floors; buy size
  ceils. Offers name the first remaining request as `counter_offer`; requests
  name the most salable free good. No AMV target or counter amount. Save
  AMV is scaled by durability (`1 - decay_rate`); decay 1.0 goods get no
  save. Liquid ranks salability then durability.
- Planning only lerps savings ratio / time preference / risk appetite.
- `DemoDesire::create_desire` is the only demo-to-pop path (`derive_desire`
  folded in). It scales `amount` **and** additive effects (player resources,
  bonus goods). Birth/mortality/sentiment/satisfaction arms stay as demo rates.
  Harvest is sat times that baked magnitude — do **not** multiply by household
  count again.
- Target efficiency always **positive**. `debug_assert` only; do not also
  `continue` on `<= 0`.
- **Tier sat** is a **sum** of desire success rates + boosts, not an average.
  Mood may normalize by count; do not store that average as tier sat. Empty
  tier: treat as `1.0` when recording "no unmet needs."
- `update_sentiments` is after consume and growth; it does not apply growth
  arms or bonus goods.

`extract_special_resources`: demographic rates (species / culture / **religion**
via `find_religion`), living-well culture, SOL/mood legitimacy
(`FIRST + EXTRA * (n - 1)` over all desire tiers), desire effects, then drain
stored player-resource arms. `LUXURY_LEGITIMACY_RATE` is unused.

Stale (notify only): `update_desires` rustdoc still lists scaling `shop_target`
for growth; `savings_ratio` field still says "share of liquid wealth";
`decay_goods` still calls `saved` a wish target; vault `Pops.md` household
section still has a REWORK banner; `TODO.md` household-helper bullet lags the
landed rates model.

**Code:** `pop.rs`, `pop/orders.rs`, `pop/deal.rs`, `pop_property.rs`,
`desire.rs`, `household.rs`, `sentiment.rs`.
