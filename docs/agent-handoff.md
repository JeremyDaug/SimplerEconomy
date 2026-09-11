# Agent handoff — EconCiv rework

**Branch:** `EconCiv-Rework-Branch`  
**Updated:** 2026-09-10

**Router, not a dump.** Read **Status** + **Routing**. Open **one** topic file
and the listed code. Session order and "do not open" list: `AGENTS.md`.

This file is **what is landed** and **where to look**. Topic files are
invariants and traps, not a substitute for the code.

---

## Status

- Pop economic day is closed through record keeping.
- Pop `create_orders` posts ceil'd shop-plan requests, freezes tender cover,
  then floor'd offers of leftover surplus. Offers/requests may name a
  `counter_offer` good (no AMV or amount). Shop need spends on-hand then
  spreads leftover sat across buyable substitutes. Morning `create_orders`
  posts a higher tier only when the wallet covers the lower one. Save AMV
  scales with durability (decay 1.0 => no save). Consume always eats
  common on-hand; luxury is skipped unless basic is complete.
  `run_market_day` parks hopeless front-group buys then keeps matching
  later bands, then waves `next_shopping_trip`
  until trips emit nothing, then tombstones. Listed offer units and
  `reserved` are not tenderable. World goods use per-good `decay_rate` in
  `goods.toml`.
  After decay, salability is capped at `1 - decayed/volume` (eaten stock
  is volume, not rot). Each market day AMV is rescaled so one unit of
  each tradeable good averages 10.0 (after salability, then the close).
  Firm AMV quotes scale with it; recorded trail samples do not.
  After the market day each pop records a buy stop (`market` / `money` /
  `transport`) if shop shortfalls remain.
  Pop keep: incoming bag uses consume / save / extra-desired / unused
  (0 / 25 / 50 / 100 salability penalty, best category lifts the bag).
  Outgoing units peel extra → save → consume at those same factors. The
  0.50 floor always applies (no floor-drop).
- Live intramarket loop: `Market::run_market_day`. PlayState intramarket and
  production phases are stubs.
- `Firm::plan` rewrites production and property targets (realized profit, sell
  success). Pace is `planning_lerp_rate`. Own quote, not lerp-to-market.
- Time is good id 0 (untradeable, transport 1.0, bulk 0). Pops get 64 * household labor
  at `Pop::start_day`. Live intramarket friction is 1 (`TRANSACTION_COST + bulk`).
  Goods have per-unit mass/volume in `goods.toml`.
- Labor **operates**. Tester `day` calls [`Market::settle_labor`] then
  [`Market::budget_labor`]. Time AMV is stamped from contracts (hours-weighted
  wage AMV), not goods matching. Firms do **not** rewrite wages from Time AMV
  yet (pops cannot move or resize). PlayState labor fire is still a stub.
- World goods, processes, and config load from `data/world/`. Processes are
  1 Time → 15 of each good (Time is process 28).
- Tester CLIs are **paused** unless asked. `market_tester` living roster loads from `data/init/`
  (one household pop and one remainder-owner firm per world good). `pop_tester` is the same pops with no firms; each morning the matching init firm's process outputs (`amount * target`) are a stock cap (add the shortfall only). Opening AMV 10.0 / salability 0.3 on every good.
  Grouped consume desires (basic/common/luxury) are 1 unit per member
  (5 units), duplicated onto every pop. **No** opening 1-of-each kit
  (init starter empty; `DAILY_ENDOWMENT` 0). Each morning: `start_day` Time, then specialty
  grant is 0 (`DAILY_OUTPUT` in `roster.rs`; `pop.id % n_goods`; pop 28 is Time
  and is the untradeable control). Each firm is that pop's remainder
  owner-operator: 10 Time, no wage basket, 1 Time → 15 line (150 output).
  Opening stock is process outputs times line target (yesterday succeeded;
  10 x 15 = 150). Posted firm sells cap at max market salability times
  daily output; remainder leftover is extra above that. Remainder owners
  cover an AMV shortfall on a loss. Coin is `gold_token`; iron ore is `iron`.
  `keep_alive on` is an emergency firm subsidy (1-iteration floor +
  coin/inputs); default off.
  Desire amounts do not rise with success.
  Luxury shop_target adds an extra level and leftover liquid above save.
  AMV moves from meetings only (accept / reject / no-proposal). Reject
  tender down-push is tender AMV / sought AMV, not raw units. Leftover
  book blend is 0. Volume-scaled leftover collapsed AMV to the bounce
  floor; do not turn it back on unless asked.
  CSV is market + trades always; pops/firms only when flagged (`csv on`).
  **Checkpoint:** zero-endowment intramarket barter holds a 50-day run
  with clustered AMVs. Do not retune leftover AMV or re-add the 1-of-each
  grant unless asked.

**Vault conflict:** `Turns.md` puts firm planning before consume. Live order is
produce, then consume, then plan. Call it out; do not silently "fix" either side.

**Live day (tester / intended lib order):**
`start_day` -> `Market::settle_labor` -> `run_market_day` -> `run_production`
(outputs go to `held`) -> pop consume / sentiments / decay -> firm decay
(`quantity` rots, then `held` joins `quantity`) -> salability rot cap ->
pop `record_keeping` -> firm `record_keeping` (`plan`) ->
`Market::budget_labor`.
With goods decaying 1.0, planning **after** decay is required; otherwise
shop_targets fence stock that will not exist next morning and nobody offers.

---

## Where truth lives

| Question | Look in |
|----------|---------|
| What is this called? | `docs/design-vocabulary.md` |
| Is it landed, stubbed, or later? | Matching topic file, **Landed vs stub** |
| How does it actually work? | The listed code |
| Turn-pipeline checklist | `TODO.md` (only if wiring a phase) |
| Open review debt | `reviewlog.md` |
| Should behavior change? | Matching EconCiv vault note, then say so |

Do not implement from a topic file. Do not open the vault for a bugfix, wire-up,
or refactor that follows existing behavior.

---

## Routing

Match the user's task. Stay in those files.

| Task | Topic file | Code |
|------|------------|------|
| Labor, wages, Time, workforce | [`labor.md`](handoff/labor.md) | `workforce.rs`, `Firm::settle_labor_contracts`, `Firm::pay_wage_shares` |
| Firm plan, production, `FirmPRow`, records | [`firms.md`](handoff/firms.md) | `firm.rs`, `firm/plan.rs` |
| Firm `create_orders` | [`firms.md`](handoff/firms.md) | `firm/orders.rs` |
| Market day, matching, AMV, salability, order priority | [`market.md`](handoff/market.md) | `market.rs`, `marketorder.rs` |
| Deals, tenders, keep, transport, whole units | [`deals.md`](handoff/deals.md) | `deal.rs`, `pop/deal.rs` |
| Pop consume, shop/save, desires, sentiment | [`pops.md`](handoff/pops.md) | `pop.rs`, `pop/orders.rs`, `desire.rs`, `pop_property.rs` |
| Tester CLI, `day`, CSV | [`tester.md`](handoff/tester.md) | `examples/market_tester/`, `examples/pop_tester/` |
| World data, config, factuals | [`world.md`](handoff/world.md) | `factuals.rs`, `config.rs`, `data/world/` |
| Init pops/firms (scenario) | [`world.md`](handoff/world.md) | `init.rs`, `data/init/` |
| PlayState / turn wiring | [`turns.md`](handoff/turns.md) | `playstate.rs` |
| Order-priority numbers (deferred ranking too) | `docs/proposals/market-order-priority.md` | `config::market_priority` |

If nothing matches, stay in the files the user named. If the task is unclear,
ask one question instead of opening more topics.

---

## Do not start unless asked

Nearby leftovers are traps, not implied scope: multimatch; `sell` rewrite /
haggling / make-change; PlayState intramarket or production wire (unless that
**is** the task); tester pages / extra CSV; species-culture-religion TOML;
init/save data; class demographics; capital amortization; AMV as a matching
weight; intra-day luxury loop; leftover-book AMV (off).

If the user did not name a task, **ask**. Do not pick a next system on your own.
If they ask "what's next": wire PlayState `phase_intra_market_day` to
`run_market_day`. Hiring/creation is skipped on purpose.

---

## Cross-cutting traps

These are the ones agents keep "fixing." Style, comments, and names are in
`AGENTS.md` / `STYLE.md` / vocabulary — do not restate them here.

- Household rates are **not** on each pop. `Factuals::get_demographic_rates`.
  Do not re-add `DemoRow.rates`.
- `create_orders` (pop or firm) is mechanical. Do not replan there.
- Do not add AMV into `match_orders`.
- Target efficiency is always **positive** (`debug_assert` only).
- Whole-unit **goods** on orders and deal maps. AMV and the wagon bill stay
  fractional.
- `debug_assert` for "must never happen." No hot-path `is_finite` fallbacks.
- **Write / set** on create; **update / edit** in the books; **stamp** only a
  completed deal. Never bare "priority", "ratio", or "knob" (knob is player-UI).

---

## How to refresh

Update the **matching topic file** and **Status** here. Do not paste essays
into this router. Do not copy vocabulary or `TODO.md` into topic files.
