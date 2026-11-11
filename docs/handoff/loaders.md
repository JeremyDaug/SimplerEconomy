# Loaders

How world data enters the library. The files are `data/world/goods.toml`, `data/world/processes.toml`, and `data/world/config.toml`.

## Goods

`Good::load_from_path` and `Good::load_from_toml` live in `src/game/good.rs`.

The document's `goods` array is read. Other tables are ignored. Each row deserializes as a `Good`. A missing `decay_rate` is 1.0. Tags use the `GoodTag` names (`fixed`, `exposure`, `untradeable`, `skill`, `transport`).

The result is a `HashMap` keyed by id. A repeated id is `GoodLoadError::Duplicate`.

## Processes

`Process::load_from_path` and `Process::load_from_toml` live in `src/game/process.rs`.

The document's `processes` array is read. Other tables are ignored. The caller passes the goods already loaded, keyed by id.

An empty goods map does not check good ids, so a process document can load on its own. A non-empty map rejects a good id that is not in it.

Complexity defaults to 1.0. Below 1.0 is subsistence (`Process::is_subsistence`). Complexity and every input and output amount must be finite and greater than 0. A process may name each good once as an input.

Input `type` is `destroyed` (the default), `consumed`, `capital`, or `factor`. `optional = true`, or any `optional_effects` entry, makes the input optional. Those effects are `throughput`, `input`, `output`, `extra_output`, and `birth_rate`.

Process `effects` are `development`, `unity`, `faith`, `authority`, `legitimacy`, and `birth_rate`. `MortalityRate` exists on `ProcessEffect` and is not a world-data key.

Errors are `ProcessLoadError`: duplicate process id, duplicate input good, or `Invalid` with the reason.

## Factuals

`Factuals::load_from_path` accepts a file or a directory.

A directory reads `goods.toml` (required), then `processes.toml` when present, then `config.toml` when present. Goods and processes go through the loaders above. Config goes through `GameConfig::load_from_path` and replaces `Factuals.config`.

A file is one TOML document. `Factuals::load_from_toml` reads the goods array, then the processes array, from that document. Processes are checked against the goods already stored plus the goods in the document. A duplicate against either map already stored is `FactualsLoadError::DuplicateGood` or `DuplicateProcess`. The goods and process errors convert into `FactualsLoadError`.

Species and cultures are not read. Those maps stay empty. There is no stratum or religion map.

## Config

`GameConfig::load_from_path` and `GameConfig::load_from_toml` live in `src/game/config.rs`.

Missing keys keep `Default`. `pop` has `childhood_years`, `adulthood_years`, `child_sex_ratio`, `partnership_trend`, `turn_length` (default 1, at least 1), `subsistence_anchor` (default 0.6, must be above 0), `subsistence_common_share` (default 0.5, from 0 to 1), and `desire_share_minimum_divisor` (default 2, either 0 or at least 1). `market` has `amv_epsilon`, `salability_default`, `price_gain` (default 0.10, above 0), `price_cap` (default 0.20, above 0 and at most 1), `migration_anger_weight`, `migration_pace`, `founding_households`, `founding_min_households`, `founding_drought_step`, `founding_both_share`, and `founding_available_drop`. `labor` has `time_per_labor`, default 16, and it must be above 0. The other sections have no keys. `validate` checks those bounds. The world file leaves the keys commented, so a load keeps the defaults.
