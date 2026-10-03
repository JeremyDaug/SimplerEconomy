# Review

Commit `0864c3dd` — cottage jobs run inside `Market::market_day`, and `examples/pop_tester` loads `data/world` plus `data/pop_tester/scenario.toml`.

Slices with nothing to change are left out. That includes the day order in `Market::market_day`, `Job` reserve and produce, and the process-complexity change. The time AMV print of `-0` is accepted.
