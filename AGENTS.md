# Simpler Economy reboot

Branch `EconCiv-Reboot`. Design source of truth is `docs/Overview.md`. Read it before changing design or deleting a system.

## Scope

The market is the keystone. Goods are concrete. Money, time, land, and skills are goods with modifiers, not separate magic resources. Production, consumption, and logistics are simulated. The player massages them.

Do not open the Obsidian vault, and do not mine other git branches, unless the user names a specific note or target.

Notes from before this reboot describe `market_tester`, `pop_tester`, `data/init`, `docs/handoff`, and a five-pop village roster. Those are not in this tree. Do not restore them.

Version cuts are not written yet. Do not invent a milestone and start building it. Record a cut in `docs/Overview.md` only after the user chooses it.

## This tree

Rust 2024 library, crate `simpler_economy`. No binary. No Bevy. Game logic uses `std` collections. Bring Bevy back with a client, and do not use Bevy types in simulation code unless there is no alternative. `hexx` stays for the hex map, without the Bevy feature.

Kept material: goods, processes, household, species, culture, religion, desires, effects, and the map (`map`, `region`, `tile`, `plot`). World factuals live in `data/world`. `examples/rates_tester.rs` probes household demographics.

Pop, firm, and market keep their names. Shopping, order priority, firm planning, wage settlement, and the salability/AMV update rules were stripped. Do not restore them from memory or from an old branch. Stored AMV may be negative and must not sit on zero (`AMV_EPSILON` in `market.rs`). Sentiment keeps its five-way partition. How sentiment changes other behavior is unwired on purpose.

`PopRecords` is an empty placeholder. A pop's job is a later primitive process, noted on `Pop`, not a field. Firm parent, children, and level stay. `work_time_fraction` lives on species (base), culture, and religion (addends), stacked by `Factuals::work_time_fraction`.

`PlayState::advance_turn` is a phase sketch full of `todo!`. Do not fill it unless asked. There is no binary. `src/main.rs` was the empty hex scaffold and is gone.

`Unit` (`src/game/unit.rs`) and `TechTree` (`src/game/techtree.rs`, with `Technology` in `src/game/tech.rs`) are intended later features. They are empty on purpose. Do not delete them for being unused, and do not fill them in unless asked. Do not turn `Technology` into a beaker pile. How the tech tree sits beside decentralized innovation is not designed yet.

`Contract` stays on `Firm` as an empty placeholder. `FirmOrganization` stays on `Firm` with no fields. The old price, sharing, and control weights are gone. Do not put them back until company rules are written.

`src/game/init.rs` is a placeholder. Scenario kickoff is not loaded. Do not delete the module.

`TIME_PER_LABOR` and `Process::recipe_profit_ratio` stay for the first market tests. Do not delete them for being unread. Whole goods use `trunc` or `floor` at the call. `whole_units_up` remains for wage baskets that round away from zero.

## Removal

The user is thinning this fork. Delete a piece when nothing references it, or when they say it is out. A live system that the overview does not mention yet stays until they reject it or a pass shows it is unused.

Prefer deleting tests that assert removed behavior over rewriting them back into the old design.

When the user names a file, edit only that file.

## Checks

- `cargo test --lib` for library changes.
- `cargo run --example rates_tester` for the household probe.

Match the surrounding module. Comments only for a non-obvious constraint. No drive-by formatting.
