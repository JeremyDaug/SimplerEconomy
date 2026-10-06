# Pops

Read this for desire satisfaction, consume, cottage work, and the first deal pass.
`examples/pop_tester` loads `data/world` and `data/pop_tester/`,
places those pops on one market, grants the day's Time, and runs
`Market::market_day`. The folder sets the opening board, crafts, stock,
household size, and the first morning's line targets. Species, culture,
stratum, and religion files hold the demographic desires, and `Pop::update_desires`
copies them onto each pop. The tester prints the meetings. It does not
choose baskets or prices. The night's plan rewrites every line target.
`docs/handoff/desire.md` sketches how a platonic desire becomes a
demographic desire and then a pop desire.

## Landed vs stub

| Piece | Status |
|-------|--------|
| `Pop::satisfy` | Reserves free stock and records satisfaction. Starts at the first basic desire. Stops at the first target it cannot take in full. Returns a copy of that desire, or `None` |
| `Pop::satisfy_continue` | Walks forward from the bookmark `satisfy` wrote. No bookmark: same as `satisfy` |
| `Pop::satisfy_tier` | One tier, from its first desire. Does not move the bookmark |
| `Pop::consume` / `consume_tier` / `consume_one_desire` | Called from `Market::market_day` after exchange. `PlayState::phase_pop_consumption` is still `todo` |
| Savings between tiers, and between luxury iterations | Not written. Intended order is tier, then that tier's savings, then the next tier. Luxury is level, savings, next level |
| `Market::match_deals` | Pair on one good, buyer proposes, seller accepts or rejects. Returns every `Meeting`. Only an accepted basket moves goods and pays freight. Both sides reevaluate afterward |
| `Market::market_day` | One market, in order: reset, reserve, produce, `match_deals`, consume, pop growth, decay, `rescale_desires`, actor books and planning, then the night card. Rescale is after decay so this day's desire effects stay at the old size. A pop's reserve is satisfy, then `apply_craft`, then the job reserve. Firm produce, reserve, and plan are empty. Institutions keep the empty `DealMaker` defaults |
| Cottage job | On the pop's stock. Plans, reserves, produces, and shops inputs. See Cottage work |
| `Pop::plan` | Covers the first short desire tier the job can make. Each paying line adds its own paced surplus. A line that does not pay and is not covering moves to the back and keeps its target. `Firm::plan` is empty |
| Stratum | Replaces class. A culture's economic subgroup. Id `0` is empty. Desires, rate addends, and the work-time addend stack with species, culture, and religion. The culture lists the stratum ids that derive from it. Stratification content is 0.3 |
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
- `Pop::rescale_desires` runs in `market_day` after goods decay and before
  planning. It rewrites amount, additive effects, and recorded satisfaction
  from the new size. `update_desires` does not. A desire with no stored
  demographic source is left alone.
- `PlayState::phase_pop_growth` only grows. A turn that also runs
  `market_day` grows those pops twice.

## Cottage work

A pop's `Job` (`src/game/job.rs`) runs on that pop's property. It does not
set a price and does not sell. `Pop::sell_orders` offers free whole units
that do not feed the lowest tier with room left.

Craft `0` is no baseline. Lines still run. An empty line list skips plan,
reserve, produce, and job buys. Two pops of the same craft keep separate
lines, targets, and stock.

`data/world/crafts.toml` is the open list (`origin` none). A culture-origin
craft, then a religion-origin craft, drops and appends process ids and
multiplies `complexity_modifier`. An omitted modifier is `1.0`. Morning `apply_craft`
adds only processes the job does not already run, as resting lines
(`Some(0.0)`). A line already present keeps its target, so a process added
that morning produces on a later day, after the night's plan.

`JobLine.target` is the quota. `None` runs as far as inputs on hand allow
and does not shop. `Some(0.0)` skips the line. `Some(n)` with `n > 0` runs
up to `n` and shops enough of each spent input that another `n` is on hand after decay, rounded up to a whole unit. Required inputs are always drawn.
Optional inputs are drawn when the line lists them. A missing required
factor shops one unit and leaves the other inputs free.

Produce runs before exchange. Inputs destroyed that morning, including
Time, are gone before freight. New output lands in `quantity`, `fresh`,
and `produced`. If a lower tier is still unsatisfied, the pop tries to
reserve, buy, and produce that tier's goods, and those goods stay off the
sell book. Higher-tier goods are open for use and exchange. Consume runs
after exchange, so the tester's extra bread is offered at the next day's
exchange.

`Actors::start_day` takes the markets and grants good 0 by
`ScalingFactor::Labor(TIME_PER_LABOR)` (64 quarter-hours per labor). A
positive grant is added to that good's production on the pop's market.
The call sits in the tester,
before `market_day`. The morning reset leaves that production in place.
The library day does not grant Time.

`Pop::plan` runs after `rescale_desires`, so it uses the new amounts.
Decay has already written each row's `lost`. `produced` is still today's
output, because the morning reset has not cleared it. The stock passed
to the job includes the next morning's time grant on good 0, added to
time still in `quantity`. The cover walks
tiers from basic upward over `quantity`. One level is `amount / efficiency`
of a single target. The highest efficiency that stock can fill is spent,
and a tie keeps the later target. The walk stops at the first tier that
still has a short desire and finishes that tier. A short desire adds the
highest-efficiency target some line outputs. Units of one good are summed,
then the remaining stock of that good is subtracted once. A desire no
line can make adds nothing.

`Job::plan` writes `Some` on every line. The first line that outputs a
short good takes it, at the largest `(gap + 1) / output.amount`. The extra unit is one more of that output than the gap. That cover is
a floor. Every line with a positive holding score adds its own surplus
on that floor. The score is one iteration's output holding minus the
holding of destroyed and consumed inputs. The extra iterations are what
required inputs remain after the cover's need and after earlier lines'
surplus, or one iteration when the process has no required input.
Surplus then paces off that output. At `lost / produced` of
`0.1` it holds the runs just made. Under that it can grow, up to twice
those runs when nothing rotted. Over that it pulls back by
`0.1 / share`. `produced == 0` and `lost > 0` leaves one extra run.
The cover plus that extra is the ideal for a line that pays. The stored
target steps at most a quarter of the way toward it in one night, using
1 as the base when the target is below 1. A line that does not pay does
not step down. When it is not covering a short good, it moves to the
back of the list and keeps the target it had. A new or removed desire,
or a growth change of a desire amount, snaps that night's cover in whole
and steps only the surplus of a line that pays.

`Job::complexity_cost` is the modifier `(complexity_modifier +
craft_distance)`, capped at `1.0`. Each process id counts once. The
weight is `pop.craft_distance` in config (default `0.1`): half for a
missing baseline process, full for an extra process. No base craft makes
`Pop::complexity_cost` return `1.0`.

`Job::plan` then stores `plan_cost`. Each positive target adds
`modifier * (complexity^2 - overlap) * iterations`. Overlap is the
fraction of that line's input and output goods that another line also
uses. Iterations stay on the desire gap.

Job buys are the shopping list, rounded up to a whole unit, added onto an
open desire buy for the same good. A purchase shrinks that list. The next
morning's reset drops the list and the claim book. Targets stay.

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
`paid`, `print_holding`, `print_units`, `sought_unmet`, `offered_unsold`,
`decayed`, and `volume`. History still copies only AMV, salability, and
friction. `match_deals` adds `traded` and `paid`, records the print, then
sets `sought_unmet` and `offered_unsold` from the book it left behind.
After consumption, `market_day` calls
`note_decay(good, decayed, volume)` for each pop and firm. A good missing
from the card is inserted at AMV `1` and salability `0.1`.

`Market::record_keeping` is the night write:

- Accepted deals record a print from the morning card. Payment holding is
  split across received goods with positive holding, by holding times units.
  The night closes a tenth of the gap between morning holding and
  `print_holding / print_units`. A good that was offered and did not trade
  also falls by `0.05` of the payment scale. Those two together are clamped
  to `±0.1` of that scale, applied in holding space, then written back to
  AMV. Unmet buys do not move AMV. Production flow is
  `(consumption - production) / (stock + production + consumption + 1)`,
  capped at `0.05`, taken off the payment scale: the paid-unit weighted
  holding value of goods that were paid and have positive holding value, or
  `1` when none were.
- Rot, when `decayed` and a base are present: base is `volume`, or `stock`
  when volume is 0. AMV falls by `fraction * |AMV|`, fraction
  `(decayed / base) / 4` clamped to `0..=0.95`.
- If AMV fell, salability falls by that loss over the old absolute AMV,
  capped at `0.2`. A night that took the good in payment and did not lose
  AMV raises salability by `0.05`.
- The tape is cleared. Production and consumption are zeroed. Stock stays.
- The card is then restated in one unit. That good is the positive-AMV row
  with the highest salability. Equal salability goes to the largest payment
  value (units paid times its pre-write holding value), then the lower id.
  Every AMV is divided by the unit's AMV, so the unit is 1. A negative AMV
  is divided by the same price. Salability is not written again.

Actor `record_keeping` and `plan` read a snapshot taken after decay and
`rescale_desires`, and before that night write, so they still see today's
card. Exchange priced itself from the snapshot at the start of
`match_deals`. Firm record keeping clears that firm's production counters.
`Pop::plan` rewrites job targets from that snapshot and from each row's
`produced` and `lost`. `Firm::reserve_for_day`,
`Firm::produce`, and `Firm::plan` are empty.

`PlayState::phase_pop_growth` only grows and does not rescale.
`phase_pop_consumption` is still `todo`. Institution decay and institution
record keeping still panic and are not called.

**Code:** `src/game/pop.rs` (`propose`, `evaluate`, `apply_craft`, `plan`),
`src/game/job.rs`, `src/game/craft.rs`, `src/game/deal.rs`
(`Meeting`, `seller_can_accept`, `freight_shortfall`, `DealMaker` day steps),
`src/game/market.rs` (`market_day`, `MarketGood`, `history`, `record_keeping`),
`examples/pop_tester/main.rs`, `examples/pop_tester/load.rs`,
`data/pop_tester/`, `data/world/crafts.toml`. Also
`src/game/desire.rs`, `src/game/pop_property.rs`.
