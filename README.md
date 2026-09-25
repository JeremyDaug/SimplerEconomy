# Simpler Economy

Working title. The design pin is [docs/Overview.md](docs/Overview.md).

Simpler Economy is a market-oriented grand-strategy game: a long historical span, a detailed barter-first market, and civilizations built from institutions, pops, and regions. The market is the keystone. Players mostly massage production, consumption, and logistics. They do not operate them directly.

This repository is a reboot of a larger project. What remains is the simulation kernel kept for that reboot: goods, processes, pops, households, firms, and markets, with world data under `data/world`. There is no game client.

```text
cargo test --lib
cargo run --example rates_tester
```

`rates_tester` probes household demographics. Other branches, and the old Obsidian vault, are outside this reboot. Open them only when a specific target is named.
