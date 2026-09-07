# Pops

Read this only for consume, shop/save, desires, sentiment, or player-resource
extract. Names: vocabulary (desire hierarchy, tier sat, consume need, savings
ratio, reserved, sentiment). Household primer:
`docs/proposals/household-population-refactor-primer.md`.

## Landed vs stub

| Piece | Status |
|-------|--------|
| Consume / growth / sentiments / record keeping / decay | Closed on `Pop`. PlayState wires these as they mature |
| `create_orders` | Three passes, whole-unit **requests**. Used by `run_market_day` |
| `start_day` | Exists. Tester uses it. PlayState day-start still stub |
| `extract_special_resources` | First pass exists. Yield is **not** routed onto `State.resources` |
| `next_shopping_trip` | `todo!()` |
| Pop offers | Not generated |
| Shop ambition / looping luxury | Not started. Staple `Desire.amount` stays fixed |
| Class demographics | Unimplemented (vault: park this) |
| Migration leaves | Orchestrator exists; leaves are `todo!()` |

Do **not** re-add `DemoRow.rates`. Resolve via `Factuals::get_demographic_rates`
(recompute-per-call is intentional). Job multiplies pops; rate keys are
demographic ids only. Do not reopen the household-rates model.

## Traps

- `record_keeping` writes next-day shop/save. Morning `update_desires` does
  **not** re-scale them for `previous_growth`.
- Consume need = `max(unsatisfied target units, consumed + used)`. Shop target
  (tradeable) = consume need + save target.
- Savings ratio is **days of buffer**, not a share of leftover liquid wealth.
  Save pile does not shrink on decline.
- Reserved is never negative. Extra luxury consume eats unreserved stock.
- Shop ambition does **not** scale with wealth or shop fill. `create_orders`
  does not loop extra staple buys.
- `create_orders` passes: desire shop, parked non-desire shop, opportunistic
  extra. Skip amounts `< 1` and `unavailable` goods.
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
