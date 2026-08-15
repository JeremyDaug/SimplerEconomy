# SimplerEconomy — Project Rules

Rust economic / civilization simulator. Current work lives on branches like
`EconCiv-Rework-Branch`. Prefer the rework design over older incomplete models
when they conflict.

## Conversation and Tone

Conversational, less clipped sentences, more verbose. Make it feel natural, like talking 
to another person.

## Goals

- Simulate a **simplified economy**: goods, processes, markets, firms, and pops.
- Pops consume and demand via **desires** sourced from species, culture, religion,
  and related demographics.
- Separate **Factuals** (mostly static world definitions: goods, processes,
  cultures, species, religions) from **game state** (map, markets, players, prices,
  current property).
- Keep systems playable and inspectable; favor clear data models over premature
  abstraction.

In-repo high-level notes also live in `README.md` and `TODO.md`. Those are
secondary to the Obsidian design vault for intent and direction.

**Session catch-up for agents:** [`docs/agent-handoff.md`](./docs/agent-handoff.md)
(refresh when major work lands so the next instance can pick up quickly).

### Design vocabulary (in-repo)

**Canonical naming for design talk and comments:**
[`docs/design-vocabulary.md`](./docs/design-vocabulary.md).

Prefer those terms over chat shorthand (e.g. say **tier sat** / **desire sat**,
not bare “ratio” or “tier fill”). When language conflicts, the vocabulary file
wins on **names**; the vault and proposals still own long-form intent.

When a new term comes up, ask user to clarify and record in the file.

## Design documents (external Obsidian vault)

Authoritative long-form design, brainstorms, and direction live **outside this
repo** in the author's Obsidian vault. Do not treat the codebase alone as the
full product vision.

### Primary — EconCiv rework (prefer this)

`/home/jeremy/Documents/Obsidian Vault/Game Ideas/EconCiv/`

Key notes:

- `Economic Civilization.md` — overall vision
- `Desires.md`
- `Pops.md`
- `Goods.md`
- `Processes.md`
- `Market.md`
- `Firms.md`
- `Turns.md`
- `The Player, State, or Nation.md`

### Supporting / historical

`/home/jeremy/Documents/Obsidian Vault/Game Ideas/Simlper Economy Simulator/`

Useful for background (e.g. `Desire Brainstorms.md`, `Good.md`, `Culture.md`,
`Market.md`, `The Civilization.md`). Prefer **EconCiv** when notes disagree.

### How to use the vault

When implementing or redesigning systems (desires, pops, markets, firms, goods,
processes, turns, player/state):

1. Read the matching note under **EconCiv** first.
2. Skim supporting notes only if needed for history or extra detail.
3. Prefer vault intent when code and notes disagree; **call out conflicts** in
   the response rather than silently inventing a third model.
4. **Do not edit vault notes** unless the user explicitly asks.

These paths are machine-local. If files are missing, say so and continue from
repo sources.

## Code organization

| Area | Location |
|------|----------|
| Game modules | `src/game/` (see `src/game.rs` for the module list) |
| Gameplay config | `config.rs` — centralized tunables (`config::living_standard`, …) |
| Desire hierarchy | `desire.rs` — PlatonicDesire → DemoDesire → Desire |
| Pops / demographics | `pop.rs` — `Pop`, `DemoRow`; property/records in `pop_property.rs` |
| World definitions | `factuals.rs` — goods, processes, species, cultures, religions |
| Scaling | `scalingfactor.rs` + `Pop::apply_scaling_factor` |
| App / play shell | `src/playstate.rs`, `src/main.rs` |

Domain modules of note: `good`, `process`, `culture`, `species`, `religion`,
`firm`, `market`, `household`, `state`, map/territory types.

## Coding style

**Full guide:** [`STYLE.md`](./STYLE.md) — enforce it for new and touched code.

Short reminders (details in `STYLE.md`):

- `Type::new` + fluent `with_*` (`mut self -> Self`); collections **push** one item
  per `with_*`; document each setter; document `debug_assert` constraints.
- `Factuals::new()` + fluent inserts; registry `with_*` **panics** on duplicate IDs.
- IDs are `usize`; `0` often means none / blank. Prefer **`f64`** for quantities.
- Desire tiers: `0` Basic, `1` Common, `2` Luxury. Scale demo amounts via
  `Pop::get_scaling_factor` / `DemoDesire::create_desire`.
- Section docs `/// # Name` on domain types and major methods; tests in
  `#[cfg(test)]` modules named `*_should`.
- Match the file you edit; no drive-by refactors; no vault edits unless asked.

## Documentation

- Comments use **ASCII only** (common keyboard characters): prefer `Sum` over
  summation glyphs, `->` over arrow characters, plain `-` over en/em dashes.
- Leave commenting to User, only add, do not edit or replace. Notify user instead.

### Code review log

Open review findings live in repo-root `reviewlog.md`. **Whenever a code review
is done** (including `/review` or ad-hoc review of commits/diffs):

1. **Add** new open bugs, suggestions, and nits that are worth tracking later.
2. **Remove or mark fixed** items that the review (or intervening work) shows
   are no longer true — do not leave stale open bugs.
3. Keep the log scannable: file path, short what/fix idea, optional priority.
4. Do not treat `reviewlog.md` as a full design doc; it is a working backlog of
   review debt only.

## Build and test

```bash
cargo check --lib
cargo test --lib
```

Bevy is a dependency; prefer `cargo check --lib` / `cargo test --lib` for fast
feedback unless full binary behavior is required.

## Branch context

Active rework emphasizes demographic desires (`DemoDesire`), culture-owned
desire lists, factual registries for species/culture/religion, and pop-scaled
desire creation. Align new work with that direction and the EconCiv vault notes.
