# SimplerEconomy — Project Rules

Rust economic / civilization simulator. Current work: `EconCiv-Rework-Branch`.
Prefer the rework design over older incomplete models.

## Conversation

Conversational, less clipped sentences, more verbose. Talk like a person.

## Session start

This file is already in context. Then open **only** what the task needs, in
this order:

1. [`docs/agent-handoff.md`](./docs/agent-handoff.md) — router (status + table)
2. The **one** matching `docs/handoff/` topic and the listed code
3. [`STYLE.md`](./STYLE.md) — if you will edit Rust, **before the first edit**
4. Grep [`docs/design-vocabulary.md`](./docs/design-vocabulary.md) for names
   you will use (do not read it cover to cover)
5. Matching EconCiv vault note — **only** if changing behavior or adding a system

Do not open sibling handoff topics, `TODO.md`, `reviewlog.md`, `README.md`,
the historical vault, or `docs/proposals/` unless a routing row names that file.
Do not list or glob `docs/handoff/`.

**Caps**

- **One** handoff topic unless the user named two systems.
- If the task is unclear, ask **one** question. Do not open more files to guess.
- If the user asked a question, answer from the router + one topic. Do not
  start coding unless they asked for a change.
- **Done** = the asked change, tests for that change, and (only if behavior
  landed) the matching topic file + Status line. Do not volunteer extra phases,
  extra files, comment rewrites, or `TODO.md` / `reviewlog.md` edits.
- Be conversational in replies. Do not pad context with extra file reads.

## Goals

- Simplified economy: goods, processes, markets, firms, pops.
- Pops demand via **desires** from species, culture, religion, and related
  demographics.
- **Factuals** (mostly static definitions) stay separate from **game state**.
- Playable and inspectable; clear data models over premature abstraction.

Long-form design is the EconCiv vault, not `README.md` / `TODO.md`.

## Vault

Primary: `/home/jeremy/Documents/Obsidian Vault/Game Ideas/EconCiv/`
(Economic Civilization, Desires, Pops, Goods, Processes, Market, Firms, Turns,
The Player, State, or Nation).

Historical: `/home/jeremy/Documents/Obsidian Vault/Game Ideas/Simlper Economy Simulator/`
— prefer EconCiv on conflict.

**Do not open the vault** for a bugfix, wire-up, or refactor that follows
existing behavior.

When **changing behavior or adding a system**:

1. Read the matching EconCiv note first.
2. Skim historical notes only if needed.
3. Prefer vault intent when code disagrees; **call out conflicts** instead of
   inventing a third model.
4. **Do not edit vault notes** unless the user asks.

Paths are machine-local. If missing, say so and continue from the repo.

## Code

Game modules: `src/game/` (list in `src/game.rs`). Task routing: the handoff.
Style authority: `STYLE.md`. Match the file you edit; no drive-by refactors.

**Comments:** ASCII only (`Sum`, `->`, plain `-`). **Add, do not edit or
replace** existing comments unless asked. Notify instead. New function
comments: **what** it does first, why second.

**Names:** the vocabulary file wins on naming. New term: ask the user and
record it there.

**Reviews:** update `reviewlog.md` (add still-open items, remove ones that are
no longer true). It is review debt, not a design doc.

**Build:** `cargo check --lib` and `cargo test --lib`. Bevy is a dependency;
prefer `--lib` unless the binary is the task.
