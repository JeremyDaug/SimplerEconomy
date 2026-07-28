# Review log

Working backlog of open code-review findings. Not everything here blocks
progress; pick up items when convenient.

## Maintenance (for every code review)

When reviewing commits, a branch, a PR, or local changes:

1. **Add** new open issues worth tracking (bug / suggestion / nit).
2. **Remove** items that are fixed or no longer accurate (do not leave stale open bugs).
3. Optionally note the review range and date in a short “Last updated” line below.
4. Keep entries scannable: file path, what, fix idea.
5. Record **dev responses** that close or reclassify items (fixed / accepted invariant /
   deferred design), then move them out of the open backlog.

---

**Last updated:** review of `771e1a3..HEAD` (through `0d9a57e`)  
**Scope:** effects consolidation (`effects.rs`), Institution v0 draft, Pop
`initial_reservations_and_update_satisfaction` / `decay_goods` / `stored_effects`,
playstate consumption wire-up  
**Diff:** 15 files, +1189 / −265 · `cargo test --lib` 90 passed

---

## Open bugs

### B3. Non-goods `PopEffect` never applied; discarded at EOD
- **File:** `src/game/pop.rs` (`stored_effects`, `growth_phase`, `decay_goods` ~885–904), `src/game/effects.rs` (`PopEffect`)
- **What:** Docs say Birthrate/Mortality/Satisfaction are consumed in growth/mood. Growth ignores them; `process_satisfaction` is `todo!()`. Decay drains non-`BonusGood` into `kept_effects` and `debug_assert`s empty (silent drop in release).
- **Fix idea:** Apply and remove matching effects in `growth_phase` / mood; leave only `BonusGood` for decay. Test stored birthrate → growth delta.

---

## Open suggestions

### 1. `DemographicEffect` never applied to household modifiers
- **File:** `src/game/effects.rs` / species/culture/religion `*_effects`
- **What:** Effects are stored but nothing folds them into `*_household_modifiers`. `demographic_update` only reads pre-baked modifiers. Setting `household_changed` without updating modifiers is a silent no-op.
- **Fix idea:** `fn apply_to_household(&self, h: &mut HouseholdDef)` (or rebuild modifiers from effects whenever effects change).
- **Related:** deferred D1 household rebuild order.

### 2. Property scaling by `previous_growth` not idempotent / can go Inf
- **File:** `src/game/pop.rs` (~190–198)
- **What:** Second `update_desires` in the same day multiplies again. Denominator `0` yields `Inf` (NaN assert does not catch it).
- **Fix idea:** Scale from absolute baseline, zero `previous_growth` after apply, assert finite growth factor.

### 3. Common/luxury growth terms may have wrong sign
- **File:** `src/game/pop.rs` (~710–716)
- **What:** Common/luxury use `-k * tier_total_satisfaction` (penalize *having* satisfaction). Basic penalizes *lack*. Common coeff nearly a no-op.
- **Fix idea:** Confirm wealth/transition intent and document, or use lack. Related: deferred D2.

### 4. `demographic_update` not wired into turn loop
- **File:** `src/playstate.rs` (~119–123)
- **What:** `phase_player_bonuses_and_demographic_updates` still `todo!()`. Institution effect push and `Pop::demographic_update` never run from the turn loop; nothing clears `household_changed`.
- **Fix idea:** Institutions/firms → `par_iter` demographic_update → orchestrator clears flags → non-demo household effects (D1). Drop unused `factuals` bind in `phase_pop_consumption` until needed.

### 5. Household size change with fixed count jumps total pop
- **File:** `src/game/pop.rs` (~514–531)
- **What:** Rebuild changes adults/children/etc. while leaving `count` fixed; total pop jumps. `alter_household_maintain_members` unused.
- **Fix idea:** Conserve members or phase deltas; `debug_assert` large swings in dev.

### 6. `growth_phase` takes unused `factuals`
- **File:** `src/game/pop.rs` (~695–697)
- **What:** Param ignored (`let _ = factuals`). Mid-turn effect-only factual edits never affect growth without household rebake.
- **Fix idea:** Drop until needed, or resolve baseline rates from factuals if household is not sole source of truth.

### 7. Test coverage gaps (growth / demographic edges)
- **File:** `src/game/pop.rs` tests
- **What:** New reservation/decay tests help. Still missing: growth rate ≤ -100% before snap, Birthrate malus / Mortality bonus arms, household birth/mortality + desire interaction; demographic missing-species / efficiency-only modifiers; stored_effects → growth.
- **Fix idea:** Lock intended growth and rebuild semantics once designs firm up.

---

## Open nits

### 8. Religion/species field names and docs say “Culture”
- **Files:** `src/game/religion.rs` (~24–33), `src/game/species.rs` (~24–34)
- **What:** Religion has `culture_effects` / `culture_household_modifiers`; species docs say “Culture” in places.
- **Fix idea:** Rename religion fields to `religion_*` and fix comments.

### 9. Growth test comment missing parentheses
- **File:** `src/game/pop.rs` (~1602–1603)
- **What:** Comment writes `count / count - previous_growth`; code uses `(count - previous_growth)`.
- **Fix idea:** `count / (count - previous_growth)`.

### 10. Unreachable code after Class `todo!`
- **File:** `src/game/factuals.rs` (~128–131)
- **What:** `todo!(...); None` — `None` unreachable.
- **Fix idea:** Keep only `todo!(...)` until Class exists.

### 11. Residual `decay_goods(&self)` on players / mapdata
- **Files:** `src/game/players.rs:14`, `src/game/mapdata.rs:22`
- **What:** Actor path fixed to `&mut self`. These stubs still take `&self`.
- **Fix idea:** Use `&mut self` now so `phase_good_decay` call sites stay stable.

### 12. Orphaned DesireEffect docs in desire.rs
- **File:** `src/game/desire.rs` (~499–509)
- **What:** Enum moved to `effects.rs`; `# Desire Effect` section header left above `# Desire Source`.
- **Fix idea:** Remove orphaned docs (canonical on `effects::DesireEffect`).

### 13. Unused import / Owners typo
- **Files:** `src/game/pop.rs:3` (`DynamicArray`), `src/game/firm.rs:252` (“ownser”)
- **Fix idea:** Drop import; fix typo.

---

## Deferred / accepted (explained, not open bugs)

### D1. Household rebuild and non-demographic mods
- **Was:** Bug — rebuild from default + demo modifiers wipes institutional / day-start overlays.
- **Dev response:** Intended order is demographic effects first, then other effects, in `phase_player_bonuses_and_demographic_updates`.
- **Status:** Deferred / WIP with open suggestion #4. Not an independent bug once that phase exists.

### D2. Mortality vs Birthrate desire effects identical in net growth
- **Was:** Bug — same arms for both variants in `tier_desire_effect_growth`.
- **Dev response:** Known; birth/mortality assumed non-negative and only sum into one net rate today.
- **Status:** Deferred design debt. Related open suggestion #3.

---

## Resolved (this and prior reviews)

### R1. Dead-pop `growth_f` → NaN
- **Resolution:** `debug_assert` + invariant on dead pops / `previous_growth`. (Inf on zero denominator still open under suggestion #2.)

### R2. Satisfaction rescale divides by zero
- **Resolution:** `debug_assert` on desire amount (see open B4 for predicate/docs mismatch).

### R3. Actors `decay_goods` signature / Pop decay implementation
- **Resolution:** `Actors::decay_goods(&mut self)` parallel-calls `Pop`/`Firm`/`Institution` mut methods. `Pop::decay_goods` implemented (used return, rate decay, consumed destroy, byproducts, desire + stored bonus goods). Residual stubs: players/mapdata (nit #11).

---

## Earlier review leftovers (pre-`1c06bb3`, may still apply)

From the prior `update_desires` review. Re-check when next touching that path.

1. Growth test vs formula mismatch (likely fixed — tests passed at later reviews).
2. Update path leaving `effect` / `decay` / `scalar` stale on existing desires.
3. Tier changes never move desires between tier vecs.
4. Desires kept if demo still exists on *any* demographic after culture/religion/species conversion (stacking).
5. `source_demo_desire` panics on missing parent demographic (`find_*`) instead of returning `None`.
6. `AGENTS.md` still names `apply_scaling_factor` vs `get_scaling_factor`.

---

## Suggested priority when picking these up

1. Wire demographic turn phase (#4) — unblocks household effect ordering (D1) and institution passives.
2. Apply/drain non-goods `PopEffect`s (B3) before anything pushes into `stored_effects`.
3. Household size conservation (#5) and `DemographicEffect` → modifiers (#1).
4. Growth semantics: common/luxury sign (#3), later Birthrate vs Mortality (D2).
5. Idempotency / Inf guard and tests (#2, #7).
6. Nits (#8–#13) whenever touching those files.
7. Re-verify earlier leftovers when next touching `update_desires`.

---

## Fixed (recent)

- **B1 / savings fence:** `consumeable` removed; `satisfy_one_desire` draws from full `quantity` (`saved` is wish-only).
- **B2 / target order:** both reserve and satisfy use `Desire::ordered_targets()`.
- **B4 / amount assert:** `debug_assert!(desire.amount >= 1.0)` matches docs.
