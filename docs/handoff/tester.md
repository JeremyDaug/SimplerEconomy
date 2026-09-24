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
decay / pop `record_keeping`. The top-up is the produce stand-in (no firms).
`day N` stops if any good AMV is negative and prints the day and those
goods. No firm production, plan, or `keep_alive`.
Caps follow init firm `target` × process output (Time output caps pops 28 and 56).

## Market tester

`keep_alive on|off` is the emergency firm subsidy toggle (`firm.keep_alive`,
default off).
`solo [id|on|off]` reboots to one remainder pop/firm (default id 1) so
internal plan can be watched without the village market. `solo off` is the
eight-household village. `cargo run --example market_tester -- solo`
starts there. Remainder owners eat from the shop shelf (not a wage).
Household needs refresh before production from in-shop desire fill
(a well's food dinner is grain, not the bread shop-plan split). Goods
the shop makes are not posted as leftover-buy demand. Uncovered owner
need raises quota. Solo water should hold basic 3.

**Code / source of truth:** `examples/market_tester/` (`main`, `roster`,
`format`, `parse`, `csv`). Recipes: `data/world/processes.toml` (1 Time → 15
of the output, one process per good; Time is process 28). Starting pops and
firms: `data/init/` (`InitData`). Opening AMV/sal: `roster.rs`.
Opening AMV 100.0 / salability 0.1 (`roster.rs`).
Living roster: eight remainder owner-operators (grain, water, bread, gold,
wood, cabins; two grain, two water). 1 household each.
On load, unused world goods (and processes that mention them) are dropped;
CLI and CSV walk the remaining ids (gaps are kept; do not treat count as
an id space). Village catalog: time, grain, water, bread, gold, gold_token,
jewelry, wood, cabins.
Pop labels are `pop{id}-{specialty}` (`pop1-grain`, `pop3-bread`); `pop1` still parses.
Firm labels match (`firm1-grain`, `firm3-bread`, `firm1`); `firm 1` still parses.
Firm 5/6 display as `gold_token` / `jewelry` (catalog id); they are the second
grain shop and second well.
`data/logs/` is gitignored. Keep at most three captures locally: a reference
to improve from, the current run, and one spare. Delete the rest.
Each firm is the matching pop's remainder owner-operator, one specialty line
plus three subsistence lines, `target` 8 on grain/water/wood, 5 on bread/gold,
2 on cabins, no wage basket. Opening stock is three decay-adjusted
days of process output (`OPENING_COVER_DAYS`) with `stock_target` matching that
buffer and `sell_target` equal to one day's output; posted sell is capped
by max market salability * daily output; Time starts empty. Pop morning specialty
grant is 0.
Village consume desires (basic food/hydration/heating, common housing plus
improved food, luxury gold) are duplicated onto every pop: 1 unit per member
(5 units) except housing (1 cabin per household). **No** opening
1-of-each kit (init starter empty; `DAILY_ENDOWMENT` 0). Boot `record_keeping`
writes shop targets. Each morning: `start_day` Time
(`TIME_PER_LABOR` 64 * household labor). Specialty output comes from the
firm (hours = target * Time input, so 10 Time, cabins 1; `pop.id % n_goods`; pops 28 and 56 make Time and
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
3. `Firm::refresh_household_needs`, then `Firm::run_production` on
   **already-loaded** factuals. Do not reload `processes.toml`. A line
   starting from 0 snaps to 1. Outputs go to `held`; later lines may spend
   `held` after on-hand stock, excluding owner dinner.
4. `Market::run_market_day` (may sell `held`). Time is untradeable
   transport; Time AMV is not leftover-book drift. Leftover books do not
   move AMV. Remainder pantry goods are not posted or stamped as leftover
   buys. Each market day AMV is rescaled to mean 100.0 after salability
   (firm quotes scale with it; the AMV trail is not rewritten), then the
   close is recorded.
5. Remainder `Pop::consume_from_firm` (shop shelf), else `Pop::consume`;
   `update_sentiments` (market-day history), then `decay_goods`. Firm
   `decay_goods` next (`quantity` rots, then `held` joins `quantity`).
   Aggregate `(decayed, volume)` and `Market::cap_salability_from_decay`.
   Consumed is volume, not rot.
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
outright (not from demographics). No merchants. `build_world` returns two remainder-owner firms per good (one per pop);
line `target` 10 except cabins at 1 (hours follow Time input) and opening stock is three
decay-adjusted days of output. No cargo goods. Leftover AMV is lib (`market.md`), not tester.

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
