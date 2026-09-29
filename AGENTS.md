# Simpler Economy

Design source of truth is `docs/Overview.md`. Read it before changing design.

## Scope

The market is the keystone. Goods are concrete. Money, time, land, and skills are goods with modifiers, not separate magic resources. Production, consumption, and logistics are simulated. The player massages them.

Do not open the Obsidian vault, and do not mine other git branches, unless the user names a specific note or target.

Version cuts are not written yet. Do not invent a milestone and start building it. Record a cut in `docs/Overview.md` only after the user chooses it.

## This tree

Rust 2024 library, crate `simpler_economy`. No binary. No Bevy in simulation code. Use `std` collections. `hexx` is the hex map. Bring Bevy back with a client.

World factuals live in `data/world`. `examples/rates_tester.rs` probes household demographics.

Stored AMV may be negative and must not sit on zero (`AMV_EPSILON` in `market.rs`). Sentiment is a five-way partition. `work_time_fraction` lives on species (base), culture, and religion (addends), stacked by `Factuals::work_time_fraction`.

`Unit` and `TechTree` are later features. Leave them empty until asked. Do not turn `Technology` into a beaker pile.

Whole goods use `trunc` or `floor` at the call. `whole_units_up` rounds a wage basket away from zero.

## Checks

- `cargo test --lib` for library changes.
- `cargo run --example rates_tester` for the household probe.

Match the surrounding module. Follow the style rules below. No drive-by formatting. When the user names a file, edit only that file.

## Style

A boolean checker that changes nothing is named as a question. `exchange_ok` is `is_exchange_ok` or `is_valid_exchange`.

Reads from storage types (`Actors`, `Players`, `Factuals`, `MapData`, and the same kind of holder) go through a getter. If the getter is missing, add it and keep the expect or panic on that getter. Call sites use the getter.

### Comments

Every function doc starts with a `#` header of the name. The header may expand the name: `get_sol` can be `# Get Standard of Living`.

The doc says the inputs, the logic, and the outputs. Small and private functions get that doc too.

A step inside a function gets a one-line comment of what that step does when the code does not already say it. `Market::market_day` labels each phase that way. Longer notes stay on the parts that are hard to follow.

When a comment already matches the code, add a link, a highlight, or a connection. Leave the paragraph in place. If a comment looks unclear or wrong and the function was not just changed, ask before rewriting it. When the function changed and the comment no longer matches, edit the comment to match the new logic.
