# Current tree

Working note for the fresh start. Read this before editing the library.

`Design_V3.md` is the design source of truth. The user owns it. Do not edit it. It is an intent document, not a code spec, and it is still being written. Where it is silent, leave the gap. `docs/Overview.md` and `TODO.md` describe the previous tree.

## What this crate is

Rust 2024 library, crate `simpler_economy`. No binary. No Bevy in simulation code. No examples. `std` collections. `hexx` for the hex map.

World data is `data/world/goods.toml`, `data/world/processes.toml`, and `data/world/config.toml`. The load path is [loaders.md](loaders.md).

`cargo test --lib` is the check.

## Live

Goods, processes, and households are the live systems.

`Good::load_from_toml` and `Process::load_from_toml` load world data. `TIME` is good id `0`. `Process::do_process` and `do_process_leg` stay, including recipe bonuses and the side-effect arms (research, culture, faith, authority, legitimacy, birth, mortality). Do not start a strip of `process.rs` unless asked.

`Household`, `BirthSentiment`, `DemographicRates`, and `HouseholdTarget` stay whole, including their methods and tests. `Household::update` reads `PopConfig` for childhood years, adulthood years, the birth sex ratio, and the partnership step. `HouseholdTarget` is also what processes store. `Household` calls `lerp` in `src/game/util.rs`.

`Factuals` loads a world folder from goods, then processes, then config. It keeps maps for species and cultures. A world folder does not load those maps.

`GameConfig` sections are `pop`, `player_resources`, `market`, `deal`, `market_priority`, `labor`, and `firm`. `pop` and `market` hold the tunables in [loaders.md](loaders.md). The other sections are empty. Missing keys keep `Default`.

`Map` stores the hex grid, wrapping, regions, and city hexes. `add_city` and `add_to_region` are in. The map tests cover that.

## Present, not a day loop

These types are in the tree. Keep the fields. Add a method only when asked.

`PlayState` holds factuals, map data, actors, players, the turn, and market lookups. It has no methods.

`Pop` is one household block, a `Job`, property rows, savings, desires, sentiment, and a demographic row of species and culture. `Job` is the cottage lines: a process, a target, and the optional inputs that line may draw.

`Desire` has platonic, demographic, and pop desires, with constructors, `set_amount`, `create_desire`, and target ordering.

`Market` holds members, goods, and price history. `MarketHistory::from_config` reads `salability_default`.

`Firm` is one workshop: owners, workforce, property rows, and production lines. `MapData` stores the map and markets. `market` and `market_mut` are the getters. `decay_goods`, `process_organized_migration`, and `process_inter_market_migration` panic.

`Species` and `Culture` keep their fields and desire lists. `Sentiment` is the five-way partition, with its tests. `Institution`, `Workforce`, `PlayerResources`, `Players`, `Nation`, effects, `ScalingFactor`, `Tile`, and `Region` keep their fields. `Plot` is the terrain enum. `Technology` is a node with id, cost, parents, and unlock lists. `TechTree` is empty. `init` is the module comment for scenario kickoff.

`Unit` and `FirmOrganization` are empty shells.

`src/game/util.rs` has `random_index`, `lerp`, and `whole_units_up`. `lerp` is used by `Household`. The other two have no caller.

`#![allow(dead_code)]` is on `desire`, `job`, `market`, `pop`, and `species`.

`DemographicSource` is species, culture, or religion. `Factuals` has no religion map. `source_demo_desire` returns `None` for a religion source.

## Gone from this tree

These are not modules. Leave them out unless asked.

Religion, stratum, deal, `DealMaker`, contract, ladder, craft, the pop tester, the firm tester, and the scenario data folders.
