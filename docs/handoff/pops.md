# Pops

Read this for desire satisfaction, consume, cottage work, and the first deal pass.
`examples/pop_tester` loads `data/world` and `data/pop_tester/`,
places those pops on one market, grants the day's Time, and runs
`Market::market_day`. The folder sets the opening board, crafts, stock,
household size, and the first morning's line targets. Species, culture,
stratum, and religion files hold the demographic desires, and `Pop::update_desires`
copies them onto each pop. The tester prints the meetings, including the
morning buys/sells ratio. It does not choose baskets or prices. The night's
plan rewrites every line target.
`docs/handoff/desire.md` sketches how a platonic desire becomes a
demographic desire and then a pop desire.

## Where this stopped

Cottage planning, savings, and the first deal pass are the current stop.
Seller weight is a first cut: the whole units on the offer. Refine that
weight later. The buyer draw is still one ticket per buy.

A specialty line aims at its cover, its paced surplus, and the unmet buys
it is allowed to take. It does not take the rest of the morning. Each pop's
surplus is one offer. The weight pulls a meeting toward the larger pile.
The other pops still make and sell the same goods.

## Landed vs stub

| Piece | Status |
|-------|--------|
| `Pop::satisfy` | Reserves free stock and records satisfaction. Starts at the first basic desire. A target that can be taken in full comes first. Stops if the chosen target cannot. Returns a copy of that desire, or `None` |
| `Pop::satisfy_continue` | Walks forward from the bookmark `satisfy` wrote. No bookmark: same as `satisfy` |
| `Pop::satisfy_tier` | One tier, from its first desire. Does not move the bookmark |
| `Pop::consume` / `consume_tier` / `consume_one_desire` | Called from `Market::market_day` after exchange. `PlayState::phase_pop_consumption` is still `todo` |
| Savings between tiers, and between luxury iterations | Written. Basic and common place that day on a good the desire eats. Luxury places it on the highest `salability * durability * fill` good. Withheld from selling and payment. The job does not spend that pile as an input. Basic, common, and the first luxury level may eat it. A further luxury level leaves it. See Savings |
| `Market::match_deals` | Pair on one good, buyer proposes, seller accepts or rejects. Returns every `Meeting`. Only an accepted basket moves goods and pays freight. Both sides reevaluate afterward |
| `Market::market_day` | One market, in order: reset, reserve, produce, a second pop satisfy for what was just made, `match_deals`, consume, pop growth, decay, `rescale_desires`, actor books and planning, then the night card. Rescale is after decay so this day's desire effects stay at the old size. A pop's reserve is satisfy, then `apply_craft`, then the job reserve. Firm produce, reserve, and plan are empty. Institutions keep the empty `DealMaker` defaults |
| Cottage job | On the pop's stock. Plans, reserves, produces, and shops inputs. See Cottage work |
| `Pop::plan` | Covers every short desire tier the job can make. Each paying line adds its own paced surplus. Unmet buys, plus units downstream production used, add runs on the best line. A line that does not pay aims at the cover and those runs. One already running keeps a run when that aim is gone. A covering line returns to the order it was added, ahead of a line that only pays or only feeds one of those lines. A line that is not covering, and does not feed one that is, moves to the back. A producer of an input runs before the line that consumes it. `Firm::plan` is empty |
| Stratum | Replaces class. A culture's economic subgroup. Id `0` is empty. Desires, rate addends, and the work-time addend stack with species, culture, and religion. The culture lists the stratum ids that derive from it. Stratification content is 0.3 |
| `Desire.decay` | Morning reset multiplies each desire's satisfaction by it. `0.0` clears satisfaction |

## Satisfy

Order is tier, then list index. Inside a bucket, a target that can be
taken in full comes first. Among those, high priority, then higher
efficiency, then the earlier index. When none can finish, the target
with the most satisfaction on hand wins, then that same tie break.
`ordered_targets` is not used.

Basic and common each get one level. Luxury repeats one level at a time.
The next luxury level starts only after every luxury desire has reached the
current one. An empty tier is finished.

Free stock is `quantity - reserved`. A take adds to `reserved` and to
`satisfaction`. `quantity` stays put. A partial take on the stopping target
is kept. Later targets in that bucket, and every later desire, are left alone.
A target whose cap is filled does not stop the desire; the next target is used.
A target whose cap is below one level, and whose open cap can be taken in
full, is reserved before a substitute takes the level. The substitute
fills what that cap left open.
Each target's contribution toward the open level is stored on the desire
(`target_taken`), so a later `satisfy` call still sees that cap as spent.
Completing the level clears those takes so the next luxury level can use its
caps again. The morning reset clears them and multiplies satisfaction by
`Desire.decay`.

The bookmark is tier, desire index, the chosen target's index in the bucket,
the satisfaction the desire already had when that target started, and the
level being filled. `satisfy_continue` finishes that target when its open
cap can be taken in full, and only the gap up to the current level
(`iter_target * amount - satisfaction`). When that cap is still short and
another target can be taken in full, the other target is reserved instead.
The short bookmark stops the walk only when nothing else can. When the open
desire reaches the level, the walk moves on.

`satisfy` always starts over at the first basic desire and replaces the
bookmark. Caps already taken toward the open level stay spent. `Market::market_day`
calls it every morning. After produce, and before exchange, each pop's
`satisfy_made` continues that bookmark. Luxury stops after one level, so
a unit above that level stays free to sell. No bookmark means the morning
walk finished, and the new output is left free. After a meeting,
`reevaluate` calls `satisfy_continue`.

The stopped desire buys every good that can still fill that open level.
Each amount rounds up to a whole unit and stays inside the good's remaining
cap. A luxury desire buys as many levels as it finished last night, and at
least one. The saved day's shortfall is added on top.

## Consume

Basic and common run once each, in list order, and do not stop when a desire
is short. Luxury benches any desire that missed the current level. A further
luxury level runs only when the luxury day was placed and is still on hand,
and leaves those units. An earlier short day means that luxury day was not
placed. The saved day is not a finished level.

`Pop::consume` first sets every desire's satisfaction to zero, so the level
`satisfy` recorded for the reserved goods is counted once, when those goods
are eaten. Each call sets its gap to a full `amount`. It subtracts the take
from `quantity` and `reserved`, moves consume targets to `consumed` and use
targets to `used`, and adds satisfaction. `consume_tier` and
`consume_one_desire` do not clear satisfaction on their own.

## Savings

One extra day per desire. The rate is fixed for this tester. Demographics
and sentiment do not change it. The day is grossed up by `1 / durability`.
Basic and common place it on a good that desire eats. Luxury may place it
on any good. Among those candidates, it sits on the single highest
`salability * durability * fill` good.
Fill is 1 when this pop makes the good or already holds that day, and
otherwise the share of today's buy that arrived. A good that decays
completely is not a candidate. The eaten good keeps a tie. A day outside
the desire is the holding value of the eaten day, and an exact holding
total adds one more whole unit.

The pile is withheld from selling and payment, and the job does not spend
it as an input. Basic, common, and the first luxury level may eat it. A
further luxury level leaves it. Output an earlier line made this morning counts
toward the pile, and only the surplus over it feeds a later line. A later
tier's day is not added while an earlier day is short of one day's
units of its winning good. The pile stored is the larger grossed hold.
Tomorrow's meal is not set aside before that check. A further
luxury level waits while the earlier day is short. The job covers a day it can
make. The morning buy adds the shortfall on the winning good.

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
above what still feeds a desire. See Proposal and evaluate.

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
Time, are gone before freight. A line may spend free stock an earlier
line made that morning, above the saved day. A line that spends time leaves enough for a
later line it feeds when that line yields more per unit of time.
Free units go before a desire's reserve. New
output lands in `quantity`, `fresh`,
and `produced`. If a lower tier is still unsatisfied, the pop tries to
reserve, buy, and produce that tier's goods. Units above what still feeds
a desire are offered the same day. Consume runs after exchange.

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
time still in `quantity`. The cover walks every tier over that stock.
One level is `amount / efficiency` of a single target. A target whose
cap is below 1 cannot finish that level. The highest efficiency that
stock can fill is spent, and a tie keeps the later target. A short
desire adds the highest-efficiency target some line outputs, and only
when that cap is at least 1. Units of one good are summed, then the
remaining stock of that good is subtracted once. A desire no line can
make adds nothing. A saved day this job can make is included in that gap.
When the line that takes a gap costs more per unit than that good's
holding, those units are also posted as a buy. The line still aims at
the gap. A purchase on hand shrinks the next night's gap.

`Job::plan` writes `Some` on every line. The line with the highest
output per unit of time takes each short good. A tie keeps the earlier
line. Its cover is the largest `(gap + 1) / output.amount` among the
goods it took. The extra unit is one more of that output than the gap.
Inputs a cover line consumes, when this job also makes them and they
survive the night, are added on top of that gap and assigned the same
way. That cover is a floor. Every line with a positive holding score adds its own surplus
on that floor. The score is one iteration's output holding minus the
holding of destroyed and consumed inputs. An input that does not survive
the night, such as time, is costed at holding 1. The extra iterations are what
required inputs remain after the cover's need and after earlier lines'
surplus, or one iteration when the process has no required input.
Surplus then paces off that output. The share is
`(lost + kept) / produced`. `kept` is the output still in `quantity`,
and no more than `produced`. At a share of `0.1` it holds the runs just
made. Under that it can grow, up to twice those runs when nothing rotted
and nothing of that output is left on hand. Over that it pulls back by
`0.1 / share`. `produced == 0` and `lost > 0` leaves one extra run.
Unmet buys of an output, plus units downstream production used, above
stock on hand, add runs on the best line for that good. That line takes
the whole shortfall above stock when its yield matches the best process
known. A worse process adds no more than its cover. One that costs
more to make than the good's holding adds none of that shortfall. The cover plus the
surplus plus those runs is the ideal for a line that pays. A line that
does not pay aims at the cover and those runs. A line already running
keeps one run when that aim is gone. A line at 0 stays at 0. The stored
target steps at most a quarter of the way toward its aim, using 1 as the
base when the target is below 1. A snap writes a new cover in whole when
the step has not reached it, and does not add the old target on top.
Those runs stay in that aim, so a snap does not drop them. A covering line returns to the order it was added, ahead of a line that
only pays or only feeds one of those lines. When a line is not covering a
short good and does not feed one that is, it moves to the back of the list.
A line that makes an input runs before the line that consumes it. A new or removed desire, or a
growth change of a desire amount,
snaps that night. A paying line with a stored cover writes the new cover
whole and steps only the surplus above the old cover. A paying line whose
stored cover is still 0 steps its current target toward the new cover
plus the surplus and those runs.

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
open desire buy for the same good. The list is the next run's input need
after decay, for goods that survive the night. A good that decays completely,
such as time, is not bought. Morning reserve writes that list before produce.
After produce, and after the second satisfy, the list is rebuilt from stock
still free. An input this morning already made is not bought again.
Output an earlier line will make tomorrow morning covers the later line's
need before a buy is posted. Capital
set aside in `used` still counts, because it returns at decay. A line whose
last plan did not pay also shops one unit of each output that survives the
night: `1 / (1 - decay)` minus the
units already on hand. A larger input buy for that good stays. The first
morning, before a plan, does not shop an output. A purchase shrinks that
list. The next morning's reset drops the list and the claim book. Targets stay.

## Match

Exchange, production, and consumption stay separate phases.

`Market::match_deals` collects sells, then picks a buyer at random from those
who still have a buy, and a seller from the valid matches in proportion to
the whole units that seller offers. An offer under one unit has no weight.
When every offer has no weight, that draw is uniform. The buyer draw does
not grow with the size of the buy. They meet because one sought good is one offered good. The buyer is
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
shrinks or disappears, and a surplus left by the trade is offered.
A rejected or abandoned pair is not retried in that
call. `match_deals` returns every `Meeting`. Only `Accepted` moves goods.

## Proposal and evaluate

Meeting order is unchanged. `Pop::propose` and `Pop::evaluate` judge a pop's
own ends. A firm still uses `seller_can_accept` only: match good present,
stock on hand, and absolute AMV of payment covering absolute AMV given.

`Pop::propose` (`src/game/pop.rs`) builds one `ProposedDeal`:

- `goods` is the buyer's change. Positive units come from the seller.
  Negative units go to the seller. A good is only on one side.
- The positive match good starts at `min(units still needed, seller's listed offer)`,
  whole units. A basket that cannot be paid steps down to the largest whole
  quantity that can. Below 1, the buyer abandons.
- Payment is sized in holding value against a seller who has no desire for
  the goods. Holding value of a positive AMV is `amv * amv_scale(salability)`.
  A negative AMV is not scaled. Exact holding parity is a tie, so the buyer
  adds one more whole unit. The buyer allows that holding loss when the
  purchase feeds no open desire. The seller still requires a gain.
- Sell orders offer the free units above what still feeds a desire and above the savings pile.
  Payment offers that surplus, and also units that feed only a later
  tier than the good being bought. Same-tier goods are not spent on each
  other. A purchase that feeds nothing spends the surplus alone. That room is the target's cap, `amount * cap`, minus what that
  target already reserved toward the level (`target_taken`), and no more
  than the gap to one level, `amount - satisfaction`.
  An input the job consumes to make a desired output counts as that desire,
  except a good that decays completely. Units already reserved are not free.
  The market reads sell orders again after every meeting.
- Seller requests come first. Then the buyer's other free goods, highest
  monetary rating first, lowest id on a tie. Reserved stock is already out
  of `available`. Unrequested transport is spent last, and only above the
  freight bill of the basket so far. Requested transport is ordinary payment.
- Freight is unchanged: `transaction_cost + bulk * market friction`, or 0
  when the world has no transport good. When the buyer cannot cover the
  bill, the bulkiest non-transport good in that basket is left out and the
  same quantity is tried again, until the basket pays or every remaining
  payment good is transport. The buyer abandons when stock plus the basket
  still cannot cover it.

`Pop::evaluate` accepts when all of these hold:

- The match good is given at 1 or more, something comes back, and every given
  good is within free stock.
- A given unit that still feeds a desire is allowed only when something
  received feeds that end or an earlier one. Earlier is a lower tier, or the
  same tier and an earlier list index. An input consumed to make a desired
  good feeds that desire. Units given are taken from the surplus first, so
  a gift inside that surplus has no end use. Coin does not buy a unit that
  still feeds a desire.
- On the tier of the best end received, satisfaction gained must exceed
  satisfaction given up. Lower tiers are not subtracted.
- With no satisfaction gain, holding-value credit must exceed holding-value
  cost. With a satisfaction gain, cost may run up to `credit * LOSS_LIMIT`
  (`4`). A good that feeds a desire is credited at face AMV. A good that
  feeds nothing is credited at holding value.

Propose does not see the seller's desires. The listed offer already leaves
out units that still feed a desire. Giving those units away is rejected
when nothing received feeds that end, and nothing moves.

Finalize moves the map, then the buyer spends transport they hold (including
transport just received) until `freight` is covered.

## Market record

`MarketGood.amv` and `MarketGood.salability` are the published card.
`Market::history` copies AMV, salability, friction, units still sought, and production.
`match_deals` takes that snapshot at the start, after the morning clear.
A missing AMV reads as `1.0`. A missing salability reads as
`SALABILITY_DEFAULT` (`0.5`). Deals do not write the card.

`amv_scale` clamps salability to `0.05..=1` and is the only multiplier on a
positive AMV. `monetary_rating` is `max(salability - 1, 0)`. It orders
payment. It does not change evaluate.

The day's exchange and rot sit on `MarketGood` with the card: `traded`,
`paid`, `print_holding`, `print_units`, `sought_unmet`, `offered_unsold`,
`decayed`, and `volume`. History copies `sought_unmet` when it is still
positive, and `production` when it is still positive. A snapshot taken
before exchange has no sought units. `match_deals` adds `traded` and `paid`, records the print, then
sets `sought_unmet` and `offered_unsold` from the book it left behind.
After consumption, `market_day` calls
`note_decay(good, decayed, volume)` for each pop and firm. A good missing
from the card is inserted at AMV `1` and salability `0.5`.

`Market::record_keeping` is the night write:

- Accepted deals record a print from the morning card. Payment holding is
  split across received goods with positive holding, by holding times units.
  The night closes a tenth of the gap between morning holding and
  `print_holding / print_units`. A good that was offered and did not trade
  also falls by `0.005` of the payment scale. Those two together are clamped
  to `±0.05` of that scale, applied in holding space, then written back to
  AMV. Open buys put that night's rot back and then rise by `0.01` of the
  payment scale. Production flow is
  `(consumption - production) / (stock + production + consumption + 1)`,
  capped at `0.005`, taken off the payment scale: the paid-unit weighted
  holding value of goods that were paid and have positive holding value, or
  `1` when none were.
- Rot, when `decayed` and a base are present: base is `volume`, or `stock`
  when volume is 0. AMV falls by `fraction * |AMV|`, fraction
  `(decayed / base) / 4` clamped to `0..=1`.
- If AMV fell, salability falls by that loss over the old absolute AMV,
  capped at `0.01`. A night that took the good in payment and did not lose
  AMV raises salability by `0.05`, including past par. A good that was
  bought and not paid out rises by the same step and stops at par. A good
  that was only offered, and did not sell, does not rise that way.
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
