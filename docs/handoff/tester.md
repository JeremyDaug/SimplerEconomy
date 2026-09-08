# Market tester CLI

Read this only for the tester, `day`, CSV logs, or the living roster.
**Paused.** Do not add pages, commands, or extra CSV series unless asked. Do
not invent a second shopping model.

```bash
cargo run --example market_tester
```

**Code / source of truth:** `examples/market_tester/` (`main`, `roster`,
`format`, `parse`, `csv`). Recipes: `data/world/processes.toml`. Starting
AMV/sal, desires, ownership, and bounds: `roster.rs` — do not copy them here.

Home is a short summary. Pages: `stock` / `orders` / `processes` / `amv` /
`day`. `home` / `cls` back. `shop` reloads books from `create_orders`. `match`
is **read-only** (does not move stock). `main.rs` is still the Bevy hex stub.

## `day` / `day N`

1. `Pop::start_day` (Time grant). Zero `income_amv`, reservations, firm
   `clear_day_flows`.
2. `LaborSettlement::settle` (via `Firm::settle_labor_contracts`). Hours, wage
   basket, and one-employer roster: `labor.md`. Time moves pop -> firm here.
3. `Market::run_market_day`. Time is untradeable transport.
4. `Firm::run_production` on **already-loaded** factuals. Do not reload
   `processes.toml`. A line starting from 0 snaps to 1.
5. Pop `consume`, `update_sentiments`, `record_keeping`. Coin save/shop
   come from that rewrite (no tester cap).
6. Firm `record_keeping` (`plan`) from the closing `MarketHistory`. Do not call
   pop `record_keeping` again.
7. `Firm::budget_labor` (hours and wage amounts; `budget_interval`, default 1).
8. Pop/firm `decay_goods`.

Prints a `MarketDayReport` plus wages, production, plans, post-consume pop
stats, and the AMV trail. `day N` adds a one-line digest (includes mean firm
confidence) and the last day's full report. Books reload from current stock
after the loop.

Working pops emit **requests** only, with desires set outright (not from
demographics). Gold is a common consume desire; jewelry is luxury consume
(world goods: jewelry decays 2%/day). No merchants. Firm default buy
priority is `FIRM_PRODUCER`. All five firms are owned by the Lord pop as
**remainder** (owner-operator). Bounds are hand-set on the roster, not
computed. No cargo goods. Leftover AMV is lib (`market.md`), not tester.

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
