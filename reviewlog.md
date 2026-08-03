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

**Last updated:** review of `0d9a57e..HEAD` (through `43990ca`)  
**Scope:** B1/B2/B4 fixes; Institution/decay; `Sentiment`; first draft
`process_satisfaction`; stored Birthrate/Mortality in `growth_phase`  
**Diff:** 16 files, +2685 / −470 · `cargo test --lib` 120 passed

---

## Open bugs

### B3. Mood/satisfaction `PopEffect`s still never applied in a real turn
- **File:** `src/playstate.rs` (no `process_satisfaction` call); `src/game/pop.rs` (`process_satisfaction`, `decay_goods`)
- **What:** Growth now applies/removes stored `Birthrate`/`Mortality` (partial B3 fix). `process_satisfaction` applies Satisfaction + Sentiment **when called**, but the day pipeline never calls it. Those effects sit until EOD decay, which expects only `BonusGood` — debug assert / silent drop in release. Migration/record-keeping never see updated `sentiment` / `recorded_tier_sat`.
- **Fix idea:** Wire a mood/satisfaction phase **after growth** (docs settled: consume → growth → `process_satisfaction`) with `par_iter_mut` `pop.process_satisfaction()`. Keep open until wired or explicitly deferred.

### B6. Firm/institution `decay_goods` still `todo!()` under live fan-out
- **File:** `src/game/firm.rs` (~90–93), `src/game/institution.rs` (~167–170), `src/game/actors.rs`, `src/playstate.rs` `phase_good_decay`
- **What:** `Actors::decay_goods` always fans out to every firm and institution. Bodies are still `todo!()`, so any non-empty firms/institutions map panics when phase_good_decay runs. Pop decay is real; partial implementation made fan-out a landmine.
- **Fix idea:** No-op stubs until real decay exists, or skip empty-logic maps until implemented.

---

## Open suggestions

### 1. `DemographicEffect` never applied to household modifiers
- **File:** `src/game/effects.rs` / species/culture/religion `*_effects`
- **What:** Effects are stored but nothing folds them into `*_household_modifiers`. Setting `household_changed` without updating modifiers is a silent no-op.
- **Fix idea:** `fn apply_to_household(&self, h: &mut HouseholdDef)` (or rebuild modifiers from effects whenever effects change).
- **Related:** deferred D1.

### 2. Property scaling by `previous_growth` not idempotent / can go Inf
- **File:** `src/game/pop.rs` (update_desires property scale)
- **What:** Second `update_desires` in the same day multiplies again. Denominator `0` yields `Inf` (NaN assert does not catch it).
- **Fix idea:** Scale from absolute baseline, zero `previous_growth` after apply, assert finite growth factor.

### 3. Common/luxury growth terms may have wrong sign
- **File:** `src/game/pop.rs` (`growth_phase`)
- **What:** Common/luxury use `-k * tier_total_satisfaction` (penalize *having* satisfaction). Basic penalizes *lack*. Common coeff nearly a no-op.
- **Fix idea:** Confirm wealth/transition intent and document, or use lack. Related: deferred D2.

### 4. `demographic_update` not wired into turn loop
- **File:** `src/playstate.rs` (`phase_player_bonuses_and_demographic_updates`)
- **What:** Still `todo!()`. Institution effect push and `Pop::demographic_update` never run; nothing clears `household_changed`.
- **Fix idea:** Institutions/firms → `par_iter` demographic_update → orchestrator clears flags → non-demo household effects (D1).

### 5. Household size change with fixed count jumps total pop
- **File:** `src/game/pop.rs` (`rebuild_household_from_demographics`)
- **What:** Rebuild changes adults/children/etc. while leaving `count` fixed; total pop jumps. `alter_household_maintain_members` unused.
- **Fix idea:** Conserve members or phase deltas; `debug_assert` large swings in dev.

### 6. `growth_phase` takes unused `factuals`
- **File:** `src/game/pop.rs`
- **What:** Param ignored. Mid-turn effect-only factual edits never affect growth without household rebake.
- **Fix idea:** Drop until needed, or resolve baseline rates from factuals if household is not sole source of truth.

### 7. Test coverage gaps (growth / demographic edges)
- **File:** `src/game/pop.rs` tests
- **What:** Reservation/decay/process_satisfaction/sentiment tests help. Still missing: growth rate ≤ -100% before snap, Birthrate malus / Mortality bonus arms, household birth/mortality + desire interaction; demographic missing-species / efficiency-only modifiers.
- **Fix idea:** Lock intended growth and rebuild semantics once designs firm up. Also add turn-loop integration once B3 is wired.

### 8. Satisfaction effect docs still say “units” / “clamps”
- **File:** `src/game/effects.rs` (`DesireEffect::Satisfaction`, `PopEffect::Satisfaction`)
- **What:** Docs say “extra satisfaction units” and “Common clamps to one full level.” Implementation + proposal treat boosts as ratio-mass / fill boosts with no common hard cap on recorded tier fill.
- **Fix idea:** Rewrite docs to match proposal and `process_satisfaction` (fill boost, never basic, no common hard cap; luxury open-ended).

### 9. Baseline sentiment daily drift
- **File:** `src/game/pop.rs` (`process_satisfaction` baseline mods)
- **What:** Empty common/luxury treated as `1.0`; a “fine” pop still gets daily happiness/contentment/hope bumps with no pull-to-content. Draft-fine, easy to over-trust for migration/politics.
- **Fix idea:** Document as draft daily pulse; consider dampening or return-to-content before relying on sentiment elsewhere.

### 10. Invalid Satisfaction tier silently dropped in release
- **File:** `src/game/pop.rs` (`process_satisfaction` boost pass)
- **What:** Invalid tier (not 1/2) fails `debug_assert` then is neither applied nor kept — silent drop in release.
- **Fix idea:** Keep invalid arms for decay assert, or log; do not vanish without a trail.

---

## Open nits

### 11. Religion/species field names and docs say “Culture”
- **Files:** `src/game/religion.rs`, `src/game/species.rs`
- **Fix idea:** Rename religion fields to `religion_*` and fix comments.

### 12. Growth test comment missing parentheses
- **File:** `src/game/pop.rs` (property growth scale test)
- **Fix idea:** Comment as `count / (count - previous_growth)`.

### 13. Unreachable code after Class `todo!`
- **File:** `src/game/factuals.rs`
- **Fix idea:** Keep only `todo!(...)` until Class exists.

### 14. Residual `decay_goods(&self)` on players / mapdata
- **Files:** `src/game/players.rs`, `src/game/mapdata.rs`
- **Fix idea:** Use `&mut self` now so call sites stay stable.

### 15. Orphaned DesireEffect docs in desire.rs
- **File:** `src/game/desire.rs`
- **Fix idea:** Remove orphaned section (canonical on `effects::DesireEffect`).

### 16. Unused `DynamicArray` import / firm “ownser” typo
- **Files:** `src/game/pop.rs:3`, `src/game/firm.rs` (“ownser”)
- **Fix idea:** Drop import; fix typo.

### 17. `process_satisfaction` drains `stored_effects` twice
- **File:** `src/game/pop.rs`
- **What:** Satisfaction pass then Sentiment/growth/bonus pass. Correct but easy to desync.
- **Fix idea:** Single drain with one match when next touching the function.

### 18. Unused `factuals` bind in `phase_pop_consumption`
- **File:** `src/playstate.rs`
- **Fix idea:** Drop until consume needs definitions.

---

## Deferred / accepted (explained, not open bugs)

### D1. Household rebuild and non-demographic mods
- **Was:** Bug — rebuild from default + demo modifiers wipes institutional / day-start overlays.
- **Dev response:** Intended order is demographic effects first, then other effects, in `phase_player_bonuses_and_demographic_updates`.
- **Status:** Deferred / WIP with open suggestion #4.

### D2. Mortality vs Birthrate desire effects identical in net growth
- **Was:** Bug — same arms for both variants in `tier_desire_effect_growth`.
- **Dev response:** Known; birth/mortality assumed non-negative and only sum into one net rate today.
- **Status:** Deferred design debt. Related open suggestion #3.

---

## Resolved (this and prior reviews)

### R1. Dead-pop `growth_f` → NaN
- **Resolution:** `debug_assert` + lifecycle invariants. (Inf on zero denominator still open under suggestion #2.)

### R2. Satisfaction rescale divides by zero
- **Resolution:** `debug_assert!(desire.amount >= 1.0)` (see R6).

### R3. Actors `decay_goods` / Pop decay implementation
- **Resolution:** Fan-out + `Pop::decay_goods` real (used return, rate decay, consumed destroy, byproducts, desire + stored bonus goods). Residual: firm/institution todos (B6), players/mapdata (nit #14).

### R4. B1 — reserved stock vs `saved`
- **Resolution:** `consumeable` removed; satisfy draws from full `quantity`; `saved` is wish-only.

### R5. B2 — reserve vs satisfy target order
- **Resolution:** Both use `Desire::ordered_targets()`.

### R6. B4 — amount assert `>` vs `>=`
- **Resolution:** `debug_assert!(desire.amount >= 1.0)` matches docs.

### R7. B3 partial — stored growth arms
- **Resolution:** `growth_phase` applies and removes stored `Birthrate`/`Mortality` (test present). Residual open as B3 (mood path + turn wire-up).

### R8. B5 — process_satisfaction phase order vs growth
- **Resolution:** Docs/proposal/vocabulary place `process_satisfaction` **after consume and growth**. Step 5 `debug_assert`s if stored Birthrate/Mortality remain (must be applied in `growth_phase`); only BonusGood is re-kept for decay. Release builds drop stray growth arms rather than re-queue them.

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

1. Wire `process_satisfaction` into the turn loop after growth (B3).
2. No-op firm/institution `decay_goods` stubs (B6) so EOD does not panic.
3. Wire demographic turn phase (#4) — unblocks D1 / institution passives.
4. Household size conservation (#5) and `DemographicEffect` → modifiers (#1).
5. Growth semantics: common/luxury sign (#3), Inf/idempotency (#2), later D2.
6. Satisfaction/sentiment polish (#8–#10) while process_satisfaction is hot.
7. Nits (#11–#18) whenever touching those files.
8. Re-verify earlier leftovers when next touching `update_desires`.
