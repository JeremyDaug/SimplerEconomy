# Labor and Time

Read this only for wages, Time, workforce, or employment. Roster numbers and
the tester calendar live in `docs/handoff/tester.md`.

Employment is a **`Workforce` roster**, not a goods-book labor market. Do not
put labor on `MarketOrder`s. No labor-time good. Hiring and contract creation
are skipped on purpose.

## Time (good id 0)

Id 0 on purpose (exception to "0 means none" for goods). Transport 1.0.
**Untradeable.** Not on `MarketOrder`s. Time AMV is stamped from labor
contracts (hours-weighted wage AMV), not leftover book pressure. Decays
100%/day into nothing. Pops receive `TIME_PER_LABOR`
(48) * household labor at `Pop::start_day` (adult 1.0, elder 0.7, child 0.3).
Every world process spends a little time as a destroyed input. Pops cannot buy
extra person-days.

**Code:** `good::TIME`, `data/world/goods.toml`, `Pop::start_day`

## Settle

[`LaborSettlement::settle`] (thin [`Firm::settle_labor_contracts`] wrapper)
pays each basket (scaling first, then flat; whole units), moves Time pop ->
firm, and reserves it for production. Tester `day` calls [`Market::settle_labor`].
PlayState labor fire is still a stub.

- One pop, one employer. A firm may have several worker pops.
- Hours are Time units. Scaling pay is per time unit; **flat** is a lump paid
  last.
- Short till: never spend the **stock fence** (`stock_target` /
  `reserve_target`). Wages may raid **growth target**. Worker profit share
  of yesterday `sold_amv - sold_cost_amv` is paid after wages. Owner is
  either **remainder** (leftover till, owner-operator) or a limited
  **profit share** (dividend / partial owner). Remainder is cut first after
  those fences and **posted sell** (`min(sell_target, max market
  salability * daily output)` for goods the firm makes); leftover is extra
  above that, paid high salability first. Limited share is capped at profit AMV.
- Remainder owner on a loss (yesterday profit AMV <= 0) covers the AMV
  shortfall vs needs (recipe inputs, wage basket, stock fence) **before**
  wages, from unreserved stock: missing inputs, missing wage goods,
  production outputs, then exchange. Contributed inputs/outputs are fenced
  so leftover remainder cannot take them back the same morning. Limited
  owners do not cover. Vault `Firms.md` does not spell this out; Owners
  rustdoc already says they are accountable for losses.
- Partial pay withholds Time linearly in AMV paid / AMV promised.
- `growth_target` is read at settle; `Firm::plan` does not write it yet.

`work_time_fraction` 0.5 is a **cap** on claimed Time (stand-in until culture /
class / religion / law). Prefer caps over a fixed daily grant.

## Labor budget

[`Firm::budget_labor`] rewrites hours and the wage **basket** after `plan`.
Tester calls [`Market::budget_labor`], which stamps Time AMV then runs each
member firm. Does not hire, fire, or move pops. Hours **snap** to recipe Time
plus today's `transport_spent + 1` (extra `transaction_cost` if purchase
targets are much above today's buys). Hours are **not** cut to fit the till.
Wages: angry/fearful pops get a 1-unit flat (in-kind they still want, else
salable). Calm + unprofitable trims flats. Calm + profit in 1.0..=1.15 holds.
Calm + richer profit adds a product flat only if they still want that kind.
Fat flats fold into hourly when `flat / hours >= 1`. Hourly rates never below 1.
Firms do **not** rewrite wages from Time AMV yet.
`labor.budget_interval` is **1 (every day)** in world config; **0 skips**.
PlayState does not.

**Code:** `src/game/workforce.rs`, `Market::settle_labor` /
`Market::budget_labor`; tunables `factuals.config.labor`.

## Tester

Tester `day` calls [`Market::settle_labor`] and [`Market::budget_labor`].
Living roster: one remainder owner-operator per good, `FIRM_HOURS` 10, no
wage basket, no worker profit share. 1 Time → 15 output so 10 Time is 150
units. Pop morning specialty grant is 0. `pay_wage_shares` is unused here.
PlayState labor fire is still a stub.

Stale (notify only): `pay_wage_shares` rustdoc still links `labor_constants`
(live reads `factuals.config`).

## Later (do not start)

Skill copy / experience, make-change on wages, pop payment preferences,
work-hours cap from demographics/law, `growth_target` from `plan`, hiring,
wage vs Time AMV (until pops can move or resize).
