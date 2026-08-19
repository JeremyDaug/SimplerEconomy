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

**Last updated:** 2026-08-18 — closed the 2026-08-18 record-keeping review items.  
**Open items:** 0 bugs, 0 suggestions, 0 nits.

---

## Open bugs

(none)

## Open suggestions

(none)

## Open nits

(none)

## Closed / deferred (2026-08-18)

- **Fixed** `pop.rs` growing-pop double scale — record keeping owns next-day shop/save
  (consume-need scaled by post-growth pre-migration `growth_f`; save buffer inflates
  only). `update_desires` no longer multiplies property targets. Morning demo-definition
  demand shifts roll in at the next rewrite.
- **Fixed** `create_orders` — planned shop shortfalls on non-desire goods (parked
  savings) are requested after the desire shop pass and before opportunistic extra buys.
- **Fixed** `satisfy_one_desire` / `PopPRow::saved` — reserved floors at 0; leftover
  AMV is leftover quantity.
- **Fixed** `living_need_amv` / `specific_buffer_weights` — shared cheapest tradeable
  cover (skip untradeable, respect `cap`).
- **Fixed** growth buffer uses `count - net_migration` as post-growth size.
- **Fixed** `playstate.rs` — `MarketLookups` is one history per market plus pop-to-market.
- **Fixed** `good_is_tradeable` now `Good::is_buyable()`; `DesireTarget::new` documents
  positive efficiency.
- **Deferred** luxury consume leveling — extra luxury passes can empty the leftover
  pile; cap/pace later. See `TODO.md`.
- **Deferred** `Pop.market_id` — membership currently lives on `Market.pops` and
  `MarketLookups.pop_to_market`. Revisit when migration leaves write.
