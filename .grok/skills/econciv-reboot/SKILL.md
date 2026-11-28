---
name: econciv-reboot
description: Working rules for JeremyDaug/SimplerEconomy. Trigger on simpler_economy, goods, processes, factuals, Design_V1_Market_Game, TODO.md, AGENTS.md, or any sim library change. Do not use for a Bevy client.
---

# Simpler Economy

## Tree

- Design pin — `Design_V1_Market_Game.md`, the V1 market game. The user owns it. Do not edit it. Open questions and their proposals in it are not decided.
- Build order — `TODO.md`, taken from the pin. Load path — `docs/handoff/loaders.md`.
- Working rules — repo `AGENTS.md`.
- Do not open the Obsidian vault or mine other branches unless the user names a note or target.
- Do not invent a milestone. Work the `TODO.md` steps in order. Later versions are in the pin's Roadmap.
- `TechTree` stays empty until its build step.

## What this crate is

Rust 2024 lib crate `simpler_economy`. No library binary. No Bevy in simulation code. The pop tester example is `examples/pop_tester`. `std` collections. V1 has one market and no map. Bring Bevy back only with a client.

World data is `data/world/goods.toml`, `processes.toml`, and `config.toml`. Goods and processes are the live systems. Their loaders are `Good::load_from_toml` and `Process::load_from_toml`. `Factuals` loads a world folder from those, then config. Details are in `docs/handoff/loaders.md`.

`Household` stays whole. Other game types keep their fields and are method stubs. Do not fill a stub unless asked. `process.rs` stays as it is until a pass is asked for.

## Edit rules

- When the user names a file, edit only that file.
- Match the surrounding module. No drive-by formatting.
- A boolean checker that changes nothing is named as a question (`is_exchange_ok`, not `exchange_ok`).
- Reads from storage types (`Actors`, `Factuals`, and the same kind of holder) go through a getter. If the getter is missing, add it and keep the expect or panic on that getter.
- Every function doc starts with a `#` header of the name. The doc says the inputs, the logic, and the outputs. Small and private functions get that doc too.
- A step inside a function gets a one-line comment when the code does not already say it.
- When a comment already matches the code, add a link or connection. Leave the paragraph. If a comment looks wrong and the function was not just changed, ask before rewriting it.
- A comment states a fact the surrounding code owns. Put it on the type or function that decides it, once. Do not repeat it on other functions in the change, or on a caller that does not decide it.
- Whole goods use `trunc` or `floor` at the call. `whole_units_up` rounds a wage basket away from zero.

## Checks

- Library change — `cargo test --lib`

## What not to invent

Do not invent mechanics `Design_V1_Market_Game.md` does not state. Do not treat a silent gap as permission to copy earlier versions from git history. The open list is the pin's Open Questions (Summary).

## Agents

In-repo agents are `.grok/agents/design.md` (read-only), `.grok/agents/simpler.md` (implementer), and `.grok/agents/review.md`. They are not Grok Bots. Session roles are in `AGENTS.md`. Do not create a Grok Bot unless the user asks for a new one.
