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
2. `Firm::pay_wage_shares` — **not** `LaborSettlement::settle`. See `labor.md`.
3. `Market::run_market_day`. Time is untradeable transport. This CLI does not
   settle labor, so firm recipes spend Time only if it is already on the firm.
4. `Firm::run_production` on **already-loaded** factuals. Do not reload
   `processes.toml`. A line starting from 0 snaps to 1.
5. Pop `consume`, `update_sentiments`, `record_keeping`, then cap coin
   `save_target` / `shop_target` at 1 unit.
6. Firm `record_keeping` (`plan`) from the closing `MarketHistory`. Do not call
   pop `record_keeping` again.
7. Pop/firm `decay_goods`.

Prints a `MarketDayReport` plus wages, production, plans, post-consume pop
stats, and the AMV trail. `day N` adds a one-line digest (includes mean firm
confidence) and the last day's full report. Books reload from current stock
after the loop.

Working pops emit **requests** only, with desires set outright (not from
demographics). No merchants. Firm default buy priority is `FIRM_PRODUCER`.
All five firms are owned by the Lord pop. Bounds are hand-set on the roster,
not computed. No cargo goods.

## CSV

One-row-per-day under `data/logs/` (gitignored). Default stem `prices`.
`csv` shows paths; `csv <name>` changes stem; `csv reset` wipes headers.
**Header mismatch** (old layout) errors until reset or a new stem.

| File | Contents |
|------|----------|
| `{stem}_market.csv` | Per good: `amv`, `salability`, `average_price` |
| `{stem}_firms.csv` | Per firm: `confidence`, `profit`, `sell_success`, then per-good qty/targets/quotes/flows |
| `{stem}_trades.csv` | Per good OHLC + `volume` (units bought as the sought good) |

TTY: pages replace the screen. Piped stdout prints home, then each command's log.
