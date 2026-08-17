# SimplerEconomy — Style Guide

Enforce this guide for new and touched code. Prefer matching the file you edit
when older code predates a rule; do not drive-by reformat unrelated modules.

Project rules, design vault paths, and agent workflow live in `AGENTS.md`. This
document is the **coding style** authority.

---

## 1. Philosophy

1. **Data first.** Prefer clear structs and explicit fields over clever abstraction.
2. **Factuals vs game state.** Definitions (`Factuals`: goods, processes, species,
   cultures, religions) stay separate from live actors, markets, and prices.
3. **Design docs win on intent.** EconCiv vault notes define behavior; call out
   conflicts instead of inventing a third model.
4. **Playable and inspectable.** Names and docs should help a human read the day
   pipeline without hunting through layers of indirection.
5. **Small, honest steps.** Implement the phase/function in front of you; leave
   stubs (`todo!`) with short docs rather than half-wired systems.

---

## 2. Language and numeric defaults

| Topic | Rule |
|--------|------|
| Language | Stable Rust; idiomatic ownership and borrowing |
| Quantities, rates, prices | **`f64`** unless profiling proves a bottleneck |
| IDs | **`usize`**; **`0` often means none / blank** (no state, no platonic base, etc.) |
| Collections | `HashMap` / `HashSet` for id-keyed registries and membership |
| Parallelism | `rayon` for independent actors/markets; **disjoint borrows** only |

Do not introduce `f32` “for performance” without measurement. Do not use stringly
typed IDs for domain entities.

---

## 3. Naming

### Types and modules

- **Types:** `UpperCamelCase` — `Pop`, `DemoDesire`, `PopPRow`, `InstitutionKind`.
- **Modules:** `snake_case` files under `src/game/` matching the primary type when
  practical (`pop.rs`, `desire.rs`, `institution.rs`).
- **Property rows:** `*PRow` for per-good stock ledgers (`PopPRow`, `FirmPRow`).
- **Enums for catalogs:** prefer one shared master (`EffectKind`) plus **domain
  enums** (`DesireEffect`, `PopEffect`, `InstitutionEffect`) that restrict which
  effects are legal at each site.

### Functions and fields

- **Methods:** `snake_case`. Prefer verb phrases for actions: `decay_goods`,
  `satisfy_one_desire`, `update_desires`, `growth_phase`.
- **Fluent setters:** `with_*` (`with_name`, `with_desire`, `with_effect`).
- **Finders:** `find_*` returning `Option` when missing is normal; **`find_*` that
  panics** (e.g. `Factuals::find_good`) only when absence is a hard data error.
- **Scaling helpers:** centralize on the owner type
  (`Pop::get_scaling_factor`) — do not duplicate `match` arms on `ScalingFactor`.

### Tests

- Nested modules named **`thing_should`** with test fns as behavior sentences:
  `mod decay_goods_should { fn returns_used_then_decays_... }`
- Fixtures: `make_pop()`, `make_desire(...)`, `make_default_factuals()` beside the
  tests that need them.

---

## 4. Construction and fluent builders

**Preferred shape:**

```rust
Type::new(required_ids_and_names)
    .with_field(x)
    .with_item(one_more)  // push into a collection
```

Rules:

1. **`new(...)`** sets safe defaults for everything else (empty vecs/maps, `0`,
   `None`, loyalty `1.0`, etc.).
2. **`with_*` takes `mut self` and returns `Self`.**
3. **Growing collections push one item per call** (`with_good`, `with_desire`,
   `with_effect`, `with_market`, `with_firm`). Do not replace entire lists in a
   setter unless the API is explicitly “set all”.
4. **One-line `///` on each fluent setter.** If the setter has a `debug_assert`,
   add a second doc line stating the constraint.
5. **Avoid hand-written struct literals** for domain types in production paths
   when a builder exists — easy to omit fields. Tests may use literals when
   building minimal fixtures, but prefer builders/`new` when available.
6. **Registry `with_*` (Factuals, Culture desires, …):** **panic** on duplicate
   IDs. That is intentional; do not silently overwrite.

---

## 5. Documentation comments

### Section headers for domain types and major methods

```rust
/// # Decay Goods
///
/// Called at the very end of the day (Pop Day §10). ...
///
/// 1. Step one.
/// 2. Step two.
pub fn decay_goods(&mut self, factuals: &Factuals) { ... }
```

- Use `/// # Name` for types and non-trivial methods (already common in-tree).
- Document **order of steps**, **what is not mutated**, and **phase placement**
  when the function is part of the day pipeline.
- Cross-link related types with `[`Type`]` / `[`Type::method`]` where it helps.
- Keep comments **honest**: if something is deferred, say so; do not document
  behavior that is still `todo!()`.

### Tone

- Clear, slightly formal, explanatory — closer to design notes than slang.
- Prefer complete sentences in public docs.
- Typos in old comments are fine to fix when touching the area; do not run
  repo-wide comment polish.

### Module-level docs

- Important catalogs may use `//!` module docs (`effects.rs`).
- Most game modules lead with the primary type’s `/// # Name` block instead.

---

## 6. Assertions and failure modes

| Mechanism | When |
|-----------|------|
| `debug_assert!` | Invariants during development: builder ranges, desire amounts, empty leftover bags after a phase |
| `panic!` / `assert!` | Must never be silent: **duplicate factual/registry IDs**, missing good when decay/consume requires definition (`find_good`) |
| `todo!` | Unimplemented phase body; keep a one-line message |
| Soft `Option` | Normal absence (no culture id, no property row yet) |

**Assert messages must match the condition** (e.g. `>= 1.0` in both the check and
the message).

Do not use `debug_assert!` for conditions that would corrupt multiplayer-visible
save state in release without a plan — prefer hard checks for data integrity of
registries.

---

## 7. Architecture patterns (style, not full design)

### Ownership

- **`Actors` owns** pops, firms, institutions.
- **Markets hold membership IDs only** (`HashSet<usize>`), not full entities.
- Multi-market entities (institutions) keep their own `markets: Vec<usize>` **and**
  appear in each market’s membership set when present.
- Controlled firms: institution stores **`firm_ids`**, not nested `Firm` values.

### Day / turn functions

- Prefer **phase methods** on the orchestrator (`PlayState`) that call into
  `Actors` / `MapData` / per-entity methods.
- Entity methods take **`&Factuals` (read-only)** when they only need definitions;
  mutate `self` for their own state.
- End-of-day paths (record keeping, decay) may **fan out in parallel** over
  disjoint maps (`rayon::scope` + `par_iter_mut`).

### Related logic must stay consistent

If two paths touch the same goods for the same desire (e.g. **reserve** vs
**satisfy**):

- Share ordering helpers (`Desire::ordered_targets`).
- Share cap/efficiency math shape.
- Document intentional differences (e.g. reserve uses `available()`, consume uses
  full `quantity`).

### Effects

- Put new effect kinds on **`EffectKind`** when they are real catalog entries.
- Add them only to the **domain enums** that may emit them.
- Implement `to_kind` / `scope` (etc.) for every arm — no silent `_ =>` drops for
  new variants in those bridges.

### Property and savings

- `quantity` is physical stock.
- `save_target` is a **soft target**, not a hard fence on consumption or reservation
  unless a future design explicitly reintroduces that.
- `reserved` is a **same-day earmark**; cleared at day start for pops.
- `used` / `consumed` are end-of-day decay inputs; document move-back and 100%
  consume decay when editing those paths.

---

## 8. Module and file layout

```
src/game/<domain>.rs     # primary type + helpers + #[cfg(test)] module
src/game/effects.rs      # shared effect catalogs
src/playstate.rs         # turn orchestration
docs/proposals/          # short design drafts (trim when implemented)
```

- One primary domain concept per file when size allows; split only when a file
  becomes unmanageable.
- **Tests live next to the code** in `#[cfg(test)] mod <name>`.
- Re-export domain effects from the module that “owns” the concept when useful
  (`pub use crate::game::effects::PopEffect` in `pop.rs`).

---

## 9. Tests

1. **Behavior-named modules and functions** (see Naming).
2. **Arrange → act → assert** with explicit numbers; prefer exact equality on
   clean fractions; use epsilon only when needed.
3. **Fixtures via `Factuals::new()` + fluent helpers** when testing registry-backed
   behavior; `find_good` paths need goods inserted or they panic.
4. Cover **invariants called out in docs** (ordering, caps, empty leftover
   collections, exposure skip, etc.).
5. Do not delete a failing test to “make CI green” — fix code or update the test
   with a comment if the design intentionally changed.
6. Prefer `cargo test --lib` for fast feedback.

---

## 10. Comments in implementation

- Step comments (`// 1. ...`, `// 2. ...`) are welcome when the method doc lists
  numbered steps — keep them in sync.
- Avoid narrating obvious Rust (`// increment i`). Explain **domain why**.
- `TODO:` is allowed for known follow-ups; prefer a short, actionable phrase.

---

## 11. What not to do

1. **Drive-by refactors** of unrelated modules or renames across the crate.
2. **Silent overwrites** in ID registries.
3. **Dual-owning** the same entity in two authoritative stores.
4. **Merging State and Institution** into one type (State is the player shell).
5. **Giving institutions full production clones** of firms when `firm_ids` + control
   is the design.
6. **Editing the Obsidian vault** unless explicitly asked.
7. **Expanding scope** mid-task (e.g. implementing the next day phase without being
   asked).
8. **Leaving review debt stale** — when reviewing, update `reviewlog.md` (add /
   remove / mark fixed).

---

## 12. PR / change hygiene

- Match naming and comment tone **in the file you edit**.
- Keep diffs focused: one concern per commit when practical.
- After behavioral changes, update **inline docs** and **tests** in the same change.
- If design and code disagree, note it in the PR/response; do not quietly pick a
  third behavior.

### Quick checklist (before you call a change done)

- [ ] Builder/`new` used where the type has one  
- [ ] Fluent `with_*` docs + asserts documented  
- [ ] IDs / `0` semantics respected  
- [ ] Related paths (reserve/satisfy, decay/consume) stay consistent  
- [ ] `debug_assert` / `panic` choice matches §6  
- [ ] Tests named `*_should` and cover the new behavior  
- [ ] No unrelated reformatting  
- [ ] `cargo check --lib` / `cargo test --lib` for touched areas  

---

## 13. Relationship to other docs

| Doc | Role |
|-----|------|
| `AGENTS.md` | Agent/project rules, vault paths, build commands, reviewlog |
| `STYLE.md` (this file) | Coding style and structural conventions |
| `docs/design-vocabulary.md` | Canonical design terms (tier sat, sentiment, …) |
| `reviewlog.md` | Working backlog of review findings only |
| `docs/proposals/*` | Short design drafts; trim after implementation |
| EconCiv Obsidian vault | Authoritative long-form design |

When style and an old file conflict, **follow this guide for new code** and
modernize the old file only when you are already editing it for a real task.
