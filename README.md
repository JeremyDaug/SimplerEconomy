# Simpler Economy

Working title. This branch builds V1, the market game: one market with simulated imports and exports, played like an incremental game, where the player runs a single firm. The design is [Design_V1_Market_Game.md](Design_V1_Market_Game.md). The build order is [TODO.md](TODO.md).

This repository is the simulation library. World data is `data/world/goods.toml`, `processes.toml`, and `config.toml`. The load path is [docs/handoff/loaders.md](docs/handoff/loaders.md). There is no game client yet.

```text
cargo test --lib
```
