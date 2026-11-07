# Live surface

Confirm against the working tree. The longer note is `docs/handoff/current.md`.

## Landed

- `Good` and `Process`, including `Process::do_process` and `do_process_leg`.
- `Household`, `BirthSentiment`, `DemographicRates`, and `HouseholdTarget`, including their methods and tests. `Household::update` reads `PopConfig`.
- Loaders: `Good::load_from_path` / `load_from_toml`, `Process::load_from_path` / `load_from_toml`, `GameConfig::load_from_path` / `load_from_toml`. `Factuals::load_from_path` reads a world folder or one TOML document. See `docs/handoff/loaders.md`.
- `data/world/goods.toml` and `data/world/processes.toml` are the world tables. `data/world/config.toml` leaves its keys commented. `pop` and `market` hold the tunables. The other config sections are empty.
- `Map` wrapping, regions, `add_city`, `add_to_region`, and the map tests.
- `src/game/util.rs`: `random_index`, `lerp`, `whole_units_up`. `Household` calls `lerp`. The other two have no caller.

## Present, not a day loop

Fields stay. Add a method only when asked. `#![allow(dead_code)]` is on desire, job, market, pop, and species.

`PlayState`, pop, job, desire, market, firm, map data, sentiment, species, culture, institution, workforce. `Unit`, `TechTree`, and `FirmOrganization` are empty shells. `Plot` is the terrain enum. `Technology` has node fields. Household is not a stub.

Species and cultures are not loaded. There is no stratum or religion map. `DemographicSource::Religion` returns no desire.

## Gone from this tree

Religion, stratum, deal, `DealMaker`, contract, ladder, craft, `examples/pop_tester`, `examples/firm_tester`, `examples/rates_tester`, `data/pop_tester`, `data/firm_tester`, `data/scenarios`, `data/world/crafts.toml`.

## Check

`cargo test --lib`
