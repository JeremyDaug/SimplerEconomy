# Pops

Read this for desire satisfaction, consume, and the first deal pass. The
current goal is a market pop tester: a simple market, pops, and trade.

## Landed vs stub

| Piece | Status |
|-------|--------|
| `Pop::satisfy` | Reserves free stock and records satisfaction. Starts at the first basic desire. Stops at the first target it cannot take in full. Returns a copy of that desire, or `None` |
| `Pop::satisfy_continue` | Walks forward from the bookmark `satisfy` wrote. No bookmark: same as `satisfy` |
| `Pop::satisfy_tier` | One tier, from its first desire. Does not move the bookmark |
| `Pop::consume` / `consume_tier` / `consume_one_desire` | Old all-at-once eater. Not called. `PlayState::phase_pop_consumption` is still `todo` |
| Savings between tiers, and between luxury iterations | Not written. Intended order is tier, then that tier's savings, then the next tier. Luxury is level, savings, next level |
| `Market::match_deals` | Pair on one good, buyer proposes a basket from the seller's book, seller accepts or rejects, accept finalizes and pays friction. Both sides reevaluate afterward |
| Class desires | Unimplemented |
| `Desire.decay` | Field only. Nothing multiplies satisfaction by it |

## Satisfy

Order is tier, then list index. The good inside a desire's bucket is
chosen at random. `ordered_targets` is not used.

Basic and common each get one level. Luxury repeats one level at a time.
The next luxury level starts only after every luxury desire has reached the
current one. An empty tier is finished.

Free stock is `quantity - reserved`. A take adds to `reserved` and to
`satisfaction`. `quantity` stays put. A partial take on the stopping target
is kept. Later targets in that bucket, and every later desire, are left alone.
A target whose cap is filled does not stop the desire; the next target is used.

The bookmark is tier, desire index, the chosen target's index in the bucket,
the satisfaction the desire already had when that target started, and the
level being filled. `satisfy_continue` only reserves the cap still open above
that recorded satisfaction, and only the gap up to the current level
(`iter_target * amount - satisfaction`). When the open desire reaches the
level, the walk moves on.

`satisfy` always starts over at the first basic desire and replaces the
bookmark. Calling it again on a half-filled cap can reserve that cap a second
time. After new stock arrives, call `satisfy_continue`.

## Consume

Basic and common run once each, in list order, and do not stop when a desire
is short. Luxury benches any desire that missed the current level and keeps
filling the ones that made it.

Each call sets its gap to a full `amount`, ignoring satisfaction already
recorded. It subtracts the take from `quantity` and `reserved`, moves consume
targets to `consumed` and use targets to `used`, and adds satisfaction.
Running it on goods `satisfy` already counted records the level twice.

## Traps

- `desires` must have three tiers, indexes 0, 1, and 2.
- The bookmark's target index is highest-efficiency-first. It stays valid only
  while that bucket is unchanged. `sort_by` is stable for equal efficiency.
- Target efficiency is positive (`DesireTarget::new` asserts it).
- `update_desires` appends missing demographic desires. It does not sort the
  tier or bake `priority` to the index. Satisfy walks the list as stored.
- `DemoDesire::create_desire` is the demo-to-pop path. It scales `amount` and
  additive effects. Birth, mortality, sentiment, and satisfaction arms stay
  at demo values.

## Match

Exchange, production, and consumption stay separate phases.

`Market::match_deals` collects sells, then picks a buyer at random from those
who still have a buy, and a seller at random from the valid matches for that
buy. They meet because one sought good is one offered good. The buyer is
shown the seller's whole book and either proposes a basket or abandons.
The seller accepts or rejects. Accept moves every good in the basket.
The basket is one map: positive units come to the buyer, negative units
leave. Freight is not a second list. Transport the buyer receives is in
that map, and after the goods move the buyer spends transport they then
hold until the bill is covered. If stock plus that purchase cannot cover
it, the buyer abandons before a proposal exists.
Reject and abandon move nothing.

After every meeting both sides `reevaluate`. A pop with a satisfaction
bookmark runs `satisfy_continue`, so goods just received get reserved. Offers
are read from free stock the next time they are asked, so a spent offer
shrinks or disappears. A rejected pair is not retried in that call.

A seller who is requesting something rejects a basket that does not give them
one of those goods. With no request, they accept when the payment AMV covers
what they give and they can spare it.

**Code:** `src/game/pop.rs`, `src/game/deal.rs`, `src/game/market.rs`,
`src/game/desire.rs`, `src/game/pop_property.rs`.
