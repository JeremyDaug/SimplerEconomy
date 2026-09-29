# Pops

Read this for desire satisfaction, consume, and the first deal pass.
`examples/pop_tester` loads `data/world`, places two pops on one market, and
runs `Market::market_day`. It does not choose baskets or prices.

## Landed vs stub

| Piece | Status |
|-------|--------|
| `Pop::satisfy` | Reserves free stock and records satisfaction. Starts at the first basic desire. Stops at the first target it cannot take in full. Returns a copy of that desire, or `None` |
| `Pop::satisfy_continue` | Walks forward from the bookmark `satisfy` wrote. No bookmark: same as `satisfy` |
| `Pop::satisfy_tier` | One tier, from its first desire. Does not move the bookmark |
| `Pop::consume` / `consume_tier` / `consume_one_desire` | Called from `Market::market_day` after exchange. `PlayState::phase_pop_consumption` is still `todo` |
| Savings between tiers, and between luxury iterations | Not written. Intended order is tier, then that tier's savings, then the next tier. Luxury is level, savings, next level |
| `Market::match_deals` | Pair on one good, buyer proposes, seller accepts or rejects. Returns every `Meeting`. Only an accepted basket moves goods and pays freight. Both sides reevaluate afterward |
| `Market::market_day` | One market, in order: reserve, produce, `match_deals`, consume, decay, actor books and planning, then the night card. Firm produce, reserve, and plan are empty. Institutions keep the empty `DealMaker` defaults |
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
time. `Market::market_day` calls it every morning. After new stock arrives
inside a day, `reevaluate` calls `satisfy_continue`.

## Consume

Basic and common run once each, in list order, and do not stop when a desire
is short. Luxury benches any desire that missed the current level and keeps
filling the ones that made it.

Each call sets its gap to a full `amount`, ignoring satisfaction already
recorded. It subtracts the take from `quantity` and `reserved`, moves consume
targets to `consumed` and use targets to `used`, and adds satisfaction.
Running it on goods `satisfy` already counted records the level twice.
`market_day` does both, so a normal day counts that level twice.

## Traps

- `desires` must have three tiers, indexes 0, 1, and 2.
- The bookmark's target index is into `desire.target` as stored, not an
  efficiency sort. It stays valid only while that bucket is unchanged.
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
shrinks or disappears. A rejected or abandoned pair is not retried in that
call. `match_deals` returns every `Meeting`. Only `Accepted` moves goods.

## Proposal and evaluate

Meeting order is unchanged. `Pop::propose` and `Pop::evaluate` judge a pop's
own ends. A firm still uses `seller_can_accept` only: match good present,
stock on hand, and absolute AMV of payment covering absolute AMV given.

`Pop::propose` (`src/game/pop.rs`) builds one `ProposedDeal`:

- `goods` is the buyer's change. Positive units come from the seller.
  Negative units go to the seller. A good is only on one side.
- The positive match good is `min(units still needed, seller's listed offer)`,
  whole units, at least 1 or the buyer abandons.
- Payment is sized in holding value against a seller who has no desire for
  the goods. Holding value of a positive AMV is `amv * amv_scale(salability)`.
  A negative AMV is not scaled. Exact holding parity is a tie, so the buyer
  adds one more whole unit. The buyer's own evaluate must also accept.
- A good whose free units still feed the lowest tier with a whole unit of
  room stays out of sell orders and out of payment, including a seller
  request for it. A higher tier can still be spent on that lower tier.
- Seller requests come first. Then the buyer's other free goods, highest
  monetary rating first, lowest id on a tie. Reserved stock is already out
  of `available`. Transport goods are skipped unless the seller requested
  them.
- Freight is unchanged: `transaction_cost + bulk * market friction`, or 0
  when the world has no transport good. The buyer still abandons when stock
  plus the basket cannot cover it.

`Pop::evaluate` accepts when all of these hold:

- The match good is given at 1 or more, something comes back, and every given
  good is within free stock.
- A given unit that still feeds a desire is allowed only when something
  received feeds that end or an earlier one. Earlier is a lower tier, or the
  same tier and an earlier list index. Coin does not buy a good the seller
  can still use.
- On the tier of the best end received, satisfaction gained must exceed
  satisfaction given up. Lower tiers are not subtracted.
- With no satisfaction gain, holding-value credit must exceed holding-value
  cost. With a satisfaction gain, cost may run up to `credit * LOSS_LIMIT`
  (`4`). A good that feeds a desire is credited at face AMV. A good that
  feeds nothing is credited at holding value.

Propose does not see the seller's desires. A seller who still needs the
match good rejects, and nothing moves.

Finalize moves the map, then the buyer spends transport they hold (including
transport just received) until `freight` is covered.

## Market record

`MarketGood.amv` and `MarketGood.salability` are the published card.
`Market::history` copies them once at the start of `match_deals`, plus
friction. A missing AMV reads as `1.0`. A missing salability reads as
`SALABILITY_DEFAULT` (`0.1`). Deals do not write the card.

`amv_scale` clamps salability to `0.05..=1` and is the only multiplier on a
positive AMV. `monetary_rating` is `max(salability - 1, 0)`. It orders
payment. It does not change evaluate.

The day's exchange and rot sit on `MarketGood` with the card: `traded`,
`paid`, `sought_unmet`, `offered_unsold`, `decayed`, and `volume`. History
still copies only AMV, salability, and friction. `match_deals` adds `traded`
and `paid`, then sets `sought_unmet` and `offered_unsold` from the book it
left behind. After consumption, `market_day` calls
`note_decay(good, decayed, volume)` for each pop and firm. A good missing
from the card is inserted at AMV `1` and salability `0.1`.

`Market::record_keeping` is the night write:

- Trade pressure `(sought - unsold) / (sought + unsold + traded + 1)`, capped
  at a quarter of the absolute AMV.
- Production flow `(consumption - production) / (stock + production +
  consumption + 1)`, capped at `0.05` of the absolute AMV.
- Rot, when `decayed` and a base are present: base is `volume`, or `stock`
  when volume is 0. AMV falls by `fraction * |AMV|`, fraction
  `decayed / base` clamped to `0..=1`.
- If AMV fell, salability falls by that loss over the old absolute AMV,
  capped at `0.2`. A night that took the good in payment and did not lose
  AMV raises salability by `0.05`.
- The tape is cleared. Production and consumption are zeroed. Stock stays.

Actor `record_keeping` and `plan` read a snapshot taken after decay and
before that night write, so they still see today's card. Exchange priced
itself from the snapshot at the start of `match_deals`. Firm record keeping
clears that firm's production counters. `Pop::plan`, `Firm::reserve_for_day`,
`Firm::produce`, and `Firm::plan` are empty.

PlayState's phase methods are still stubs. Institution decay and institution
record keeping still panic and are not called.

**Code:** `src/game/pop.rs` (`propose`, `evaluate`), `src/game/deal.rs`
(`Meeting`, `seller_can_accept`, `freight_shortfall`, `DealMaker` day steps),
`src/game/market.rs` (`market_day`, `MarketGood`, `history`, `record_keeping`),
`examples/pop_tester/main.rs`. Also `src/game/desire.rs`,
`src/game/pop_property.rs`.
