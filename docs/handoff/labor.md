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

## Settle (lib, not wired)

[`LaborSettlement::settle`] (thin [`Firm::settle_labor_contracts`] wrapper)
pays each basket (scaling first, then flat; whole units), moves Time pop ->
firm, and reserves it for production.

- One pop, one employer. A firm may have several worker pops.
- Hours are Time units. Scaling pay is per time unit; **flat** is a lump paid
  last.
- Short till: never spend the **stock fence** (`stock_target` /
  `reserve_target`). Wages may raid **growth target**. Profit share of
  yesterday `sold_amv - sold_cost_amv` is paid last and cut first.
- Partial pay withholds Time linearly in AMV paid / AMV promised.
- `growth_target` is read at settle; `Firm::plan` does not write it yet.

`work_time_fraction` 0.5 is a **cap** on claimed Time (stand-in until culture /
class / religion / law). Prefer caps over a fixed daily grant.

**Code:** `src/game/workforce.rs`; tunables `factuals.config.labor`.

## Tester stand-in

Tester `day` still calls [`Firm::pay_wage_shares`]: living owners 30%, workers
30%, both ceil, owners first. Missing owners do not drain the till. Mine and
well have Time as an input, so they keep the 40% remainder (a producer with no
process inputs would pay the whole till). No wage bargaining. Do not swap the
tester to `settle` unless that is the task. PlayState labor fire is still a stub.

Stale (notify only): `pay_wage_shares` rustdoc still links `labor_constants`
(live reads `factuals.config`).

## Later (do not start)

Skill copy / experience, make-change on wages, pop payment preferences,
work-hours cap from demographics/law, `growth_target` from `plan`.
