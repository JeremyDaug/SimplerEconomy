# Simpler Economy

Design source of truth is `Design_V3.md`. The user owns that file. Do not edit it unless asked. It takes precedence over older design notes. Read `docs/handoff/current.md` before editing the library.

`docs/Overview.md` and `TODO.md` describe the previous tree. They are not the current cut.

## Scope

Do not open the Obsidian vault, and do not mine other git branches, unless the user names a specific note or target.

Do not invent a milestone. Do not start work from the version list in `docs/Overview.md`. Record a cut only after the user writes it.

## This tree

Rust 2024 library, crate `simpler_economy`. No binary. No Bevy in simulation code. Use `std` collections. `hexx` is the hex map. Bring Bevy back with a client.

World data is `data/world/goods.toml`, `processes.toml`, and `config.toml`. Goods and processes are the live systems. Their loaders are `Good::load_from_toml` and `Process::load_from_toml`. `Factuals` loads a world folder from those, then config. The load note is `docs/handoff/loaders.md`.

`Household` stays whole. Other game types keep their fields and stay method stubs. Do not fill a stub unless asked. `Unit` and `TechTree` stay empty.

There is no example binary.

Whole goods use `trunc` or `floor` at the call. `whole_units_up` rounds a wage basket away from zero.

## Checks

- `cargo test --lib` for library changes.

Match the surrounding module. Follow the style rules below. No drive-by formatting. When the user names a file, edit only that file.

## Style

A boolean checker that changes nothing is named as a question. `exchange_ok` is `is_exchange_ok` or `is_valid_exchange`.

Reads from storage types (`Actors`, `Players`, `Factuals`, `MapData`, and the same kind of holder) go through a getter. If the getter is missing, add it and keep the expect or panic on that getter. Call sites use the getter.

### Comments

Every function doc starts with a `#` header of the name. The header may expand the name: `get_sol` can be `# Get Standard of Living`.

The doc says the inputs, the logic, and the outputs. Small and private functions get that doc too.

A step inside a function gets a one-line comment of what that step does when the code does not already say it. `Factuals::load_from_dir` names each file it reads that way. Longer notes stay on the parts that are hard to follow.

When a comment already matches the code, add a link, a highlight, or a connection. Leave the paragraph in place. If a comment looks unclear or wrong and the function was not just changed, ask before rewriting it. When the function changed and the comment no longer matches, edit the comment to match the new logic.

A comment states a fact the surrounding code owns. Put that fact on the type or function that decides it, once. Other functions in the same change, and callers that do not decide it, do not repeat it.

## Session roles

These are session hats, not standing Grok Bots. Default is Implementer.

Name a role in the first message: Design, Implementer, or Reviewer. One role at a time.

- Design — read-only. `Design_V3.md` first, then `docs/handoff/current.md`. Conflicts with older notes get listed, not resolved by inventing mechanics. No file edits.
- Implementer — named files only. This style section. `cargo test --lib` after a library change.
- Reviewer — no new features. Simplify, refine, debug. Cite `docs/handoff/current.md`. Edits only when asked to apply a specific fix.

Cursor copies live in `.grok/agents/`. The working skill lives in `.grok/skills/econciv-reboot/`.
