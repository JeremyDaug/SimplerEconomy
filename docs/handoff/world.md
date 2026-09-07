# World data

Read this only for factuals, `data/world/` TOML, or gameplay config.

## Landed vs stub

| Piece | Status |
|-------|--------|
| Goods, processes, config load | `Factuals::load_from_path` on `data/world/` |
| Species / culture / religion | Still in-code |
| Initialization data (starting pops/firms/prices) | Not started |
| Save data | Not started |

Split: **world data** (factuals, human-editable) vs **init data** vs **save
data**. Duplicate good or process ids error. Duplicate process input goods
error. Missing config keys keep compiled defaults. Mid-game config edits are
invalid.

Live paths read `factuals.config` (`GameConfig`). Load returns **every** bound
failure in one `ConfigLoadError::Invalid` list. Compile-time `*_constants` are
Default and unit-test fallback. Buffer sizes (`HISTORY_MAX`, `AMV_HISTORY_MAX`)
stay compile-time. Do not add a process-global OnceLock; tests run in parallel.

**Code:** `src/game/factuals.rs`, `src/game/config.rs`, `data/world/goods.toml`,
`processes.toml`, `config.toml`.
