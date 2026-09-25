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

Match the surrounding module. Comments only for a non-obvious constraint. No drive-by formatting. When the user names a file, edit only that file.
