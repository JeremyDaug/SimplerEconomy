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
owner/remainder/household. Current scenario is an eight-household village:
grain, water, bread, gold, wood, cabins (two grain shops, two wells). Specialty
`target` 8 on grain/water/wood, 5 on bread/gold, 2 on cabins. One line
each. Desires are food (grain/bread), water, wood heat,
one cabin per household, extra bread, and gold. Pops open with one day of the matching firm's specialty output (Time skipped) so day 1 has tenders. Firm `target` is process iterations; opening
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
Specialized recipes: process id equals the output good id, except Time
(good 0) which is process 28. Subsistence recipes are 29 farm, 30 water,
31 forage (`tags = ["subsistence"]`, complexity weight 0.25; untagged is
1.0; weight must be > 0). Raw extracts take Time only as a required input;
`make grain` and `make wood` may take optional boosters (water / wood_tools).
Crafted recipes take Time plus at least one destroyed material. Subsistence
is Time-only and weaker than the matching extract. Sample grain (optional
boosters), pots (clay + coal), Time, and subsistence farm. Init does not
attach those lines. Hours are the named line's Time. One line pays no
complexity tax. The living tester village is eight firms on a six-good
catalog (two grain, two water). Plan does not special-case the subsistence tag.

**Code:** `src/game/factuals.rs`, `src/game/config.rs`, `src/game/init.rs`,
`data/world/goods.toml`, `processes.toml`, `config.toml`, `data/init/`.
