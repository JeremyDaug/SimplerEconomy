# Suggested prune — reduce agent/session baggage

**Date:** 2026-08-03  
**Branch context:** `EconCiv-Rework-Branch` (post–records mid-migration)  
**Purpose:** Working note of what to delete, consolidate, or simplify for a
fresher, more focused instance. Not a design doc; act on items when ready.

**Guiding principle**

- **Keep:** day-logic code, one status surface, one style surface, one vocabulary
  surface, tests that lock real behavior.
- **Cut:** session handoffs, duplicate status docs, outdated in-repo design
  prose, half-migrated types/fields, essay comments that restate vault/`docs/`,
  unused config knobs.
- **Do not** delete vault notes unless explicitly asked.
- **Do not** gut `process.rs` / firm production as “cleanup” — that is domain
  bulk, not agent bloat.

---

## Tier 1 — Delete or replace (highest leverage)

| Item | Why | Action |
|------|-----|--------|
| **`docs/agent-handoff.md`** | Pure agent baggage. Already stale (claimed uncommitted mid-migration; work is committed and still mid-broken). Forces every new instance to load a long, half-wrong story. | **Delete** after finishing the records migration, or fold 10–15 lines of “current focus” into `todo.md` and then delete. |
| **Stale mid-migration dead API in `pop.rs`** | Imports/tests for `LivingStandardHistory`; rebuild of `PopRecords` with fields that no longer exist (`wealth_amv_per_household`, `living_history`, `record_living_standard`). Compile break + mental double-bookkeeping. | **Remove.** Finish **one** records model only (`pop_property::PopRecords` + `update_living_standard` / `update_trend`). |
| **Dead living-standard knobs in `config.rs`** | `DEADBAND`, `TREND_ALPHA_*`, `TREND_SCALE`, `history_capacity` (and comments naming `LivingStandardHistory`) if trend is raw δ and the ring is just `CircularBuffer`. | **Delete unused constants** once migration is green; keep only what `update_trend` / sentiment still read. |

These three clear more current confusion than any doc merge.

---

## Tier 2 — Consolidate docs (less baggage for humans and agents)

Four overlapping “what’s going on?” surfaces today:

| File | Role today | Recommendation |
|------|------------|----------------|
| **`todo.md`** | Working checklist | **Keep as primary focus list.** Prefer short: P0 only + “done recently.” |
| **`reviewlog.md`** | Full review archaeology (~200 lines: open + resolved + deferred + leftovers) | **Prune hard.** Keep only open bugs/suggestions you still care about (e.g. B3, B6, a few suggestions). Drop “Resolved”, “Earlier leftovers”, long narrative. Or merge open items into `todo.md` and delete this file. |
| **`docs/agent-handoff.md`** | Session dump | **Delete** (Tier 1). |
| **`docs/proposals/satisfaction-ratio-and-boosts.md`** | Good design note; overlaps vocabulary | **Keep thin** *or* fold the few formulas you still need into `design-vocabulary.md` and delete/archive the proposal. Don’t maintain both at full length. |

**Also:**

| File | Recommendation |
|------|----------------|
| **`docs/proposals/institution-draft.md`** | Short and useful — **keep**. |
| **`docs/design-vocabulary.md`** | **Keep** (canonical names). Trim only where it still names obsolete fields (`wealth_amv_per_household`, `living_history`, etc.). |
| **`AGENTS.md` + `STYLE.md`** | **Keep both**; don’t expand further. AGENTS = vault/map/workflow; STYLE = code rules. Avoid a third philosophy doc. |
| **`README.md`** | Old “Wants” design essay, not the EconCiv rework. Either **replace with a ~20-line pointer** (what it is, build, vault, branch) or leave it but **stop treating it as design truth**. Misleading if agents read it first. |

**Target meta footprint for a “fresh” instance:**

```text
AGENTS.md
STYLE.md
docs/design-vocabulary.md
todo.md                    # current focus only
(+ optional short institution note)
```

Everything else is optional.

---

## Tier 3 — Simplify code surfaces (focus, not mass deletion)

### `src/game/pop.rs` (~3.1k lines, ~1.8k tests)

Real center of gravity — not fake bloat, but hard to work in.

- **Don’t delete** growth / consume / process_satisfaction / decay / update_desires.
- **Do simplify:**
  - One records write path (no dual assign + full struct rebuild).
  - One SoL formula for `records.living_standard` (pick either mood-prepared
    `living_standard_score` **or** raw weighted `update_living_standard`; document
    the choice).
  - Optional later: split tests into `pop` submodules / separate test files so
    opening the file isn’t everything at once.

### `src/game/sentiment.rs` (~750 lines)

- Type is good; **module-level design essay is longer than the API needs**.
- **Simplify:** short contract (partition, mods, blend). Keep tests.
- Not a delete candidate.

### `src/playstate.rs`

- Long phase comments (especially migration) restate vault `Turns.md` / `Pops.md`.
- **Simplify** to 2–4 lines per phase + `todo!("…")`. Keep the phase skeleton;
  don’t invent full implementations while cleaning.

### `src/game/effects.rs`

- Keep the consolidation (good cleanup).
- **Simplify:** shorten arm docs that restate vocabulary; point at
  `design-vocabulary.md` once.

### Empty / placeholder modules

`tech.rs`, `techtree.rs`, `unit.rs`, `contract.rs`, empty `src/screens/`

- **Low priority.** Tiny; not the main baggage feeling.
- Delete only if you want a smaller module list **and** nothing needs them yet.

### `process.rs` / `firm.rs`

- Large mainly from **tests + real production math**.
- **Do not gut** as part of “fresh instance.” Opposite of focus if work is on
  pop/satisfaction.

---

## Tier 4 — Do not throw out

| Keep | Reason |
|------|--------|
| Pop day methods that already work | Near complete; deleting is regression. |
| `pop_property.rs`, `config.rs`, `util.rs` | Right direction; finish them, don’t reverse. |
| `sentiment.rs` core API | Needed by `process_satisfaction`. |
| Institution v0 + short draft | Small, intentional. |
| Vault EconCiv notes | Source of truth; clean **in-repo** mirrors instead. |

---

## Suggested cleanup sequence (low drama)

1. **Finish or freeze records migration** so two models don’t coexist while pruning.
2. **Delete `docs/agent-handoff.md`** (or replace with ~15 lines in `todo.md`).
3. **Prune `reviewlog.md` → open items only** (or merge into `todo.md`).
4. **Strip dead config + dead `LivingStandardHistory` code/tests.**
5. **Shorten playstate / sentiment / process_satisfaction comments** that only restate docs.
6. Optionally **rewrite `README.md`** to a short rework pointer so agents don’t load old “Wants” design first.

Skip mass deletion of domain modules. Baggage is mostly **status docs + dual records design + comment essays**, not the existence of `firm.rs`.

---

## One-sentence diagnosis

Previous instances optimized for **continuity and review archaeology** (handoff,
full reviewlog, dual proposals, long phase essays). A fresh working instance
wants **one focus list, one vocabulary, one records model, and code that
compiles** — everything else is optional weight.

---

*Act when ready; this file is a suggestion list only.*
