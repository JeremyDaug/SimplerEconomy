# SimplerEconomy — Project Rules

Rust economic / civilization simulator. Current work lives on branches like
`EconCiv-Rework-Branch`. Prefer the rework design over older incomplete models
when they conflict.

## Goals

- Simulate a **simplified economy**: goods, processes, markets, firms, and pops.
- Pops consume and demand via **desires** sourced from species, culture, religion,
  and related demographics.
- Separate **Factuals** (mostly static world definitions: goods, processes,
  cultures, species, religions) from **game state** (map, markets, players, prices,
  current property).
- Keep systems playable and inspectable; favor clear data models over premature
  abstraction.

In-repo high-level notes also live in `README.md` and `todo.md`. Those are
secondary to the Obsidian design vault for intent and direction.

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
| Desire hierarchy | `desire.rs` — PlatonicDesire → DemoDesire → Desire |
| Pops / demographics | `pop.rs` — `Pop`, `DemoRow`, property rows |
| World definitions | `factuals.rs` — goods, processes, species, cultures, religions |
| Scaling | `scalingfactor.rs` + `Pop::apply_scaling_factor` |
| App / play shell | `src/playstate.rs`, `src/main.rs` |

Domain modules of note: `good`, `process`, `culture`, `species`, `religion`,
`firm`, `market`, `household`, `state`, map/territory types.

## Coding style

### Builders and fluent APIs

- Prefer `Type::new(...)` plus fluent `with_*` setters that take `mut self` and
  return `Self`.
- Collection fields that grow over time usually **push** one item per call
  (e.g. `with_good`, `with_desire`, `with_effect`), matching existing patterns
  on `PlatonicDesire`, `DemoDesire`, `Culture`, `Factuals`, `Process`.
- One-line `///` docs on each fluent setter. If a setter has a `debug_assert`,
  add a second doc line describing the constraint.

### Factuals and IDs

- Construct with `Factuals::new()`, then chain or insert additions. Avoid
  hand-written struct literals that omit fields.
- Fluent `with_*` helpers on `Factuals` **panic** if an ID already exists; keep
  that contract for any similar registries.
- IDs are `usize`; `0` often means none / blank (e.g. no platonic base, no state).

### Desires and demographics

- Desire **tiers**: `0` Basic, `1` Common, `2` Luxury (three nested vecs on pops
  and cultures).
- `DemoDesire` amounts are scaled for **one household**; materialize a pop-level
  `Desire` via `DemoDesire::create_desire(&pop, source)`, which multiplies amount
  by `pop.apply_scaling_factor(self.scalar)`.
- Put shared scalar resolution on `Pop::apply_scaling_factor` rather than
  duplicating `match` arms.

### Docs and tests

- Public/domain types use section-style comments (`/// # Name`) where the
  codebase already does.
- Prefer `debug_assert!` for invariant checks in builders during development;
  use `panic!` / `assert!` where failure must never be silent (e.g. duplicate
  factual IDs).
- Tests live in `#[cfg(test)]` modules beside the code. When building test
  fixtures, use `Factuals::new()` and fluent helpers where available.

### Scope discipline

- Match existing naming and comment tone in the file you edit.
- Do not drive-by refactor unrelated modules.
- Do not rewrite design docs or README unless asked.

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
