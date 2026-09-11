# Pops

Read this only for consume, shop/save, desires, sentiment, or player-resource
extract. Names: vocabulary (desire hierarchy, tier sat, consume need, savings
ratio, reserved, sentiment). Household primer:
`docs/proposals/household-population-refactor-primer.md`.

## Landed vs stub

| Piece | Status |
|-------|--------|
| Consume / growth / sentiments / record keeping / decay | Closed on `Pop`. Consume always eats common; luxury waits on basic. PlayState wires these as they mature |
| `create_orders` | Shop-plan **requests** (ceil) then leftover **offers** (floor). Higher consume tier only if the wallet covers the lower one. Tender freeze covers posted request AMV |
| `start_day` | Exists. Tester uses it. PlayState day-start still stub |
| `extract_special_resources` | First pass exists. Yield is **not** routed onto `State.resources` |
| `next_shopping_trip` | Solidify, then at most one request and one offer (full remaining size). Open/parked request => offer only. `run_market_day` waves call it |
| Pop offers | Morning leftover after tender cover. Named `counter_offer` good only (no AMV, no amount). Trip adds at most one more good |
| Shop ambition / looping luxury | Record keeping adds one extra luxury level and leftover liquid above save onto luxury shop. Staple `Desire.amount` stays fixed. `create_orders` does not extra-walk luxury. Trip can post one extra desire request |
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
- Consume always runs common (eat on-hand even if basic is short). Luxury
  is skipped unless every basic desire has a full level. Empty tier counts
  as complete. Morning `create_orders` still gates posting a higher shop
  tier on the wallet covering the lower one.
- Shop ambition: all tiers get spread consume need. Luxury leftover dump
  only if leftover AMV remains (none after 100% decay). `Desire.amount`
  does not rise. Morning `create_orders` is what skips a higher tier when
  the wallet cannot cover the lower one.
- `create_orders`: basic shop, parked save, then common, then luxury.
  A higher consume tier is posted in full only when remaining budget covers
  **all** of that tier; otherwise walk until overdraw and skip the next.
  Skip `unavailable` on requests only. Extra desire buys are
  `next_shopping_trip`, not morning.
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
