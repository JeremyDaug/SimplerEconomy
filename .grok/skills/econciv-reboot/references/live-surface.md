# Live surface on EconCiv-Reboot

Confirm against the branch tip. Refreshed 2026-09-30 for cottage jobs and the file-loaded pop tester.

## Landed

- `Pop::satisfy` reserves free stock in tier order and stops on the first target it cannot fill. Bookmark is written. Calling `satisfy` again from the top can double-reserve a half-filled cap.
- `Pop::satisfy_continue` resumes from that bookmark. After new stock arrives inside a day, `reevaluate` uses this, not a fresh `satisfy`.
- `Pop::consume` / `consume_tier` / `consume_one_desire` run after exchange in `Market::market_day`. Consume ignores satisfaction already recorded and can count a level twice if both satisfy and consume run. That is the current day path.
- `Market::match_deals` pairs on one good. Buyer proposes, seller accepts or rejects. Only an accepted basket moves goods and pays freight. Both sides reevaluate after.
- `Market::market_day` order — reset, reserve, produce, `match_deals`, consume, decay, actor books and planning, night card. A pop's reserve is satisfy, then `apply_craft`, then the job reserve.
- A pop `Job` plans, reserves, produces, and shops on the pop's stock. Craft `0` is no baseline; lines still run. An empty line list skips the work. `Pop::plan` rewrites every line target after decay. `Pop::complexity_cost` is available and does not scale iterations.
- `examples/pop_tester` loads `data/pop_tester/scenario.toml` and calls `Actors::start_day` before `market_day`. The scenario's line targets are the first morning. The night plan replaces them.
- Night `record_keeping` writes trade pressure, production flow, rot, and salability drift. Deals do not write the published AMV/salability card during the day. History snapshots the card at the start of `match_deals`.

## Empty or stub

- `Firm::reserve_for_day`, `Firm::produce`, `Firm::plan`
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
- `examples/pop_tester` — scenario file, one market, `Actors::start_day`, then `Market::market_day`
- `examples/rates_tester` — household demographics
