# World data

Read this only for factuals, `data/world/` TOML, or gameplay config.

## Landed vs stub

| Piece | Status |
|-------|--------|
| Goods, processes, config load | `Factuals::load_from_path` on `data/world/`. Goods catalog is `goods.toml` (keep existing ids; new goods append) |
| Species / culture / religion | Still in-code |
| Initialization data (starting pops/firms/prices) | Scenario TOML in `data/init/` (`pops.toml`, `firms.toml`). Human-editable. Not a save. Starting prices still tester-side (`roster.rs`). |
| Save data | Not started |

Split: **world data** (factuals) vs **init data** (scenario kickoff) vs **save
data** (later, compressed, not for hand-editing). Init files stay short:
shared desires, optional starter, one line per pop/firm, names or ids, defaults for
owner/remainder/household. Current scenario has two pops and two remainder-owner
firms per world good, line `target` 8 (grain/wood 16, water 20, cabins 2). Pops open with one day of the matching firm's output (Time skipped) so day 1 has tenders. Firm `target` is process iterations; opening
stock is three decay-adjusted days of each process output (yesterday succeeded; Time
output skipped) plus four days of required non-Time inputs (`use_target` = one day
of recipe use so they are not sold). Hours default to target * Time input. Root arrays
(`starter`, `pops`) must sit above `[[desires]]` so they are not swallowed
by the last desire table. Duplicate good or process ids error. Duplicate
process input goods error. Missing config keys keep compiled defaults.
Mid-game config edits are invalid.

Live paths read `factuals.config` (`GameConfig`). Load returns **every** bound
failure in one `ConfigLoadError::Invalid` list. Compile-time `*_constants` are
Default and unit-test fallback. Buffer sizes (`HISTORY_MAX`, `AMV_HISTORY_MAX`)
stay compile-time. Do not add a process-global OnceLock; tests run in parallel.

Catalog load tests sample a few goods (Time, one staple, one later id). Do
not enumerate the file or assert an exact goods count. Missing `decay_rate`
defaults to **1.0** (full daily decay). Live world goods use per-good rates
in `goods.toml` (Time still 1.0; other rates were pulled back so a few
days of stock survive, food still faster than metal). `mass` / `volume` are kg and m³ per
game unit; bulk is `mass + 400 * volume`. Time is 0/0. Tokens are light;
cabins are bulky.
Processes are one recipe per good. Time is process 28 so process id 0
stays none. Raw extracts (grain, water, gold, wood, iron, copper, tin,
bronze, coal, clay) and the Time dummy take Time only. Crafted recipes
take Time plus at least one destroyed material. Outputs are a few units
(1..=8); Time input is usually below 1.0. Sample grain (Time-only), pots
(clay + coal), and Time.

**Code:** `src/game/factuals.rs`, `src/game/config.rs`, `src/game/init.rs`,
`data/world/goods.toml`, `processes.toml`, `config.toml`, `data/init/`.
