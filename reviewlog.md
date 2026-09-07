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

**Last updated:** 2026-09-06 — AMV bounds are planning guidestones (no
trade skip/clamp/void), tester Lord pop owns all firms, sell-plan goods
can tender their exchange slice again.
**Open items:** 0 bugs, 1 suggestion, 0 nits.

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

(none)
