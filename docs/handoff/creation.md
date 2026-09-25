# Firm founding

Read this only to **found** a new firm (**split** a divided shop, or later
savings founding). Order emit is `create_orders` (`firms.md`). Names:
`docs/design-vocabulary.md` (found, split, savings founding, disorganized).
Vault `Firms.md` is company tree and player/institution founding — call that
out; do not implement Independence / Management for this.

## Pickup (2026-09-23)

Remainder village now feeds itself and does some specialty trade. The missing
fact is how a **divided** mixed shop becomes two shops, with the departing
piece allowed one line change (for example subsistence grain → extract grain).

Two methods (user):

1. **Split.** Only **sufficiently divided** firms (prime example:
   **disorganized**). A multi-pop subsistence shop peels off some of its
   pops. Existing production lines **scale down** with that fraction of
   people. The new firm may **add and/or remove one line** as part of the
   same act (example: 10 pops, split off 1, take ~1/10 of each garden line,
   add non-subsistence grain and/or drop subsistence grain).
2. **Savings founding.** Saved goods open a new small shop. Wants a money
   good. **Not the first slice.**

The living eight remainder shops are **one pop each**. They are **not**
divided enough to split. Do not peel a baker line off firm 3 and call it a
split. Do not add a subsistence-tag plan policy. Farm/water/forage tags
start init only.

**First slice.** Fixture one **disorganized** subsistence firm with several
pops (the user’s 10-and-1 picture; a smaller N is fine if the fraction is
obvious) and only garden lines. Split off **one** pop with the matching
fraction of each line’s quota and a matching fraction of stock. The child
may add one specialized line and/or remove one garden line in that same
act. Child is remainder for the departing pop. Parent keeps the other pops
and scaled lines. Lib tests are the proof. Do not add tester pages or CSV
series unless asked. Parent still gardens at the reduced scale. The child
has the swapped line. The departing pop is the child's remainder owner and
is off the parent workforce. Hours move with that pop. Owners eat from
their own bag, not the shop shelf.

**Pin before coding.** Current labor is **one pop, one employer**
(`labor.md`). Split **fits** that if the departing pop leaves the parent
workforce and becomes remainder owner of the child. Do not keep one pop on
both shops. Do not invent a wage paycheck. Current
`remainder_owner_firm` / pantry / `buy_with_firm` assume one liable shop
per pop — that stays true if each pop still has one shop. What is **not**
landed: a multi-pop subsistence firm (several pops on one workforce /
ownership). That fixture is part of this slice, not a separate hiring
system.

**Not this pickup:** hiring classes / owner-vs-worker as a general rewrite;
money as a standard; savings founding; adding lines to the eight 1-pop
remainder shops; splitting those 1-pop shops; tech or culture recipe gates;
pop growth/migration; PlayState intramarket/production; salability rewrite;
leftover-book AMV; DIY vs buy; `FirmOrganization` company rules;
subsistence-only floors.

**Vault conflict:** EconCiv `Firms.md` founds firms via player influence or
institutions, then company hierarchy. Live work is automatic **split** of a
divided shop. Do not mix those. Do not edit the vault unless asked.

## Landed vs stub

| Piece | Status |
|-------|--------|
| Remainder owner-operator village (8× one pop) | Landed. Shop is pantry and wallet. **Cannot split.** See `firms.md` |
| Line **abandon** (remove a line, firm stays) | Landed. Not a split. Split may remove one line on the **child** as part of founding |
| Init load of firms | Landed. One remainder owner per firm. No runtime founding |
| `Market.firms` membership | Landed as a set of ids. No helper to register a new firm mid-game |
| Multi-pop subsistence / disorganized firm | Fixture in `Firm::split` tests. Not a new org type. Not the eight 1-pop shops |
| Split / found | Landed. `Firm::split`, `Market::found_firm`. One departing pop, `1/n` of each line, whole-unit in-kind share, optional one-line add and/or remove on the child. 1-pop shop returns `SplitReject::NotDivided` |
| Savings founding | Parked (second slice; wants money) |
| Disorganized as a type | Parked as a full type. First slice may fixture the behavior without a new org enum |
| Company / `FirmOrganization` | Placeholder; unused |

## Invariants for the first split

- **Found** adds a firm actor and a market id. It is not `create_orders`.
- Only a **sufficiently divided** firm may split. Disorganized is the prime
  example. A 1-pop remainder shop may not.
- Pops move with the split. Lines **scale** with headcount (1 of 10 pops →
  ~1/10 of each parent line’s quota and a ~1/10 in-kind share of those
  goods). Then the child may **add one line and/or remove one line**.
- In-kind move is a transfer, not a market deal. Do not record it as
  leftover-buy.
- Departing pop is remainder owner of the child. Parent keeps the others.
  One pop, one employer.
- Load does not attach farm/water/forage. A child gardens only when the
  scaled parent lines include those processes.
- Tester CLIs stay paused except as needed to watch the split. No extra CSV
  series unless asked.

## Traps

- `create_orders` is mechanical emit. Never found a firm there.
- Do not implement split on the eight 1-pop village shops. They are not
  divided.
- Do not treat split as “peel the specialty line, same owner, two shops.”
  That fights one-employer labor and is not the design.
- Do not put subsistence attach back on load. A child that dropped
  subsistence grain must not gain it from init.
- Empty firms already exist after abandon-all-lines. Founding is the
  opposite direction; do not reuse abandon as spawn.
- Gold_token / jewelry are **unloaded** in the village catalog. Firm ids 5/6
  are the second grain shop and second well.
- Scaling quotas: whole-unit goods; a 1/10 of a tiny line may snap to 0 or
  1. Pin that in the helper, do not silently drop the garden.
- Complexity tax follows line count on each shop after the split. A child
  that keeps three gardens and adds extract is four-line again.

## Code

- Landed: `src/game/firm/split.rs` (`Firm::split`), `Market::found_firm`
- Touch: `src/game/init.rs` (fixture; load does not attach subsistence),
  `src/game/firm.rs` (lines, property, owners, workforce),
  `src/game/workforce.rs` (pop leaves parent, joins child),
  `src/game/market.rs` (register the new firm id), `examples/market_tester/`
- Read first: this file, `labor.md` (one pop, one employer), `firms.md`
  **Invariants**, `STYLE.md` before the first Rust edit
- Vocab: found, split, savings founding, disorganized. Grep; do not invent
  a third name
