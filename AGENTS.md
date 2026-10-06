# Simpler Economy

Design source of truth is `docs/Overview.md`. Read it before changing design.

## Scope

The market is the keystone. Goods are concrete. Money, time, land, and skills are goods with modifiers, not separate magic resources. Production, consumption, and logistics are simulated. The player massages them.

Do not open the Obsidian vault, and do not mine other git branches, unless the user names a specific note or target.

Version cuts are in `docs/Overview.md` under Versions. Working placement notes are in `TODO.md`. Do not invent a milestone. Record a new cut in `docs/Overview.md` only after the user chooses it. Do not start a later cut's work in the current tester.

Pop and firm testers do not use `PlayState`. `PlayState` starts in the Full Market cut (0.3.0).

## This tree

Rust 2024 library, crate `simpler_economy`. No binary. No Bevy in simulation code. Use `std` collections. `hexx` is the hex map. Bring Bevy back with a client.

World factuals live in `data/world`. `examples/rates_tester.rs` probes household demographics.

Stored AMV may be negative and must not sit on zero (`AMV_EPSILON` in `market.rs`). Sentiment is a five-way partition. `work_time_fraction` lives on species (base), culture, stratum, and religion (addends), stacked by `Factuals::work_time_fraction`.

`Unit` is a 0.4 type. Leave it empty until that cut. It stays thin until a client can show it.

`TechTree` starts in 0.2 and finishes in 0.3, with states and institutions. Leave it empty until 0.2. Do not turn `Technology` into a beaker pile.

Buildings and upkeep are goods, not a separate system. They are not a milestone.

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

A comment states a fact the surrounding code owns. Put that fact on the type or function that decides it, once. Other functions in the same change, and callers that do not decide it, do not repeat it.

## Session roles

These are session hats, not standing Grok Bots. Default is Implementer.

Name a role in the first message: Design, Implementer, or Reviewer. One role at a time.

- Design — read-only. Overview terms, matching code paths, conflicts, open decisions. No invented mechanics or version cuts. No file edits.
- Implementer — named files only. This style section. `cargo test --lib` after a library change.
- Reviewer — no new features. Simplify, refine, debug. Cite `docs/handoff/pops.md` on satisfy, consume, propose, evaluate, or `match_deals`. Edits only when asked to apply a specific fix.

Cursor copies live in `.grok/agents/`. The working skill lives in `.grok/skills/econciv-reboot/`.
