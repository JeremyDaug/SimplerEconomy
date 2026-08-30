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

**Last updated:** 2026-08-30 — implemented mixed-good / self-trade `None`,
buyer unit-AMV cap, firm sell/offer earmark on tenders, and pop tender
helper comment style. Leftover salability TODO and tester roster-count nit
still open.
**Open items:** 0 bugs, 1 suggestion, 1 nit.

---

## Open bugs

(none)

## Open suggestions

- `src/game/deal.rs:139` — leftover `TODO, consider adding salability here`
  on the full-price load, immediately before the received-side haircut that
  already applies salability. Reads as design chatter and makes it look like
  given goods might also be haircut (would contradict keep-ratio). Drop it,
  or replace with a one-liner that given goods stay at full AMV on purpose.

## Open nits

- `examples/market_tester.rs:4` — module docs say "3 pops, 5 producer firms".
  The roster builds six firms (farm, bakery, mine, mint, jeweler, well).
  Say 6 firms (or 6 producers).
