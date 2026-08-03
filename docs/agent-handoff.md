# Agent handoff — EconCiv rework (pop / satisfaction / config)

**Branch:** `EconCiv-Rework-Branch` (tracks `origin/EconCiv-Rework-Branch`)  
**Last commit on branch tip:** `f467272` — *Update reviewlog for 0d9a57e..43990ca review*  
**Handoff date:** 2026-08-03  
**Purpose:** Feed a new agent/session. Prefer this + `AGENTS.md`, `STYLE.md`, `todo.md`, `reviewlog.md`, and `docs/design-vocabulary.md` over inventing process from scratch.

**Build status at handoff:** **does not compile.** Working tree is mid-migration on living-standard / `PopRecords` (see §5).

---

## 1. Project orientation (stable)

Rust economic / civilization sim. **Authoritative design** lives in the local Obsidian vault:

- Primary: `/home/jeremy/Documents/Obsidian Vault/Game Ideas/EconCiv/`
- Historical: `…/Simlper Economy Simulator/` (prefer EconCiv when they disagree)

In-repo:

| File | Role |
|------|------|
| `AGENTS.md` | Project rules, vault paths, code map, build commands |
| `STYLE.md` | Coding style (builders, tests `*_should`, f64, etc.) |
| `docs/design-vocabulary.md` | **Canonical names** (tier sat, desire sat, fill boost, …) |
| `docs/proposals/` | Longer proposals (satisfaction boosts, institutions) |
| `todo.md` | Working checklist |
| `reviewlog.md` | Open review debt (update when reviewing) |

```bash
cargo check --lib
cargo test --lib
```

Prefer lib check/test over full Bevy binary unless needed.

---

## 2. Recent commits (focus window)

### `9620362` — Sentiment + STYLE

- Added `STYLE.md`; slimmed `AGENTS.md` toward “point at STYLE/vault.”
- New `src/game/sentiment.rs`: share-based mood axes, flat/relative mods, blend, transfers, tests.
- Groundwork for `process_satisfaction`.

### `43990ca` — `process_satisfaction` first draft + vocabulary

- Large `Pop::process_satisfaction` draft in `pop.rs`.
- Design vocabulary + satisfaction-ratio proposal docs.
- Effects docs for Satisfaction / Sentiment arms.
- **Intent:** after consume + growth, apply fill boosts, write day records, baseline + desire + stored sentiment, keep BonusGood for decay; growth arms must already be drained in `growth_phase`.

### `f467272` — Reviewlog refresh

- Documented what that review closed vs left open (unwired mood phase B3, phase-order B5, firm/institution decay B6, boost/sentiment polish, etc.).

### Slightly older (context)

| Commit | Notes |
|--------|--------|
| `f0264cc` | Institution draft shape; pop `decay_goods`; actors mut-decay |
| `0d9a57e` / `f18dc6c` | Pop day work (growth, reservations, update_desires, etc.) |
| `e506fa6` | Bugfixes post-review |

Full day-pipeline wiring (playstate) is still mostly stubs; pop **logic** is ahead of **turn loop** calls.

---

## 3. What exists on tip (committed) that new work builds on

### Pop day logic (in `pop.rs`, large file ~3k LOC)

Rough order of concern (docs settled; not all wired in `playstate`):

1. Reservations / satisfaction decay  
2. Consume / satisfy desires  
3. **`growth_phase`** — applies stored Birthrate/Mortality  
4. **`process_satisfaction`** — fill boosts, records, sentiment (not called from turn yet)  
5. **`decay_goods`** — used return, decay, consumed wipe, bonus goods  

### Sentiment

- `Sentiment` + `SentimentMod` in `sentiment.rs`.  
- Baseline mood from tier sat + trend-based mods + desire/stored sentiment effects in `process_satisfaction`.

### Design language (use these terms)

- **Tier sat** — avg desire fill in a tier after boosts (`records.tier_sat`).  
- **Desire sat** — per-desire `satisfaction / amount`.  
- **Fill boost** — Satisfaction effects; never on basic; common may exceed 1.0 (surplus); luxury open.  
- **Common sat surplus** — common &gt; 1.0; mood uses full weight to 1.0, **half weight** above.  
- Prefer vocabulary file over chat shorthand (“ratio”, “tier fill”).

### Effects

- Consolidated documentation/enums in `effects.rs`; desire vs pop stored arms; growth vs mood vs bonus-good phase ownership.

---

## 4. Uncommitted work (working tree — important)

**Not committed.** Includes new modules and a partial rewrite of records/history.

### New / untracked modules

| Path | Role |
|------|------|
| `src/game/config.rs` | **Constants catalog** for tunables (not a pass-around `GameConfig`) |
| `src/game/pop_property.rs` | `DemoRow`, `PopPRow`, `PopRecords` extracted from pop |
| `src/game/util.rs` | Generic helpers; currently `lerp(a, b, t)` for f64 |

Registered in `src/game.rs`: `config`, `pop_property`, `util`.

Also modified: `AGENTS.md`, vocabulary/proposal docs, `reviewlog.md`, `todo.md`, **`pop.rs`** (large), etc.

### Config philosophy (decided this session)

- **Factuals:** world content objects; mostly static but *can* change in-game.  
- **Config constants:** more fundamental — *how* systems behave; **load once, change never** mid-run.  
- Rejected: passing `GameConfig` into every call (`push_with_config`, `process_satisfaction(&…, &config)`).  
- Adopted: `config::living_standard::*` constants + small helpers (`score`, `history_capacity`).  
- Future mods: resolve at boot → freeze; do not mutate knobs during the day.

Current living-standard knobs (names may still evolve):

- History: `HISTORY_MAX` (16), `HISTORY_LEN` (10), `history_capacity()`  
- Rolling: `ROLLING_AVG_WEIGHT` (0.25) — EMA α for SoL average  
- Legacy/extra (may be unused after redesign): `DEADBAND`, `TREND_ALPHA_*`, `TREND_SCALE`  
- Score weights: basic **1.0**, common **0.6**, luxury **0.4** (comments in records still match this style)  
- Sentiment *impact* gains/deadband: `SENTIMENT_*` (apply when mapping trend → mood, not necessarily into stored trend)

### Dependency already in Cargo.toml

- `circular-buffer = "1.2.0"` — prefer this over hand-rolled rings for SoL history.  
- User on rustc **1.91.x** → stay on **circular-buffer 1.x** (v2 may want newer MSRV).  
- API (v1): `CircularBuffer::<N, T>::new()`, `push_back`, overwrite when full.

### Generic math

- No Bevy scalar `f64` lerp; project uses f64 for quantities.  
- Place pure helpers in **`src/game/util.rs`** (user chose `util` over `math`).  
- `lerp(a, b, t) = a + (b - a) * t` (t unclamped).

---

## 5. Living standard / PopRecords redesign (in progress)

### Motivation

Author found earlier design (nested `LivingStandardHistory` + mirrored fields on `PopRecords`) **redundant** and **confusing trend with trend *impact*** (deadband, scale, clamp baked into stored trend). Direction: **one records surface**, clearer observables vs policy.

### Intended `PopRecords` shape (current `pop_property.rs` draft)

```text
tier_sat[3]
wealth_amv
satisfaction_units_total
living_standard     // today composite from tier_sat weights
sol_avg             // EMA of level
trend               // direction signal (redesigning)
sol_history         // CircularBuffer<HISTORY_MAX, f64> raw daily scores
```

Helpers drafted on `PopRecords`:

- `update_living_standard()` — weighted sum from `tier_sat` + config weights  
- `update_trend()` — first day seed; then `sol_avg = lerp(sol_avg, living_standard, ROLLING_AVG_WEIGHT)`; **current draft sets `trend = living_standard - prev_avg` (raw Δ vs previous average, not EMA of signal, no deadband/scale/clamp)**; push to `sol_history`

### Design decisions discussed (not all implemented)

| Topic | Direction |
|--------|-----------|
| Trend vs impact | Store readable change; apply deadband/gains/clamp only when building **sentiment** |
| Deadband / TREND_SCALE / clamp on trend | **Not required** for a trend line; optional noise filter / unit normalization; prefer full magnitude on stored trend |
| EMA | Level: `sol_avg = lerp(old, today, α)`. Trend: either raw δ, EMA of δ, OLS slope on ring, or endpoint chord — **author still consolidating** |
| Ring crate | `circular-buffer` |
| Config pass-around | No; constants module |
| Wealth in SoL | Wealth recorded; **not** in current composite score formula |

### Compile break (must fix next)

`pop.rs` still references the **old** API/fields:

- imports `LivingStandardHistory` (removed)  
- builds `PopRecords` with `wealth_amv_per_household`, `living_history`  
- calls `record_living_standard`  
- may double-assign wealth / satisfaction fields while also writing into `self.records` partially  

`PopRecords` now has `update_living_standard` / `update_trend` instead.

**Next coding step:** rewire `process_satisfaction` to:

1. Fill `tier_sat` / wealth / satisfaction totals on `self.records` (mutate in place; do not wipe history).  
2. `update_living_standard()` then `update_trend()`.  
3. Read `records.trend` / `tier_sat` for sentiment mods (gains in config; do not re-encode into trend).  
4. Drop remaining `LivingStandardHistory` references and broken tests.  
5. `cargo test --lib`.

---

## 6. Session design notes worth preserving

### EMA (level)

```text
new = lerp(old, today, α)   // same as α*today + (1-α)*old
```

### Minimal trend (author preference leaning)

```text
// after updating / reading prev avg:
trend = living_standard - prev_avg   // or EMA of that later
// sentiment later:
if |trend| big enough { mood += gain * trend }  // clamp effect here if needed
```

### Trend alternatives discussed

- Endpoint chord: `(last - first) / (n-1)` — middles cancel; only net move  
- OLS slope on ring — uses all points; still O(n) tiny  
- Half-window mean gap — very readable  
- EMA of day-to-day δ — O(1) state; “flat after crash” while level stays low  

### Config vs factuals

Do not treat config like a registry entity. Systems **obey** constants; they **look up** factuals.

---

## 7. Open priorities (from todo / reviewlog)

**P0 pipeline**

- Wire `process_satisfaction` into the day **after** growth (docs: consume → growth → process_satisfaction).  
- Demographic update / player-bonus phase still `todo!()`.  
- Firm/institution `decay_goods` stubs are landmines if actors decay runs.  

**P1 pop**

- Finish records/trend consolidation + compile green.  
- Sentiment → migration/politics later.  
- Baseline mood may need return-to-content / dampen for long runs.  

**Do not**

- Edit Obsidian vault unless user asks.  
- Drive-by refactors outside the task.  
- Silently invent a third model when vault/code disagree — **call out conflicts**.

---

## 8. Suggested first actions for a new instance

1. Read this file + `AGENTS.md` + `docs/design-vocabulary.md`.  
2. `cargo check --lib` — confirm errors still in `process_satisfaction` / `PopRecords` mismatch.  
3. Align `pop.rs` with `pop_property.rs` `PopRecords` API; remove dead history type.  
4. Decide final **trend** formula with user if still ambiguous (raw δ vs EMA of δ vs OLS); keep **impact** out of the stored value.  
5. Run `cargo test --lib`; fix `process_satisfaction_should` tests (living history len, field names).  
6. Only then consider wiring into `playstate` day loop.

---

## 9. File map (touched / new in this arc)

```text
src/game/config.rs          # NEW — living_standard constants
src/game/util.rs            # NEW — lerp
src/game/pop_property.rs    # NEW — DemoRow, PopPRow, PopRecords (+ trend helpers)
src/game/pop.rs             # process_satisfaction, growth, decay; MID-MIGRATE
src/game/sentiment.rs       # committed earlier
src/game/effects.rs         # satisfaction/sentiment docs + arms
src/game.rs                 # mod config, pop_property, util
docs/design-vocabulary.md
docs/proposals/satisfaction-ratio-and-boosts.md
docs/agent-handoff.md       # this file
AGENTS.md, STYLE.md, todo.md, reviewlog.md
Cargo.toml                  # circular-buffer already listed
```

---

*End of handoff. Update this file when the records/trend migration compiles and when process_satisfaction is wired into the turn.*
