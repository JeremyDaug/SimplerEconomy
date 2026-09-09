# Market tester CLI

Read this only for the tester, `day`, CSV logs, or the living roster.
**Paused.** Do not add pages, commands, or extra CSV series unless asked. Do
not invent a second shopping model. `keep_alive on|off` is the emergency
firm subsidy toggle (`firm.keep_alive`, default off).

```bash
cargo run --example market_tester
```

**Code / source of truth:** `examples/market_tester/` (`main`, `roster`,
`format`, `parse`, `csv`). Recipes: `data/world/processes.toml`. Starting
AMV/sal, desires, ownership, and bounds: `roster.rs` — do not copy them here.
Living roster: one pop per world good, 1 household each, empty firm list.
Grouped consume desires (basic food/hydration/heating/housing, common
utility/improved food/materials/health, luxury shiny tokens/libations) are
duplicated onto every pop at 1 unit per member (5 units). **No** 1-of-each
starter kit (`DAILY_ENDOWMENT` 0). Each morning: `start_day` Time
(`TIME_PER_LABOR` 64 * household labor), then specialty only (`DAILY_OUTPUT`
in `roster.rs`; `pop.id % n_goods`; pop 28 produces Time and cannot sell
it). Opening AMV is 10.0 and salability 0.3 on every good (no money good,
no price spread). Coin is `gold_token`; iron ore is `iron`.

Home is a short summary. Pages: `stock` / `orders` / `processes` / `amv` /
`day`. `home` / `cls` back. `shop` reloads books from `create_orders`. `match`
is **read-only** (does not move stock). `main.rs` is still the Bevy hex stub.
Firm helpers stay in `roster.rs` but are not on the living roster.

## `day` / `day N`

1. `Pop::start_day` (Time grant). Tester morning specialty only
   (`DAILY_ENDOWMENT` 0). Zero `income_amv`, reservations, firm
   `clear_day_flows`.
2. `Market::settle_labor` (pays contracts, stamps Time AMV). Hours, wage
   basket, and one-employer roster: `labor.md`. Time moves pop -> firm here.
3. `Market::run_market_day`. Time is untradeable transport; Time AMV is not
   leftover-book drift. Leftover books do not move AMV. Each market day AMV
   is rescaled to mean 10.0 after salability, then the close is recorded.
4. `Firm::run_production` on **already-loaded** factuals. Do not reload
   `processes.toml`. A line starting from 0 snaps to 1.
5. Pop `consume`, `update_sentiments` (market-day history), then
   `decay_goods`. Firm `decay_goods` next. Aggregate `(decayed, volume)`
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
stats, and the AMV trail. `day N` adds a one-line digest (includes mean firm
confidence) and the last day's full report. Books reload from current stock
after the loop.

Working pops emit **requests and leftover offers**, with desires set
outright (not from demographics). No merchants. Firm helpers remain in
`roster.rs` but
`build_world` returns an empty firm list. No cargo goods. Leftover AMV is
lib (`market.md`), not tester.

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
| `{stem}_firms.csv` | flagged firms | Per flagged firm: `confidence`, `profit`, `sell_success`, then per-good qty/targets/quotes/flows |
| `{stem}_pops.csv` | flagged pops | Per flagged pop: sat/SOL/shop/income/wealth, then per-good qty/shop/save/consumed |

TTY: pages replace the screen. Piped stdout prints home, then each command's log.
