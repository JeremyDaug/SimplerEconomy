# Pops

Read this before changing the pop tester, `Market::market_day`, or the morning plan.
`examples/pop_tester` loads `data/world` and `data/pop_tester/`,
places those pops on one market, grants the day's Time, and runs
`Market::market_day`. The folder sets the opening board, crafts, stock,
and the first morning's line targets. A missing household count is 10.
The pop tester's opening pop is 100 households.
A household table overrides the adult, elder, and child averages. The
firm tester loads its pops through the same reader. Species, culture,
stratum, and religion files hold the demographic desires, and `Pop::update_desires`
copies them onto each pop.

The tester prints each pop's work, then the pop table, then founding, then
the market board. Founding is days of set-aside,
whether that clears the wealth floor, the lowest unmet
desire, and the goods offered or made today, skills left out. A pop
removed today adds a row marked `left`, with why it went. The market board is AMV with a mark for the move from the morning ask (`^` up, `v` down, `=` the same, and up to two extra marks on a larger move), salability, units lost to rot, and that loss as a percent, then the day's book: offered, requested,
exchanged, and the price. A good that drew no quote leaves the book blank. A book that did not trade prints `none`. The
work table is the craft and the schedule. The schedule stays in its own
column and ends with the complexity cost of the current targets. `pop N`
or `p N` prints that pop's holdings and buy/sell (`buy`, `sell`, `bought`,
`sold`, `back+`, `back-`). A bare name is working stock that prints as
zero. A skill stays off the market board and that look unless it traded.
Each day starts and ends with its number. The pop
table is standard of living, people, the change in households, net
households from migration, and the five sentiment shares. The last row
sums people, the household change, and migration, and averages the
standard of living, sentiment, and the per-household books. Size is people.
Growth is the household change since that morning. It does not choose
baskets or prices. `docs/handoff/desire.md` sketches how a platonic desire
becomes a demographic desire and then a pop desire.

Cottage work through Market record, below, is the earlier one-to-one day.
`Market::market_day` does not call `match_deals` or `Pop::plan`. The live
day is this stop note and `src/game/ladder.rs`. `TODO.md` still describes
that older day in its 0.1 paragraph. This note is the status.

## Where this stopped

The mass day is in. One sell book, then each pop buys. The morning plan is
`Job::plan_purchases`. The night does not call `Pop::plan`. Firms are not
on the ladder. `Firm::plan` stays empty. `PlayState` is not in this cut.

The day order is reset, then each pop writes its purchase targets,
`buy_ladder` reserves each desire as the walk reaches it, the input hold
returns to property, produce,
consume, rest, sentiment, growth, the population cap, prune under one
household, internal migration, one founded pop, decay, rescale desires,
record keeping,
gather use, and the night card. Yesterday's mass offer and exchange stay
on the history the plan reads. Sentiment runs before growth. Rescale runs
after decay and after migration.

`buy_ladder` orders pops by currency AMV per household, then the lower id.
Each pop buys and reserves the job's inputs, then walks basic desires in
order. For each desire the bucket is ranked
by satisfaction per AMV first. The winning good is reserved from stock on
hand, and that reservation comes off the sell line. The rest of the slice
is the order, and an empty book still posts it.
A buy that cannot finish leaves that good and the next one is
tried. The slice rounds up to a whole unit, and so does each good's minimum
share of one over twice the goods in the bucket. It then saves one level
and saves another. A basic
level the purse cannot buy stops before that save. The input buy has
already run. Common walks its desires, then saves, then buys any inputs
still open, then saves another level. An open common level still saves
and still buys those inputs. Luxury
keeps the shorter batch walk. A batch that does not finish saves the step
already reached and leaves. An unfilled whole
input stops that pop before the later tier. The next desire runs while a whole
coin remains. The morning book lists
every unit held, working and saved, including time and skills. An untradeable
good that is not a skill stays off. Saved units leave the live line before
the first buy. Tender the plan kept stays listed. Desires do not spend it,
and the currency save does not either. The input buy does. A savings target
the purse cannot finish still lets that buy run. A pop with no line running
then stops. A line already set to run keeps buying the later tiers. A
positive holding lists, including a fraction. A currency lists the whole
pile. The buyer pays in whole coins, and
one extra coin covers a remainder the coins do not divide. A currency is bought with other
goods when the tender on hand cannot cover it, even when another tender
is in stock. A good that is not a currency is paid only in tender.

Each savings level is `savings_amv` times the mean card price of the desire
tiers, per household. The world file sets that multiplier to 3. Goods in a tier are averaged equally, and the tiers
are averaged equally. A pop with no priced tier saves one per household.
The save takes a coin that is already the most durable, or a cheaper coin
when a more durable one is on offer. A cheaper coin with nothing more
durable for sale stays in the purse, and that step does not stop the walk.
A short reservable purse writes the coins that fit. A level still short of
its target stops a pop that is not running a line. A running line keeps the
later buys with the coin the save left working. An empty reservable pool
writes nothing and does not stop the walk. The durability swap then spends
saved currency on a more durable coin that is in stock. The night's offer is what remained for sale
plus what exchanged, so grain spent as tender or set aside is not an
unsold offer. Beside that, the day records an initial offering (the morning
book), an effective offering (that book minus reservations), and excess
supply (what is still listed at the end). A maximal buy is the units a
purse would take if money were not the limit. A real buy is the units
purchased. Those five figures are not read by the night prices.
[`MarketGood::stock`] stays yesterday's survivors, so it is not the
initial offering. A pop that cannot pay still bids one unit of each good in
the desire the walk stops on, and one unit of each currency when that pop
did not already bid it. A later desire is not bid. A currency surplus
does not move the card when no other tender was offered for at least one
unit, and that one-unit bid does not count as the quote that would move
it. Rot, process use, a print, flow, and an unmet buy
do not move a good that did not trade. Nothing moves that AMV unless the
good exchanged at least one unit. A book nobody could buy still falls when
nothing traded, a real bid was left, and the night's step is a fall. The
unpaid one-unit want is not that bid, so a surplus against only that want
keeps its price. An empty offer with a real bid buyers could pay rises
toward a tenth above the least efficient process that just pays. The first
night closes a quarter of that gap. A later night adds the whole increase
in that rest, then a quarter of what remains. The rise does not cross the
rest and does not pull a higher price down. A traded good does not take
that rise. No such process leaves the idle
step, which still stops one idle step above the cheapest process that just
pays. A surplus fall stops there too. A price already on that ask does not
cross it. A purchase bid larger than the offer
raises it once that unit has traded, by that idle fraction as an amount of
AMV. An open buy adds a flat step. Neither grows with the price, so a
currency and the good it buys cannot lift each other without bound. A
surplus still takes its fraction off the price. A two-sided book that traded, and
that nobody could buy for one unit, falls by twice the idle fraction of
its price. A traded book that
would have held falls by the full idle step. A currency book with a
purchase bid on both sides does move when it traded, because other goods
can pay it. The price also moves once another tender is for sale and this
good traded. Each good keeps the AMV its own day wrote. Units of a currency paid for other goods sit on both
sides of that currency's idle step, so the spend shrinks the move and
does not open one. A short bid the morning time grant already covers does not raise the
card. Output made after the book, including a good that was eaten, saved,
or never offered, does not cover that bid. An output that did not
clear does not price its inputs. A rise in AMV that was not paid
restores salability toward par by that gain, capped at one percent.

A line pays when its trade score is positive. A specialty also pays when
an offered output sold at least one unit. Subsistence drops that mark when
a specialty pays. When no line pays, each line that can run still takes an
equal share of the spare time and the spare points, on top of the runs a
short desire already holds, and buys the inputs that share is short. A
sought bid does not take that share back. A job
that still has a paying line buys in list order. The morning stores that
list upstream first, then by the higher trade score inside a band. The
first line sees the spare time and the spare points. Each later line
buys what that line left. Time on hand is the time still in the pile
when that line buys. A
currency input the pile is short is shopped with other goods, even when
another tender was offered. When that tender was offered and the purse
covers the card, the purse pays it instead, and a higher rate funds more
of the ceiling from that purse. The goods or the tender that pay stay off
the sell book. A currency output is not treated as an unsold product. A positive score that
sold nothing still takes the day. Scaling a partial sale down to the sold
share was tried and taken out. It cut the pops that were selling, and a
pile that sold nothing kept the whole day. A short desire holds time for the line that fills it best before a higher
score takes the rest, including when that line also pays. Basic desires
are first. When no specialty pays, common and luxury desires take what
that cover leaves, and the hold stops when the time is gone. A specialty
that pays keeps the day after the basic cover. A later desire that uses
a good already in that cover stays there. The hold stands whether
or not the pop has a coin. A specialty covering a subsistence good
spends that subsistence complexity on the desire-cover runs. Further
runs spend its own. A specialty that makes something else is shrunk
before a subsistence cover is. A gap subsistence can fill with a
different good stays on that subsistence line. A later desire no
subsistence process makes spends the subsistence floor, ahead of a later
subsistence tier, and it includes what an earlier desire will eat of that
good, including a pile of a worse target that this good replaces, and a meal of that target already set aside. The
water that desire drinks is not that recipe's water. Produce leaves that drink unspent, and the line that makes it runs those units before a later specialty keeps the points, so a later line spends only the surplus of the water just made. A later specialty
that does not make a basic good waits behind that subsistence tier. That recipe is the new pop's line, and the founder does not add it to its own job. A line with no stored target writes the aim the same morning. A stored target closes a quarter of the gap toward a higher aim, so one morning does not jump to it. It does not step down because an input is short. It steps down when the score is not positive, or when a product was offered and a full unit did not sell and a buyer could have paid, and then only a quarter of the way toward the cover plus the sold share of the surplus. A cleared book on a line another line feeds, whose free pile is more than that sale grown by the same quarter, steps the same way, and that share is the sale over the sale plus the pile. A currency output is left out of that sale. `turn_length` divides population growth and does not change that step. Yesterday's target is scaled by today's household count before the step. A doubled pop doubles the aim because today's time and today's desires already follow the new size.
Produce spends that cover rate on the
cover runs and leaves those points for a later line. A paying line that
another line feeds also keeps the points for the runs above that cover,
and the feeder still spends the points those input runs take. A line that feeds
another runs first. A line already running adds runs for an
output still sought yesterday, by a quarter of the runs it already has.
A subsistence line at zero still starts that bid. The time comes from what
is still free, and from a quarter of the subsistence runs above the desire
hold, so a shift between large lines moves more runs than the same shift
between small ones. A specialty keeps its day. A
currency the holder does not make lists
the producer share when that good was still sought. A line that stays at zero for one
turn, with no positive score, no sale, and no reserved run, is dropped.
The last line stays.

`turn_length` is 4. Infant and maternal mortality stay fractions of the
births in the turn. Subsistence farm, water, forage, and cotton are
processes 29–32, complexity 0.25, 12 time. Farm, water, and forage make
3. Cotton makes 6. A further luxury level leaves the goods still on hand
after produce, so spare cotton can sit in the next meal. Migration pace is 0.1. One
calm point of living is a twentieth of the households before size. The offer
and the room each multiply that share by log2 of the people, and each stops
at the whole pop. Anger weighs
half beside fear. Wealth is per person. Profit is the best running trade
score, counted once per pop: a worse job pushes and a better job pulls,
squared like living. A different craft closes above 1.10 times the origin's
wealth per person. Half of the moving goods travel. A move that would leave fewer
than one household takes the whole pop, and that pop is removed. A pop
with no line running and no whole currency coin, in working stock or
saved, offers every household. Pace, destination room, and the wealth
gate do not cap that offer. It still will not cross species. A
demographic gap still chooses who receives the whole count. Another pop
in the same state is not a destination. With nowhere to sit, the paced
offer stands. A dead pop that cannot leave, including the last pop,
switches to every subsistence process it can run from time and the goods
it already holds. A pop already on that set switches to one
non-subsistence process it can run the same way. The search starts above
the highest id it runs now, then wraps. Make time is not chosen. The
switch does not remove households.

The tester opens as one subsistence pop. The earlier eleven-pop file is
no longer the opening. A founded pop's growth cell prints `none`, because
it was not stored that morning. Its migration cell is the households it
received.

After internal migration, `Market::found_pop` may add one pop. The founder
is the pop with the most days of set-aside, then the most households,
then the higher finite standard of living, then the lower id. A pop with
no edit is skipped, and the next pop in that order is asked. They need
at least twenty-five households. They give five. The gift
is recorded on `migration`.

Working goods travel as those five households' share of working quantity.
The parent keeps the same working goods per household. The set-aside gift
is `founding_goods_weight` days per leaving household (default 3), capped
at the whole set-aside. That gift is currency savings, saved goods past
the meal and the input hold, meal units above one day of desires, and
working currency above the night savings claim. It
is added to the child's working quantity. A pop of twenty-five households
therefore sends a fifth of its working stock and three fifths of one
pop-day of set-aside. The parent may split once `founding_wealth` days of
set-aside are on hand (default 1.5). A parent of twenty-five households
sitting exactly on that floor gives 0.6 pop-days and keeps about 1.125
days of desires for the twenty households that stay. When the wait is open
and no pop clears that gate, a founding can still leave. The chance is 1%
on the first open day and rises by 1% each open day after that, stopping
at certainty. The same five households leave, and they still take
`founding_goods_weight` days of set-aside each. A pop that cannot give
five and keep one household does not leave. Migration is unchanged:
half of the moving share travels, and saved goods stay behind.

The founding waits until that stake can run the edited job `founding_runs`
times (default 3). The stake is the goods that would travel, counted
before they move. Time is the morning grant and is not required on hand.
A consumed or destroyed input must cover that many runs. Capital must be
present once. The stake must also hold at least that many units in total,
so a time-only process can be founded from any goods. Make time is not
added. A drop is allowed only when a loaded process that remains can run
that many times. Unloaded ids left on the job do not count. The last
process is not dropped.

`founding_useful_share` (default 0.8) of foundings draw from the useful
pool. That pool adds a loaded process whose trade score is finite and
positive, or drops a process whose score is finite and not positive. An
add weighs 1, doubled when a variable
output is a good no pop on this market already makes, and doubled again
when that good is asked for. The ask is the book's request, and a desire
target with a request under one counts as one. That ask then multiplies
the weight by one plus its natural log. The founder's lowest unmet desire
quadruples an add that makes one of its targets, including a target this
market already makes. The roll can still choose a lighter process. A
process whose trade score is not positive joins that pool only when it
makes that desire and the stake can run it. A fixed output and time
do not count as that good. A drop weighs 1, or half again when a variable product was offered for at least one unit and that offer covered the request. The other foundings add the
process for an open bid that is not already in the useful pool, when the
stake covers it. An open bid is still requested at least one and offered
under one. An empty pool falls through to the other. One edit is tried.
One founding in five also takes the other edit: an add drops one
unprofitable line that is not the process just added, and a drop adds
one process the stake can run. Adding a specialty drops each
subsistence process that makes the same variable good. That drop is
part of the add. A
desire no line can fill keeps extra runs on the lines that make its
inputs, enough that the traveling share can start that recipe. A desire no line can make also adds runs on a line that can sell, enough that the extra output pays for buying it, and the inputs those runs spend stay on the lines that make them. A set
the founder already runs founds nobody.

An open craft with that exact process set supplies the craft id. Otherwise
the subsistence craft supplies it when its processes are contained in the
set. Another pop with that demographic and that set receives the household
instead. No new id is taken. A pop that later has that demographic and
that set joins the earlier one the same day. The goods it holds go with
it. When the new job has no loaded process that
runs on time and the goods that household carries, every subsistence
process that does run on that stock is added. A new line starts at an
open target. A line already on the joined pop keeps its target. The new
pop copies the founder's species, culture, stratum, and religion, and has
no population-cap share.
Death is still the under-one prune. The new id is one higher than any
pop id still stored. A dead id is not reused. Technology stays empty.

`cargo test --lib` still fails
`load_from_path_reads_world_dir_goods_and_processes`. The loader rejects a
non-subsistence time input above 1. The panic names whichever of those
processes the map yields first.

Left out of the next pass: writing prices from the job, seeding one run of
a line that has never sold, reusing a pop id after the under-one prune,
retuning migration, `turn_length`, recipes, rot, or sentiment, and putting
firms on this ladder. The pop tester still
shares one ceiling. Each pop's starting size times a multiplier (default
1000) is its share. A founded pop is outside that ceiling. After the days
on the command line, it asks how many more to run.

## Landed vs stub

| Piece | Status |
|-------|--------|
| `Pop::satisfy` | Reserves free stock and records satisfaction. Starts at the first basic desire. A target that can be taken in full comes first. Stops if the chosen target cannot. Returns a copy of that desire, or `None` |
| `Pop::satisfy_continue` | Walks forward from the bookmark `satisfy` wrote. No bookmark: same as `satisfy` |
| `Pop::satisfy_tier` | One tier, from its first desire. Does not move the bookmark |
| `Pop::consume` / `consume_tier` / `consume_one_desire` | Called from `Market::market_day` after produce and the input-hold release, before rest. `PlayState::phase_pop_consumption` is still `todo` |
| Savings between tiers, and between luxury iterations | Written. Basic and common place that day on a good the desire eats. Luxury places it on the highest `salability * durability * fill` good. Withheld from selling and payment. The job does not spend that pile as an input. Basic, common, and the first luxury level may eat it. A further luxury level leaves it. See Savings |
| `Market::match_deals` | Still in the tree. The live day does not call it. See Match |
| `Market::market_day` | One market, in order: reset, meal and `plan_purchases`, `buy_ladder`, release the input hold, produce, consume, rest, sentiment, growth, `rebalance_population_cap`, prune under one household, internal migration, `found_pop`, `fold_duplicate_pops`, decay, `rescale_desires`, record keeping, gather use, then the night card. Yesterday's mass offer and exchange stay on the history the plan reads. Sentiment is before growth. Rescale is after decay and after migration and founding. A pop with no share is left out of the ceiling. The morning does not call `apply_craft`. Firm produce, reserve, and plan are empty. Institutions keep the empty `DealMaker` defaults |
| Cottage job | On the pop's stock. `plan_purchases` writes the morning runs and the input buy. The pop posts the sell book. The job does not set a price. See Where this stopped. The Cottage work section below is the earlier night plan |
| `Pop::plan` | Still in the tree. `market_day` does not call it. The morning plan is `Job::plan_purchases`. `Firm::plan` is empty |
| Stratum | Replaces class. A culture's economic subgroup. Id `0` is empty. Desires, rate addends, and the work-time addend stack with species, culture, and religion. The culture lists the stratum ids that derive from it. Stratification content is 0.3 |
| `Desire.decay` | Morning reset multiplies each desire's satisfaction by it. `0.0` clears satisfaction |
| Sentiment | Updated before growth. Desire sentiment arms and stored sentiment effects are collected first, and those stored arms are dropped. Unmet basic needs then move anger and fear. Common satisfaction, including a share above one full level, moves contentment. Luxury levels, counted again for each time they are met, move hope and happiness. Bonus satisfaction counts on the tier that holds it. Wealth per household, priced on the day's board and excluding time, moves hope, or fear when it is negative. Living above the average of the last five days then moves hope and happiness, and living below it moves anger and fear, at a larger rate than those flats. The first day has no average. Each other emotion moves 15 percent of its stored share into contentment in that same batch, and a larger stored share moves more. Growth scales births per woman by the stacked schedule on species, culture, stratum, and religion. The human default is +25% at full hope, +10% at full happiness, −30% at full fear, and −10% at full anger. Contentment does nothing. The scale uses the sentiment this day just wrote, so today's mood scales this year's births. Fear and anger also scale the migration offer, and hope scales the pull. Tester desires carry no sentiment arms |
| Internal migration | After the cap and the under-one prune, before founding and decay. Households move between pops on this market. The offer is the squared living gap, the squared relative wealth shortfall per person, and the squared profit gap, times fear and half of anger, then saturated by pace 0.1. One calm point of living is a twentieth of the households before size. The offer and the room each multiply that share by log2 of the people and stop at the whole pop. Wealth is per person and the average weights people. Profit is the best running trade score, and the average counts each pop once. A living standard or a profit above that average pulls, squared, times hope. Demographic distance and wealth gate the destination. The distance cap averages the two cultures' assimilation with the two religions' conversion. A different species does not move. Stratum has no such ease. A different craft closes above 1.10 times the origin's wealth per person. Half of the moving share's goods travel. A move that would leave fewer than one household takes the whole pop, and that pop is removed. The per-pop migration methods stay `todo` |
| Pop founding | After migration, before decay. One pop a day. See Where this stopped |
| Skills | Goods, priced and tradeable. A job lists the skill goods its processes take. Day start raises each to 0.1 per household. Each process takes that skill as an optional input. Decay is 0.05. Skill that was used comes back fresh, so a fully used pile does not decay that night. Skills are not listed on the mass book |

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
least one. When a higher-efficiency target in that bucket has a strictly
lower AMV per point of satisfaction than a good already held, the level's
buy moves to that target. The units are one level of it, or the luxury
levels already reached, minus units of it already on hand. The other
targets of that desire come off the buy. The held good stays in the bucket.
A desire the morning did not stop on posts the same level when it holds
the worse good. A substitute that costs as much or more per point of
satisfaction is not bought. The saved day's shortfall is added on top.

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
- `Pop::rescale_desires` runs in `market_day` after migration and goods decay,
  and before planning. It rewrites amount, additive effects, and recorded satisfaction
  from the new size. `update_desires` does not. A desire with no stored
  demographic source is left alone.
- `PlayState::phase_pop_growth` only grows. It does not fit the ceiling
  or rescale. A turn that also runs `market_day` grows those pops twice.

## Cottage work

The live morning plan is `Job::plan_purchases`, called from `Market::market_day` before the ladder. The notes under this heading are the earlier night plan.

A pop's `Job` (`src/game/job.rs`) runs on that pop's property. It does not
set a price. The pop posts the sell book. `Pop::sell_orders` offers free whole units
above what still feeds a desire. See Proposal and evaluate.

Craft `0` is no baseline. Lines still run. An empty line list skips plan,
reserve, produce, and job buys. Two pops of the same craft keep separate
lines, targets, and stock.

`data/world/crafts.toml` is the open list (`origin` none). A culture-origin
craft, then a religion-origin craft, drops and appends process ids and
multiplies `complexity_modifier`. An omitted modifier is `1.0`. `Pop::apply_craft`
adds processes the job does not already run, as resting lines (`Some(0.0)`),
when a caller attaches that craft. A line already present keeps its target.
The morning reserve does not call it. A process left off the job stays off.

`JobLine.target` is the quota. `None` runs as far as inputs on hand allow
and does not shop. `Some(0.0)` skips the line. `Some(n)` with `n > 0` runs
up to `n` and shops enough of each spent input that another `n` is on hand after decay, rounded up to a whole unit. Required inputs are always drawn.
A missing required factor shops one unit and leaves the other inputs free.

`JobLine.inputs` is the on/off list. An optional factor is claimed when
held and is not on the list. Any other optional, including capital, is
drawn only when the line lists it. A skill on that list is claimed and is
not shopped. An empty list draws none of those. The first morning starts
empty, because the list is written at night.

`Job::plan` rewrites the list. Each optional is scored on its own, against
the same one-iteration run with no optionals. Capital is costed at its
decay. A destroyed or consumed optional is costed at its holding. The
product includes that optional's output bonus on ordinary outputs, and an
extra output as its own units. A fixed output stays at its written amount.
When another line on the job consumes the product as a required input, the
product is priced at the better of holding it and that later line's
unboosted output holding per input unit. One step. The best later line
wins. The optional stays on when the rate and the product both hold or
rise. A lower rate stays on when the bare run already earns a positive
return and the product's relative gain is at least the rate's relative
drop. A product that does not rise turns it off. The list does not change
the buy. A listed input is still topped up for the line's quota.

The pop tester owns two goods. The householder common desire eats wood at
efficiency 1 and owns wood tools at efficiency 2. The hearth luxury desire
eats jewelry at efficiency 1, owns cabins at efficiency 2, and takes time
at efficiency 1 with cap 0.5. `use = true` on the demographic row is
`DesireTargetType::Use`. Satisfy reserves that pile before the job can
claim it as capital. Only units above the reserve and the saved day equip
a run. Some tester processes take wood tools as optional capital, one per
run, with an output effect, and the skill input carries an output effect
beside its extra output. Those amounts are test data.

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
make adds nothing. A saved day this job can make joins that gap when the
line that takes the good has a positive holding score. A line at or below
zero keeps the meal and leaves the saved day off the quota. When that
line is a worse process than one the world knows, and it costs more per
unit than that good's positive holding, the buy posted is that day's meal,
including after the bin already holds it. The line keeps making the gap
until those units are in quantity. A
purchase on hand shrinks the next night's gap, and the line then steps
down. A line already running steps toward one run. A line at 0 stays at
0. The best known recipe keeps the meal. When that holding rises above
the make cost, the line is no longer buying the good. A holding at or
below zero keeps the meal on the line.

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
more to make than the good's positive holding adds none of that shortfall.
The best known recipe still takes it. The cover and those runs stop at
the iterations this morning can finish. Stock on hand counts, including
the morning time grant, and so does output an earlier line will make
after the time it holds for a later higher-yield line it feeds. The cover plus the
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
whole and steps the surplus above the old cover. Sought runs then step
from that whole line. A paying line whose stored cover is still 0 steps
its current target toward the new cover plus the surplus and those runs.

`Job::complexity_cost` multiplies `complexity_modifier` by
`pop.extra_process_accel` (default `1.2`) for each process outside the
craft. Each process id counts once. A missing baseline process adds half
of `pop.craft_distance` (default `0.1`) and does not accelerate. No base
craft makes `Pop::complexity_cost` return `1.0`.

`Job::plan` then stores `plan_cost`. Each positive target adds
`modifier * (complexity^2 - overlap) * iterations`. Overlap is the
fraction of that line's input and output goods that another line also
uses. Iterations stay on the desire gap.

Job buys are the shopping list, rounded up to a whole unit, added onto an
open desire buy for the same good. The list is the next run's input need
after decay, for goods that survive the night. A good that decays completely,
such as time, is not bought. A non-factor optional is shopped only when
the line lists it. A skill is not shopped. Morning reserve writes that list before produce.
After produce, and after the second satisfy, `settle_shopping` rebuilds
that list from stock still free. An input this morning already made is not bought again.
Output an earlier line will make tomorrow morning covers the later line's
need before a buy is posted. Capital
set aside in `used` still counts, because it returns at decay. A line whose
last plan did not pay also shops one unit of each output that survives the
night: `1 / (1 - decay)` minus the
units already on hand. A larger input buy for that good stays. The first
morning, before a plan, does not shop an output. A purchase shrinks that
list. The next morning's reset drops that shopping list and the claim book. Targets and the optional-input list stay.

## Match

`Market::market_day` does not call `match_deals`. The live buy is `Market::buy_ladder` in `src/game/ladder.rs`. The notes under this heading are the earlier one-to-one pass.

Exchange, production, and consumption stay separate phases.

`Market::match_deals` builds the buy list, the sell list, and each actor's
requests once. It then keeps one opening per buy that can meet a sell.
It picks an opening at random, and a seller from that opening's matches
in proportion to the whole units that seller offers. An offer under one
unit has no weight. When every offer has no weight, that draw is uniform.
The buyer draw does not grow with the size of the buy. They meet because
one sought good is one offered good.
A pop who cannot propose one unit against that seller is not shown a
book. That buy stays posted, and the pair is left unmarked, so a later
meeting in the same call can still match them once the buyer's stock
covers the unit. A buyer who can is shown the seller's offers and
requests already stored on the book, and either proposes a basket or
abandons.
The seller accepts or rejects. Accept moves every good in the basket.
The basket is one map: positive units come to the buyer, negative units
leave. Freight is not a second list. Transport the buyer receives is in
that map, and after the goods move the buyer spends transport they then
hold until the bill is covered. If stock plus that purchase cannot cover
it, the buyer abandons before a proposal exists.
Reject and abandon move nothing.

After every meeting both sides `reevaluate`. A pop with a satisfaction
bookmark runs `satisfy_continue`, so goods just received get reserved.
Only the buyer and the seller then rewrite their buy and sell orders
from the free stock that remains. A spent offer shrinks or disappears,
and a surplus left by the trade is offered. Openings that name those
two actors are patched. Every other actor's orders stay as posted.
A rejected or abandoned pair is not retried in that
call. `match_deals` returns every `Meeting`. Only `Accepted` moves goods.

## Proposal and evaluate

Meeting order is unchanged. `Pop::propose` and `Pop::evaluate` judge a pop's
own ends. A firm still uses `seller_can_accept` only: match good present,
stock on hand, and absolute AMV of payment covering absolute AMV given.
A firm sell is stamped with the card's raw AMV.

`Pop::propose` (`src/game/pop.rs`) builds one `ProposedDeal`:

- `goods` is the buyer's change. Positive units come from the seller.
  Negative units go to the seller. A good is only on one side.
- The positive match good starts at `min(units still needed, seller's listed offer)`,
  whole units. A buy whose posted raw price is below the seller's posted
  raw price does not meet. A stored price of `0` means the card's raw AMV.
  Salability does not scale either price. A basket that cannot be paid
  steps down to the largest whole quantity that can. Below 1, the buyer abandons.
- Payment is sized in raw AMV. Each payment good counts at the card's raw
  price. The whole unit that crosses the seller's price is the last unit.
  The buyer offers that basket when satisfaction gained exceeds satisfaction
  given up, or when both are zero and the raw price is met, or when the only
  miss is that last whole unit paying more raw AMV than it received.
  The seller does not allow that overshoot. The seller needs the satisfaction
  gain, or raw payment at least equal to the raw cost.
- Sell orders offer the free units above one level of a desire and above the savings pile,
  at the card's raw AMV. A later tier of the same good does not reserve another level.
  A meal already set aside counts as that level. Buy orders are stamped with that same raw AMV.
  Payment offers that surplus, and also units that feed only a later
  tier than the good being bought. Same-tier goods are not spent on each
  other. A purchase that feeds nothing spends the surplus alone. That room is the target's cap, `amount * cap`, minus what that
  target already reserved toward the level (`target_taken`), and no more
  than the gap to one level, `amount - satisfaction`.
  An input the job consumes to make a desired output counts as that desire,
  except a good that decays completely. Units already reserved are not free.
  The market does not read every actor's orders again after a meeting.
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
- Satisfaction is one total, across every tier. Satisfaction gained must
  strictly exceed satisfaction given up. That accepts the trade even when
  raw AMV falls. A satisfaction loss is not saved by an AMV gain.
- When both sides have zero satisfaction, raw AMV received must be at least
  raw AMV given. Equality meets the price. Salability does not scale it.

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
After internal migration and founding, `market_day` calls
`note_decay(good, decayed, volume)` for each pop and firm. A good missing
from the card is inserted at AMV `1` and salability `0.5`.

`Market::record_keeping` is the night write. A good keeps the AMV it opened
with unless it exchanged at least one unit. The steps below are that write:

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
  `(decayed / base) / 4` clamped to `0..=1`. A currency with no other
  tender offered keeps its AMV, so that rot is not taken off the card.
- If AMV fell, salability falls by that loss over the old absolute AMV,
  capped at `0.01`. A night that took the good in payment and did not lose
  AMV raises salability by `0.05`, including past par. A good that was
  bought and not paid out rises by the same step and stops at par. A rise
  that was not paid restores salability toward par by that gain over the
  old absolute AMV, capped at `0.01`. A good that was only offered, and
  did not sell, does not rise that way.
- The tape is cleared. Production and consumption are zeroed. Stock stays.
- Each good keeps the AMV its own day wrote. A price that landed in the
  dead zone is lifted unless a seller valued that good below zero.
  Salability is not written again.

Actor `record_keeping` and `plan` read a snapshot taken after decay and
`rescale_desires`, and before that night write, so they still see today's
card. Exchange priced itself from the snapshot at the start of
`match_deals`. Firm record keeping clears that firm's production counters.
`Pop::plan` rewrites job targets and each line's optional inputs from that
snapshot and from each row's `produced` and `lost`. `Firm::reserve_for_day`,
`Firm::produce`, and `Firm::plan` are empty.

`PlayState::phase_pop_growth` only grows. It does not fit the ceiling or rescale.
`phase_pop_consumption` is still `todo`. Institution decay and institution
record keeping still panic and are not called.

**Code:** `src/game/pop.rs` (`propose`, `evaluate`, `apply_craft`, `plan`),
`src/game/job.rs`, `src/game/craft.rs`, `src/game/deal.rs`
(`Meeting`, `seller_can_accept`, `freight_shortfall`, `DealMaker` day steps),
`src/game/market.rs` (`market_day`, `MarketGood`, `history`, `record_keeping`),
`examples/pop_tester/main.rs`, `examples/pop_tester/load.rs`,
`data/pop_tester/`, `data/world/crafts.toml`. Also
`src/game/desire.rs`, `src/game/pop_property.rs`.
