# Tester CLIs

Read this only for the testers, `day`, CSV logs, or the living roster.
**Paused** except when asked. Do not add pages, commands, or extra CSV
series unless asked. Do not invent a second shopping model.

Two examples share the same world/init data and CLI shape:

```bash
cargo run --example market_tester
cargo run --example pop_tester
```

## Pop tester

Copy of `market_tester` with firms left out. Code: `examples/pop_tester/`
(`main`, `roster`, `format`, `parse`, `csv`). Loads pops from `data/init/`.
Init firms are read only to size each pop's morning stock cap: process
output `amount * target` (no scaling). Those firms are not kept on the
session. Default CSV stem is `pop_prices`.

Each morning: `start_day` Time, then top up to that cap (add only the
shortfall; already-at-cap stock is left alone), then market / consume /
decay / pop `record_keeping`. `day N` stops if any good AMV is negative
and prints the day and those goods. No firm production, plan, or
`keep_alive`.
Caps follow init firm `target` × process output (Time output caps pop 28).

## Market tester

`keep_alive on|off` is the emergency firm subsidy toggle (`firm.keep_alive`,
default off).

**Code / source of truth:** `examples/market_tester/` (`main`, `roster`,
`format`, `parse`, `csv`). Recipes: `data/world/processes.toml` (1 Time → 15
of the output, one process per good; Time is process 28). Starting pops and
firms: `data/init/` (`InitData`). Opening AMV/sal: `roster.rs`.
Opening AMV 100.0 / salability 0.1 (`roster.rs`).
Living roster: one pop and one firm per world good, 1 household each.
Pop labels are `pop{id}-{specialty}` (`pop1-grain`, `pop28-time`); `pop1` still parses.
Firm labels match (`firm1-grain`, `firm1`); `firm 1` still parses.
Each firm is the matching pop's remainder owner-operator, one specialty line,
`target` 10 (150 output), no wage basket. Opening stock is process outputs
times line target (sell target matches opening qty; posted sell is capped
by max market salability * daily output; Time starts empty). Pop morning specialty
grant is 0.
Grouped consume desires (basic food/hydration/heating/housing, common
utility/improved food/materials/health, luxury shiny tokens/libations) are
duplicated onto every pop at 1 unit per member (5 units). **No** opening
1-of-each kit (init starter empty; `DAILY_ENDOWMENT` 0). Boot `record_keeping`
writes shop targets. Each morning: `start_day` Time
(`TIME_PER_LABOR` 64 * household labor). Specialty output comes from the
firm (`FIRM_HOURS` 10; `pop.id % n_goods`; pop 28's firm makes Time and
cannot sell it). `DAILY_OUTPUT` is 0. Opening AMV is 100.0 and salability 0.1 on every good (no money good,
no price spread). Coin is `gold_token`; iron ore is `iron`.

Home is a short summary. Pages: `stock` / `orders` / `processes` / `amv` /
`day`. `home` / `cls` back. `shop` reloads books from `create_orders`. `match`
is **read-only** (does not move stock). `main.rs` is still the Bevy hex stub.

## `day` / `day N`

`market_tester` order below. `pop_tester` skips firm steps (2 wages are a
no-op, 4 production, 5 firm decay, 7 plan) and tops each pop up to its
init-firm process outputs after `start_day` instead of `DAILY_OUTPUT` 0.
`pop_tester` also stops `day N` when any good AMV is negative.

1. `Pop::start_day` (Time grant). Tester morning specialty grant is 0
   (`DAILY_ENDOWMENT` 0, `DAILY_OUTPUT` 0). Zero `income_amv`, reservations, firm
   `clear_day_flows`.
2. `Market::settle_labor` (pays contracts, stamps Time AMV). Hours, wage
   basket, and one-employer roster: `labor.md`. Time moves pop -> firm here.
3. `Market::run_market_day`. Time is untradeable transport; Time AMV is not
   leftover-book drift. Leftover books do not move AMV. Each market day AMV
   is rescaled to mean 10.0 after salability (firm quotes scale with it;
   the AMV trail is not rewritten), then the close is recorded.
4. `Firm::run_production` on **already-loaded** factuals. Do not reload
   `processes.toml`. A line starting from 0 snaps to 1. Outputs go to
   `held`; later lines may spend `held` after on-hand stock.
5. Pop `consume`, `update_sentiments` (market-day history), then
   `decay_goods`. Firm `decay_goods` next (`quantity` rots, then `held`
   joins `quantity`). Aggregate `(decayed, volume)`
   and `Market::cap_salability_from_decay`. Consumed is volume, not rot.
6. Pop `record_keeping` from the closing `MarketHistory` **after** decay and
   the rot cap so shop/save are not written against stock that will rot away
   and save ranking sees leftover-rot salability. Coin save/shop come from
   that rewrite (no tester cap).
7. Firm `record_keeping` (`plan`) after its decay (already done in step 5).
   Do not call pop `record_keeping` again.
8. `Market::budget_labor` (hours and wage amounts; Time AMV restamp;
   `budget_interval`, default 1).

Prints a `MarketDayReport` plus wages, production, plans, post-consume pop
stats, and AMV (now, day diff, trail trend, salability). `day N` adds a one-line digest and the last day's full report. Books reload from current stock
after the loop.

Working pops emit **requests and leftover offers**, with desires set
outright (not from demographics). No merchants. `build_world` returns one
remainder-owner firm per pop; line `target` 10 (hours follow Time input)
and opening stock is outputs times target. No cargo goods. Leftover AMV is lib (`market.md`), not tester.

## CSV

One-row-per-day under `data/logs/` (gitignored). Default stem `prices`.
`csv` shows paths; `csv <name>` changes stem; `csv reset` wipes headers.
`csv on <actor>...` / `csv off <actor>...` flag pops and firms; `csv off`
clears flags. **Header mismatch** (old layout or a changed flag set) errors
until reset or a new stem.

| File | When | Contents |
|------|------|----------|
| `{stem}_market.csv` | always | Per good: `amv`, `salability`, `average_price` |
| `{stem}_trades.csv` | always | Per good OHLC + `volume` (units bought as the sought good) |
| `{stem}_firms.csv` | flagged firms | Per flagged firm: `profit`, `sell_success`, then per-good qty/targets/quotes/flows |
| `{stem}_pops.csv` | flagged pops | Per flagged pop: sat/SOL/shop/income/wealth, then per-good qty/shop/save/consumed |

TTY: pages replace the screen. Piped stdout prints home, then each command's log.
