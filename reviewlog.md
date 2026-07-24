# Review log

Working backlog of open code-review findings. Not everything here blocks
progress; pick up items when convenient.

## Maintenance (for every code review)

When reviewing commits, a branch, a PR, or local changes:

1. **Add** new open issues worth tracking (bug / suggestion / nit).
2. **Remove** items that are fixed or no longer accurate (do not leave stale open bugs).
3. Optionally note the review range and date in a short “Last updated” line below.
4. Keep entries scannable: file path, what, fix idea.

---

**Last updated:** review of `1c06bb3..HEAD` (through `771e1a3`)  
**Scope:** `demographic_update`, `growth_phase`, `DemographicEffect`, `update_desires` test fixes, playstate `advance_turn` stubs  
**Diff then:** 20 files, +1089 / −62 · `cargo test --lib` 79 passed

---

## Bugs

### 1. Dead-pop `growth_f` → NaN
- **File:** `src/game/pop.rs` (~382)
- **What:** Property target rescaling uses `growth_f = count / (count - previous_growth)`. When a pop is already dead (`count == 0`) and `previous_growth == 0`, this is `0/0` → NaN and poisons `shop_target` / `desire_needs`. Also Inf if `previous_growth == count` (grew from zero old count).
- **Fix idea:** Guard before dividing: if old count is ~0, skip scaling or use `1.0`; if `count == 0`, zero targets and/or skip dead pops before `update_desires`. Assert/clamp the `previous_growth` invariant after `growth_phase`.

### 2. Satisfaction rescale divides by zero
- **File:** `src/game/pop.rs` (~364)
- **What:** Existing-desire resync does `desire.satisfaction *= new_amount / desire.amount`. Zero `desire.amount` yields Inf/NaN and breaks `tiers_satisfied()` / growth.
- **Fix idea:** If `desire.amount` is ~0, set satisfaction to 0 (or only preserve ratio when both sides non-zero). Same caution in `Desire::tiers_satisfied` and `satisfy_one_desire` if needed.

### 3. Household rebuild wipes non-demographic mods
- **File:** `src/game/pop.rs` (~303–321)
- **What:** `rebuild_household_from_demographics` always starts from `HouseholdDef::default()` and only adds species/culture/religion modifiers. Institutional / day-start mods baked into `demographics.household` get wiped whenever `household_changed` is set. Phase 4 runs before market/production, so reapply may never happen.
- **Fix idea:** Either rebuild as `base_demo_household + institutional_overlay` (overlays stored separately), or document that institutional effects must be reapplied after every demographic rebuild and wire that into `phase_player_bonuses_and_demographic_updates` / day-start.

### 4. Mortality vs Birthrate desire effects are identical
- **File:** `src/game/pop.rs` (~686–690)
- **What:** `tier_desire_effect_growth` treats `DesireEffect::Mortality` and `DesireEffect::Birthrate` with the same arms (`+v*sat` bonus, `-v*lack` malus). Docs distinguish them; `growth_phase` even TODOs separating birth vs mortality for multiplicative stacking. As written the variants are interchangeable for net rate.
- **Fix idea:** Decide semantics. e.g. Birthrate only adds; Mortality only subtracts — or keep net-identical arms but accumulate birth/mortality separately before combining (match the TODO).

---

## Suggestions

### 5. `DemographicEffect` never applied to household modifiers
- **File:** `src/game/demographiceffect.rs` (and species/culture/religion)
- **What:** Effects are stored (`species_effects` / `culture_effects`, etc.) but nothing folds them into `*_household_modifiers`. `demographic_update` only reads pre-baked modifiers. Setting `household_changed` without updating modifiers is a silent no-op for intended effect changes.
- **Fix idea:** `fn apply_to_household(&self, h: &mut HouseholdDef)` (or rebuild modifiers from effects whenever effects change) as single source of truth before setting `household_changed`.

### 6. Property scaling by `previous_growth` is not idempotent
- **File:** `src/game/pop.rs` (~381–390)
- **What:** A second `update_desires` / `demographic_update` in the same day (or before `growth_phase` overwrites `previous_growth`) multiplies targets by the same factor again.
- **Fix idea:** Scale from an absolute baseline, zero `previous_growth` after applying property scale, or track “already scaled this turn.”

### 7. Common/luxury growth terms may have wrong sign
- **File:** `src/game/pop.rs` (~632–638)
- **What:** Common/luxury use `-k * tier_total_satisfaction` (penalize *having* satisfaction). Basic correctly penalizes *lack* (`-0.30 * (1 - sat)`). Common coeff `0.0002` is nearly a no-op vs luxury `0.005`. Confirm intent (wealth/demographic-transition effect vs copy-paste inversion).
- **Fix idea:** If unmet needs should hurt growth, use lack. If wealth reduces births, document that next to basic’s opposite sign.

### 8. `demographic_update` not wired into turn loop
- **File:** `src/playstate.rs` (~104–106)
- **What:** `phase_player_bonuses_and_demographic_updates` is still `todo!()`, so `Pop::demographic_update` never runs from the turn loop, and nothing clears shared `household_changed` flags after all pops update.
- **Fix idea:** When filling the stub: apply player demographic edits → `par_iter` `pop.demographic_update` → clear `household_changed` on touched factuals (orchestrator clears flags, not `Pop`).

### 9. Household size change with fixed count jumps total pop
- **File:** `src/game/pop.rs` (~303–321)
- **What:** Rebuild changes adults/children/etc. while leaving `demographics.count` fixed, so total population (`count * size()`) jumps immediately. Species/culture docs call this out; `Household::alter_household_maintain_members` exists but is unused.
- **Fix idea:** For size-affecting changes, conserve members (adjust count) or phase the delta over turns; at least `debug_assert` large swings in dev.

### 10. `growth_phase` takes unused `factuals`
- **File:** `src/game/pop.rs` (~617–648)
- **What:** `growth_phase` takes `factuals` but ignores it; growth only uses already-baked household. Combined with #5, mid-turn effect-only factual edits never affect growth.
- **Fix idea:** Drop unused param until needed, or resolve baseline birth/mortality from factuals if household is not sole source of truth.

### 11. Test coverage gaps
- **File:** `src/game/pop.rs` (~1480–1636 and update_desires tests)
- **What:** `growth_phase` tests miss: `count == 0`, rate ≤ -100% before snap, Birthrate malus / Mortality bonus arms, household birth/mortality + desire interaction. `demographic_update` tests miss: missing factual species id, rebuild wiping custom overlays, efficiency-only modifiers.
- **Fix idea:** Dead-pop + `update_desires` property-scale test (catches #1); overlay-wipe test (catches #3).

---

## Nits

### 12. Religion/species field names and docs say “Culture”
- **Files:** `src/game/religion.rs` (~24–33), `src/game/species.rs` (~24–34)
- **What:** Religion has `culture_effects` / `culture_household_modifiers` and Culture-flavored comments; species docs also say “Culture” in places.
- **Fix idea:** Rename religion fields to `religion_*` and fix comments.

### 13. Growth test comment missing parentheses
- **File:** `src/game/pop.rs` (~1389–1411)
- **What:** Comment writes `growth_f = count / count - previous_growth` and `10 / 10 - 5 = 2.0`; code correctly uses `(count - previous_growth)`.
- **Fix idea:** Comment as `count / (count - previous_growth)`.

### 14. Unreachable code after Class `todo!`
- **File:** `src/game/factuals.rs` (~128–131)
- **What:** `DesireSource::Class` arm is `todo!(...); None` — `None` is unreachable.
- **Fix idea:** Keep only `todo!(...)` until Class exists.

### 15. `decay_goods` takes `&self` but will need `&mut self`
- **Files:** `src/game/actors.rs` (~27–29), players equivalent
- **What:** Signatures will have to change when implemented and will ripple through `phase_good_decay`.
- **Fix idea:** Use `&mut self` now so call sites stay stable.

---

## Earlier review leftovers (pre-`1c06bb3`, may still apply)

From the prior `update_desires` review (`1a22072^..1c06bb3`). Some may have been fixed in this range; re-check before treating as open.

1. Growth test vs formula mismatch (may be fixed in `3884c6b` / `ce61887` — tests passed at latest review).
2. Update path leaving `effect` / `decay` / `scalar` stale on existing desires.
3. Tier changes never move desires between tier vecs.
4. Desires kept if demo still exists on *any* demographic after culture/religion/species conversion (stacking).
5. `source_demo_desire` panics on missing parent demographic (`find_*`) instead of returning `None`.
6. `AGENTS.md` still names `apply_scaling_factor` vs `get_scaling_factor`.

---

## Suggested priority when picking these up

1. Numeric guards (#1, #2) — cheap, prevent silent NaN corruption.
2. Wire or clearly defer demographic turn phase (#8).
3. Household rebuild / overlay design (#3, #5, #9) — design decision before more turn work.
4. Birthrate vs Mortality semantics (#4, #7).
5. Idempotency and tests (#6, #11).
6. Nits whenever touching those files.
