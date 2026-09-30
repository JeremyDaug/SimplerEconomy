# Live surface on EconCiv-Reboot

Confirm against the branch tip. This note tracked SHA `bd60ac45` (2026-09-29).

## Landed

- `Pop::satisfy` reserves free stock in tier order and stops on the first target it cannot fill. Bookmark is written. Calling `satisfy` again from the top can double-reserve a half-filled cap.
- `Pop::satisfy_continue` resumes from that bookmark. After new stock arrives inside a day, `reevaluate` uses this, not a fresh `satisfy`.
- `Pop::consume` / `consume_tier` / `consume_one_desire` run after exchange in `Market::market_day`. Consume ignores satisfaction already recorded and can count a level twice if both satisfy and consume run. That is the current day path.
- `Market::match_deals` pairs on one good. Buyer proposes, seller accepts or rejects. Only an accepted basket moves goods and pays freight. Both sides reevaluate after.
- `Market::market_day` order — reserve, produce, `match_deals`, consume, decay, actor books and planning, night card.
- Night `record_keeping` writes trade pressure, production flow, rot, and salability drift. Deals do not write the published AMV/salability card during the day. History snapshots the card at the start of `match_deals`.

## Empty or stub

- `Firm::reserve_for_day`, `Firm::produce`, `Firm::plan`, `Pop::plan`
- `PlayState` phase methods including intramarket and pop consumption
- Class desires
- `Desire.decay` field exists; nothing multiplies satisfaction by it
- Savings between tiers and between luxury iterations
- `Unit`, `TechTree`
- Institution decay / institution record keeping panic if called

## Deal rules that bite

- Holding value of a positive AMV is `amv * amv_scale(salability)`. Negative AMV is not scaled.
- Goods that still feed the open (lowest) tier stay off the payment book.
- With a satisfaction gain, cost may run up to `credit * LOSS_LIMIT` (`4`).
- Propose does not see the seller's desires. A seller who still needs the match good rejects.
- Freight is `transaction_cost + bulk * market friction`, or 0 when the world has no transport good. Freight is inside the same goods map, not a second list.

## Tests and examples

- `cargo test --lib`
- `examples/pop_tester` — two pops, one market, `Market::market_day`
- `examples/rates_tester` — household demographics
