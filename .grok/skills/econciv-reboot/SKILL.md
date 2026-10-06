---
name: econciv-reboot
description: Working rules for JeremyDaug/SimplerEconomy on EconCiv-Reboot. Trigger on simpler_economy, market_day, pops, firms, desires, AMV, salability, households, PlayState, pop_tester, Overview.md, AGENTS.md, or any sim library change. Do not use for Bevy client UI or for EconCiv-Rework-Branch unless that branch is named.
---

# EconCiv Reboot

## Tree

- Owner/repo/branch — `JeremyDaug/SimplerEconomy`, `EconCiv-Reboot`. SHA was `bd60ac45` as of 2026-09-29. Confirm tip with GitHub before editing remote files.
- Design pin — `docs/Overview.md`. Working rules — repo `AGENTS.md`.
- Do not open the Obsidian vault or mine other branches unless the user names a note or target.
- Version cuts are unset. Do not invent a milestone and start building it. Record a cut in `docs/Overview.md` only after the user chooses it.
- `Unit` and `TechTree` stay empty until asked. Do not turn `Technology` into a beaker pile.

## What this crate is

Rust 2024 lib crate `simpler_economy`. No binary. No Bevy in simulation code. `std` collections. `hexx` for the hex map. Bring Bevy back only with a client.

World factuals live in `data/world`. `examples/rates_tester.rs` probes household demographics. `examples/pop_tester` loads `data/world` and `data/pop_tester/scenario.toml`, grants Time, and calls `Market::market_day`.

Money, time, land, and skills are goods with modifiers, not separate magic resources. Stored AMV may be negative and must not sit on zero (`AMV_EPSILON` in `market.rs`). Sentiment is a five-way partition. `work_time_fraction` stacks species base plus culture, stratum, and religion addends via `Factuals::work_time_fraction`.

## Live work (do not pretend it is finished)

Current surface is pop satisfy/consume, cottage jobs, and `Market::market_day` (pop-pop deals). Firm produce, reserve, and plan are empty. PlayState phase methods are stubs. Institution decay and institution record keeping still panic and are not called.

Read `docs/handoff/pops.md` before touching satisfy, consume, propose, evaluate, `match_deals`, jobs, or crafts. That note lists landed vs stub and known traps. Details that change often live in `references/live-surface.md`.

## Edit rules

- When the user names a file, edit only that file.
- Match the surrounding module. No drive-by formatting.
- A boolean checker that changes nothing is named as a question (`is_exchange_ok`, not `exchange_ok`).
- Reads from storage types (`Actors`, `Players`, `Factuals`, `MapData`, and the same kind of holder) go through a getter. If the getter is missing, add it and keep the expect or panic on that getter.
- Every function doc starts with a `#` header of the name. The doc says inputs, logic, and outputs. Small and private functions get that too.
- A step inside a function gets a one-line comment when the code does not already say it. `Market::market_day` labels each phase that way.
- When a comment already matches the code, add a link or connection. Leave the paragraph. If a comment looks wrong and the function was not just changed, ask before rewriting it.
- A comment states a fact the surrounding code owns. Put it on the type or function that decides it, once. Do not repeat it on other functions in the change, or on a caller that does not decide it.
- Whole goods use `trunc` or `floor` at the call. `whole_units_up` rounds a wage basket away from zero.

## Checks

- Library change — `cargo test --lib`
- Household probe — `cargo run --example rates_tester`
- Market day smoke — `cargo run --example pop_tester` when that example is in scope

## What not to invent

No abstract production currencies. No second inter-regional market. No slot-based buildings. No leftover-AMV retune unless asked. Do not sync firm reserve to `reserve_target` after every qty change if that work comes back.

## Agents

In-repo Cursor agents are `.grok/agents/design.md` (read-only) and `.grok/agents/simpler.md` (implementer). They are not Grok Bots. Session roles are in `AGENTS.md`. Do not create a Grok Bot unless the user asks for a new one.
