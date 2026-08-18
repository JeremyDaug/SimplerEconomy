# Review log

Working backlog of open code-review findings. Not everything here blocks
progress; pick up items when convenient.

## Maintenance (for every code review)

When reviewing commits, a branch, a PR, or local changes:

1. **Add** new open issues worth tracking (bug / suggestion / nit).
2. **Remove** items that are fixed or no longer accurate (do not leave stale open bugs).
3. Optionally note the review range and date in a short “Last updated” line below.
4. Keep entries scannable: file path, what, fix idea.
5. Record **dev responses** that close or reclassify items (fixed / accepted invariant /
   deferred design), then move them out of the open backlog.

---

**Last updated:** 2026-08-18 — local review of pop record-keeping / planning / turn wiring.  
**Open items:** 1 bug, 5 suggestions, 2 nits.

Full write-up: `/tmp/grok-1000/grok-review-72fee57c.md`  
Summary: `/tmp/grok-1000/grok-review-summary-72fee57c.md`

---

## Open bugs

- `src/game/pop.rs:1433` — `savings_growth_buffer` already grows the save pile, then next-morning `update_desires` multiplies the whole `shop_target` (consume need + that grown save) by the same `growth_f` and does not scale `save_target`. Growing pop save slice is `growth_f^2`; `shop = consume need + save` does not survive day-start. Fix: one owner for the scale. Test `record_keeping` then `update_desires`.

## Open suggestions

- `src/game/pop.rs:1503` — Liquid leftover parks on the most salable tradeable already in `goods`, including non-desire goods. `create_orders` only walks desire targets, so that buy may never be requested. Fix: emit surplus-funded buys, or park only on goods orders already know.
- `src/game/pop.rs:1140` — `saved_amv` uses `quantity - reserved`. Luxury consume can drive `reserved` negative, so leftover AMV is overstated. Fix: floor reserved at 0, or snapshot leftover `quantity`.
- `src/game/pop.rs:1330` — `living_need_amv` takes cheapest target with no Untradeable skip and no `cap`. `specific_buffer_weights` skips untradeable but still sums every substitute's full-desire AMV. Fix: cheapest feasible tradeable cover; weight the fear pile from that same basket.
- `src/game/pop.rs:1275` — `savings_growth_buffer` uses current `count` minus `previous_growth`. After migration is live, that is post-migration size. Dormant while migration leaves are `todo!()`. Fix: use post-growth pre-migration count.
- `src/playstate.rs:205` — `pop_market_histories` clones the price map once per member pop, twice per turn. Fix: one history per market (or `Arc`) and map pop id to market id.

## Open nits

- `src/game/pop.rs:1377` — `good_is_tradeable` reimplements `Good::is_buyable()`. Call `find_good(...).is_buyable()`.
- `src/game/desire.rs:467` — `debug_assert!(eff > 0.0)` on `DesireTarget::new` has no rustdoc. STYLE.md wants the constraint documented on the constructor.
