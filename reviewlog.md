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

**Last updated:** reorganized after dev responses on review of `1c06bb3..HEAD` (through `771e1a3`)  
**Scope then:** `demographic_update`, `growth_phase`, `DemographicEffect`, `update_desires` test fixes, playstate `advance_turn` stubs  
**Diff then:** 20 files, +1089 / −62 · `cargo test --lib` 79 passed

---

## Open bugs

*(None currently tracked from that review after reclassification. See earlier leftovers below.)*

---

## Open suggestions

### 1. `DemographicEffect` never applied to household modifiers
- **File:** `src/game/demographiceffect.rs` (and species/culture/religion)
- **What:** Effects are stored (`species_effects` / `culture_effects`, etc.) but nothing folds them into `*_household_modifiers`. `demographic_update` only reads pre-baked modifiers. Setting `household_changed` without updating modifiers is a silent no-op for intended effect changes.
- **Fix idea:** `fn apply_to_household(&self, h: &mut HouseholdDef)` (or rebuild modifiers from effects whenever effects change) as single source of truth before setting `household_changed`.
- **Related:** open deferred item on household rebuild order (demographic then other effects in one phase).

### 2. Property scaling by `previous_growth` is not idempotent
- **File:** `src/game/pop.rs` (~381–390)
- **What:** A second `update_desires` / `demographic_update` in the same day (or before `growth_phase` overwrites `previous_growth`) multiplies targets by the same factor again.
- **Fix idea:** Scale from an absolute baseline, zero `previous_growth` after applying property scale, or track “already scaled this turn.”

### 3. Common/luxury growth terms may have wrong sign
- **File:** `src/game/pop.rs` (~632–638)
- **What:** Common/luxury use `-k * tier_total_satisfaction` (penalize *having* satisfaction). Basic correctly penalizes *lack* (`-0.30 * (1 - sat)`). Common coeff `0.0002` is nearly a no-op vs luxury `0.005`. Confirm intent (wealth/demographic-transition effect vs copy-paste inversion).
- **Fix idea:** If unmet needs should hurt growth, use lack. If wealth reduces births, document that next to basic’s opposite sign.
- **Related:** deferred Birthrate vs Mortality semantics.

### 4. `demographic_update` not wired into turn loop
- **File:** `src/playstate.rs` (~104–106)
- **What:** `phase_player_bonuses_and_demographic_updates` is still `todo!()`, so `Pop::demographic_update` never runs from the turn loop, and nothing clears shared `household_changed` flags after all pops update.
- **Fix idea:** When filling the stub: apply player demographic edits → `par_iter` `pop.demographic_update` → clear `household_changed` on touched factuals (orchestrator clears flags, not `Pop`). Then apply non-demographic household effects so rebuild-from-demo alone does not leave the household incomplete (see deferred household rebuild note).

### 5. Household size change with fixed count jumps total pop
- **File:** `src/game/pop.rs` (~303–321)
- **What:** Rebuild changes adults/children/etc. while leaving `demographics.count` fixed, so total population (`count * size()`) jumps immediately. Species/culture docs call this out; `Household::alter_household_maintain_members` exists but is unused.
- **Fix idea:** For size-affecting changes, conserve members (adjust count) or phase the delta over turns; at least `debug_assert` large swings in dev.

### 6. `growth_phase` takes unused `factuals`
- **File:** `src/game/pop.rs` (~617–648)
- **What:** `growth_phase` takes `factuals` but ignores it; growth only uses already-baked household. Combined with #1, mid-turn effect-only factual edits never affect growth.
- **Fix idea:** Drop unused param until needed, or resolve baseline birth/mortality from factuals if household is not sole source of truth.

### 7. Test coverage gaps
- **File:** `src/game/pop.rs` (~1480–1636 and update_desires tests)
- **What:** `growth_phase` tests miss: rate ≤ -100% before snap, Birthrate malus / Mortality bonus arms, household birth/mortality + desire interaction. `demographic_update` tests miss: missing factual species id, efficiency-only modifiers. (Dead-pop / zero-amount NaN cases closed as invariants with `debug_assert`.)
- **Fix idea:** Prefer tests that lock intended growth semantics and demographic rebuild behavior once those designs firm up.

---

## Open nits

### 8. Religion/species field names and docs say “Culture”
- **Files:** `src/game/religion.rs` (~24–33), `src/game/species.rs` (~24–34)
- **What:** Religion has `culture_effects` / `culture_household_modifiers` and Culture-flavored comments; species docs also say “Culture” in places.
- **Fix idea:** Rename religion fields to `religion_*` and fix comments.

### 9. Growth test comment missing parentheses
- **File:** `src/game/pop.rs` (~1389–1411)
- **What:** Comment writes `growth_f = count / count - previous_growth` and `10 / 10 - 5 = 2.0`; code correctly uses `(count - previous_growth)`.
- **Fix idea:** Comment as `count / (count - previous_growth)`.

### 10. Unreachable code after Class `todo!`
- **File:** `src/game/factuals.rs` (~128–131)
- **What:** `DesireSource::Class` arm is `todo!(...); None` — `None` is unreachable.
- **Fix idea:** Keep only `todo!(...)` until Class exists.

### 11. `decay_goods` takes `&self` but will need `&mut self`
- **Files:** `src/game/actors.rs` (~27–29), players equivalent
- **What:** Signatures will have to change when implemented and will ripple through `phase_good_decay`.
- **Fix idea:** Use `&mut self` now so call sites stay stable.

---

## Deferred / accepted (explained, not open bugs)

These were filed as bugs in review; dev responses reclassify them. Track here so the intent is not lost, but they are not merge-blocking bugs as originally stated.

### D1. Household rebuild and non-demographic mods
- **Was:** Bug — rebuild from `HouseholdDef::default()` + demo modifiers wipes institutional / day-start overlays.
- **Dev response:** Known potential issue; being worked on. Intended order is **demographic effects first, then all other effects**, in the same phase `phase_player_bonuses_and_demographic_updates`, which would make the wipe a non-issue. Code does not fully support this yet.
- **Status:** Deferred design / WIP with open suggestion #4 (wire the phase). Not an independent “forgot reapply” bug once that phase exists.

### D2. Mortality vs Birthrate desire effects identical in net growth
- **Was:** Bug — `tier_desire_effect_growth` uses the same arms for both variants.
- **Dev response:** Known flaw to address eventually. Current assumption is birth and mortality rates are always non-negative; since they are only added into a single net rate today, the distinction is largely meaningless.
- **Status:** Deferred design debt (separate accumulators / non-negative contracts later). Related open suggestion #3 (common/luxury sign intent).

---

## Resolved

### R1. Dead-pop `growth_f` → NaN
- **File:** `src/game/pop.rs` (property target rescaling)
- **Resolution:** `debug_assert` added. Invariant: household count is only `0.0` when the pop is dead, which happens in growth and is destroyed immediately after. `previous_growth` should not drive old count below one household either. New pops have `previous_growth == 0.0` regardless of size, so old count is just `count > 0`.

### R2. Satisfaction rescale divides by zero
- **File:** `src/game/pop.rs` (existing-desire resync)
- **Resolution:** `debug_assert` added. Invariant: `desire.amount` is not allowed to be `0` anywhere outside of initialization.

---

## Earlier review leftovers (pre-`1c06bb3`, may still apply)

From the prior `update_desires` review (`1a22072^..1c06bb3`). Re-check before treating as open.

1. Growth test vs formula mismatch (likely fixed in `3884c6b` / `ce61887` — tests passed at later review).
2. Update path leaving `effect` / `decay` / `scalar` stale on existing desires.
3. Tier changes never move desires between tier vecs.
4. Desires kept if demo still exists on *any* demographic after culture/religion/species conversion (stacking).
5. `source_demo_desire` panics on missing parent demographic (`find_*`) instead of returning `None`.
6. `AGENTS.md` still names `apply_scaling_factor` vs `get_scaling_factor`.

---

## Suggested priority when picking these up

1. Wire demographic turn phase (#4) — unblocks intended household effect ordering (D1).
2. Household size conservation (#5) and `DemographicEffect` → modifiers (#1) while in that area.
3. Growth semantics: common/luxury sign (#3), later Birthrate vs Mortality split (D2).
4. Idempotency and tests (#2, #7).
5. Nits (#8–#11) whenever touching those files.
6. Re-verify earlier leftovers when next touching `update_desires`.
