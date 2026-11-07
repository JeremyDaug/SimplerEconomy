---
name: econciv-reboot
description: Working rules for JeremyDaug/SimplerEconomy. Trigger on simpler_economy, goods, processes, factuals, Design_V3, Overview.md, AGENTS.md, or any sim library change. Do not use for a Bevy client.
---

# Simpler Economy

## Tree

- Design pin — `Design_V3.md`. The user owns it. Do not edit it. It wins over older notes where they disagree.
- Working note — `docs/handoff/current.md`. Load path — `docs/handoff/loaders.md`.
- `docs/Overview.md` and `TODO.md` describe the previous tree. Do not restore that behavior from them.
- Working rules — repo `AGENTS.md`.
- Do not open the Obsidian vault or mine other branches unless the user names a note or target.
- Do not invent a milestone. Do not start work from the version list in `docs/Overview.md`.
- `Unit` and `TechTree` stay empty until asked.

## What this crate is

Rust 2024 lib crate `simpler_economy`. No binary. No Bevy in simulation code. No examples. `std` collections. `hexx` for the hex map. Bring Bevy back only with a client.

World data is `data/world/goods.toml`, `processes.toml`, and `config.toml`. Goods and processes are the live systems. Their loaders are `Good::load_from_toml` and `Process::load_from_toml`. `Factuals` loads a world folder from those, then config. Details are in `docs/handoff/loaders.md`.

`Household` stays whole. Other game types keep their fields and are method stubs. Do not fill a stub unless asked. `process.rs` stays as it is until a pass is asked for.

## Edit rules

- When the user names a file, edit only that file.
- Match the surrounding module. No drive-by formatting.
- A boolean checker that changes nothing is named as a question (`is_exchange_ok`, not `exchange_ok`).
- Reads from storage types (`Actors`, `Players`, `Factuals`, `MapData`, and the same kind of holder) go through a getter. If the getter is missing, add it and keep the expect or panic on that getter.
- Every function doc starts with a `#` header of the name. The doc says the inputs, the logic, and the outputs. Small and private functions get that doc too.
- A step inside a function gets a one-line comment when the code does not already say it.
- When a comment already matches the code, add a link or connection. Leave the paragraph. If a comment looks wrong and the function was not just changed, ask before rewriting it.
- A comment states a fact the surrounding code owns. Put it on the type or function that decides it, once. Do not repeat it on other functions in the change, or on a caller that does not decide it.
- Whole goods use `trunc` or `floor` at the call. `whole_units_up` rounds a wage basket away from zero.

## Checks

- Library change — `cargo test --lib`

## What not to invent

Do not invent mechanics `Design_V3.md` does not state. Do not treat a silent gap as permission to copy `docs/Overview.md`. The open list is in `docs/handoff/current.md`.

## Agents

In-repo agents are `.grok/agents/design.md` (read-only), `.grok/agents/simpler.md` (implementer), and `.grok/agents/review.md`. They are not Grok Bots. Session roles are in `AGENTS.md`. Do not create a Grok Bot unless the user asks for a new one.
