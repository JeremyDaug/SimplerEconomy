# Turns / PlayState

Read this only when wiring `PlayState`, `advance_turn`, or a day phase.

`advance_turn` has several stub leaves. Scale: unique demographic combos, not
cartesian precompute.

**Live day (tester / intended lib order):**
`start_day` -> wages -> `run_market_day` -> `run_production` -> pop consume /
sentiments / records -> firm `record_keeping` (`plan`) -> decay.

Vault `Turns.md` puts firm planning before consume. Call that conflict out; do
not silently "fix" either side.

## Wired vs stub

| Piece | Status |
|-------|--------|
| Pop consume / growth / record keeping / decay | Wired |
| Sentiments | After growth, before migration |
| `MarketLookups` | Rebuilt at sentiments and record keeping |
| `extract_special_resources` | Runs; yield discarded |
| Firm `record_keeping` | Rolling average + `plan` |
| `phase_intra_market_day` | `todo!()` (`run_market_day` exists in lib) |
| Production phase | `todo!()` (`run_production` exists; tester calls it) |
| Day-start | `Pop::start_day` exists, "Completed not Connected" |
| Institution / state orders | Not collected |
| Migration leaves | Orchestrator exists; leaves `todo!()` |
| Market / institution / state record_keeping | `todo!()` |

Pipeline checklist: `TODO.md` (only if that is the task).

Stale (notify only): PlayState record-keeping comment still says the only shared
input is factuals; some playstate/firm/institution docs still mention
`Pop::demographic_update`.

**Code:** `src/playstate.rs`.
