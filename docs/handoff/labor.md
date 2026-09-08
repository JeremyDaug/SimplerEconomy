# Labor and Time

Read this only for wages, Time, workforce, or employment. Roster numbers and
the tester calendar live in `docs/handoff/tester.md`.

Employment is a **`Workforce` roster**, not a goods-book labor market. Do not
put labor on `MarketOrder`s. No labor-time good. Hiring and contract creation
are skipped on purpose.

## Time (good id 0)

Id 0 on purpose (exception to "0 means none" for goods). Transport 1.0.
**Untradeable.** Decays 100%/day into nothing. Pops receive `TIME_PER_LABOR`
(48) * household labor at `Pop::start_day` (adult 1.0, elder 0.7, child 0.3).
Every world process spends a little time as a destroyed input. Pops cannot buy
extra person-days.

**Code:** `good::TIME`, `data/world/goods.toml`, `Pop::start_day`

## Settle

[`LaborSettlement::settle`] (thin [`Firm::settle_labor_contracts`] wrapper)
pays each basket (scaling first, then flat; whole units), moves Time pop ->
firm, and reserves it for production. Tester `day` calls it. PlayState labor
fire is still a stub.

- One pop, one employer. A firm may have several worker pops.
- Hours are Time units. Scaling pay is per time unit; **flat** is a lump paid
  last.
- Short till: never spend the **stock fence** (`stock_target` /
  `reserve_target`). Wages may raid **growth target**. Worker profit share
  of yesterday `sold_amv - sold_cost_amv` is paid after wages. Owner is
  either **remainder** (leftover till, owner-operator) or a limited
  **profit share** (dividend / partial owner). Remainder is cut first after
  those fences; limited share is capped at profit AMV.
- Partial pay withholds Time linearly in AMV paid / AMV promised.
- `growth_target` is read at settle; `Firm::plan` does not write it yet.

`work_time_fraction` 0.5 is a **cap** on claimed Time (stand-in until culture /
class / religion / law). Prefer caps over a fixed daily grant.

## Labor budget

[`Firm::budget_labor`] rewrites hours and the wage **basket** after `plan`.
Does not hire, fire, or move pops. Hours **snap** to recipe Time plus today's
`transport_spent + 1` (extra `transaction_cost` if purchase targets are much
above today's buys). Hours are **not** cut to fit the till. Wages: angry/fearful
pops get a 1-unit flat (in-kind they still want, else salable). Calm +
unprofitable trims flats. Calm + profit in 1.0..=1.15 holds. Calm + richer
profit adds a product flat only if they still want that kind. Fat flats fold
into hourly when `flat / hours >= 1`. Hourly rates never below 1.
`labor.budget_interval` is **1 (every day)** in world config; **0 skips**.
Tester calls it every day. PlayState does not.

**Code:** `src/game/workforce.rs`; tunables `factuals.config.labor`.

## Tester

Tester `day` calls [`LaborSettlement::settle`] and [`Firm::budget_labor`].
Roster hours are day-1 recipe
Time (farm 15, bakery 28, mine 32, jeweler 5, well 30); buying firms add one
`transaction_cost` of Time so a restock meeting does not starve production.
Wage basket is 1 coin per Time unit. Lord is **remainder** owner-operator.
Sell piles and a coin wage-float are retained as `growth_target` (plan does
not write it yet). One pop, one employer: farmers-farm, laborers-mine,
townsfolk-bakery, jewelers-jeweler, wellhands-well. `pay_wage_shares` is
unused here. PlayState labor fire is still a stub.

Stale (notify only): `pay_wage_shares` rustdoc still links `labor_constants`
(live reads `factuals.config`). `LaborSettlement::settle` rustdoc still says
it is not wired into the tester.

## Later (do not start)

Skill copy / experience, make-change on wages, pop payment preferences,
work-hours cap from demographics/law, `growth_target` from `plan`, hiring.
