# Simpler Economy

Design source of truth is `Design_V1_Market_Game.md`, the V1 market game. The user owns that file. Do not edit it unless asked. Plain statements in it are decided. An **Open question** with a **Proposal** is not decided; build to the proposal only when the work needs it, and say so.

`TODO.md` is the V1 build order from that doc. Work one step at a time, and do not start a step until the one before it passes its milestone.

## Scope

Do not open the Obsidian vault, and do not mine other git branches, unless the user names a specific note or target.

Do not invent a milestone. The milestones are the ones in the design doc's Build Order and Test Milestones. Later versions are in its Roadmap; do not pull them into V1.

## This tree

Rust 2024 library, crate `simpler_economy`. No library binary. No Bevy in simulation code. Use `std` collections. V1 has one market and no map. Bring Bevy back with a client.

World data is `data/world/goods.toml`, `processes.toml`, and `config.toml`. Goods and processes are the live systems. Their loaders are `Good::load_from_toml` and `Process::load_from_toml`. `Factuals` loads a world folder from those, then config. The load note is `docs/handoff/loaders.md`.

`Household` stays whole. Other game types keep their fields and stay method stubs. Do not fill a stub unless asked. `TechTree` stays empty until its build step.

The pop tester is `examples/pop_tester`. It runs one pop through the turn.

Whole goods use `trunc` or `floor` at the call. `whole_units_up` rounds a wage basket away from zero.

## Checks

- `cargo test --lib` for library changes.

Match the surrounding module. Follow the style rules below. No drive-by formatting. When the user names a file, edit only that file.

## Style

A boolean checker that changes nothing is named as a question. `exchange_ok` is `is_exchange_ok` or `is_valid_exchange`.

Reads from storage types (`Actors`, `Factuals`, and the same kind of holder) go through a getter. If the getter is missing, add it and keep the expect or panic on that getter. Call sites use the getter.

### Comments

Every function doc starts with a `#` header of the name. The header may expand the name: `get_sol` can be `# Get Standard of Living`.

The doc says the inputs, the logic, and the outputs. Small and private functions get that doc too.

A step inside a function gets a one-line comment of what that step does when the code does not already say it. `Factuals::load_from_dir` names each file it reads that way. Longer notes stay on the parts that are hard to follow.

When a comment already matches the code, add a link, a highlight, or a connection. Leave the paragraph in place. If a comment looks unclear or wrong and the function was not just changed, ask before rewriting it. When the function changed and the comment no longer matches, edit the comment to match the new logic.

A comment states a fact the surrounding code owns. Put that fact on the type or function that decides it, once. Other functions in the same change, and callers that do not decide it, do not repeat it.

## Session roles

These are session hats, not standing Grok Bots. Default is Implementer.

Name a role in the first message: Design, Implementer, or Reviewer. One role at a time.

- Design — read-only. `Design_V1_Market_Game.md` first, then `TODO.md`. Conflicts between the doc and the code get listed, not resolved by inventing mechanics. No file edits.
- Implementer — named files only. This style section. `cargo test --lib` after a library change.
- Reviewer — no new features. Simplify, refine, debug. Cite the design doc. Edits only when asked to apply a specific fix.

Cursor copies live in `.grok/agents/`. The working skill lives in `.grok/skills/econciv-reboot/`.
