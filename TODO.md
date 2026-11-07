# TODO

Working list from the version cuts in `docs/Overview.md`, plus the placements chosen after that draft. A line here is a known hole or a chosen placement. It is not a new mechanic.

Pop and firm testers do not use `PlayState`. That type starts in 0.3. Do not pull later day phases back into the current tester.

Buildings and upkeep are goods, not a separate system. They show up in whatever cut needs them. They are not a milestone.

## 0.1.0 Pop Tester Alpha (current)

Focus: pops, jobs, goods, processes, and one market. The cut ends when there is no more work that can be done with only pops and jobs. `examples/pop_tester` already runs one day. Cottage work, satisfy, consume, `match_deals`, and the night card are in. `docs/handoff/current.md` is the status note.

The day stays on the tester and `Market::market_day`. Do not wire `PlayState` here.

Landed pop and job work:

- [x] Savings between tiers, then between luxury levels.
- [x] `Desire.decay`. Morning reset multiplies satisfaction by it (`0.0` clears).
- [x] Skills as goods: priced and tradeable. A job lists the skill goods its processes take. Day start raises each one to 0.1 per household. Each process takes that skill as an optional input. Decay is 0.05. Used skill comes back fresh. A deal offers at most one unit of a skill.
- [x] Emergent money selection. The night write restates the card in one unit.
- [x] Population Growth testing. With both capped and uncapped versions. The pop tester takes a cap multiplier after the day count (default 1000). Each pop's starting size times that multiplier is its share of one ceiling. After growth, if the total passes the sum, every capped pop is scaled by the same factor and desires match that size. After those days it asks for more: enter is one day, a number is that many, 0 quits.
- [x] Sentiments. The day applies desire and stored sentiment effects, then tier satisfaction, wealth per household, and the change in standard of living against its recent average. Each other emotion then moves 15 percent of its share into contentment. Growth scales births per woman by the stacked demographic sentiment schedule. Wider effects are still unused. Needed for Internal Migration.
- [x] Internal Migration. Households move between pops on one market after the population cap and before decay. The offer is the squared gap in standard of living, plus the squared relative wealth shortfall, scaled by fear and half of anger, then saturated and scaled by a fifth, so one calm point of living moves about a tenth of the households. A higher living standard pulls, scaled by hope. Demographic distance and wealth cap the destination. That cap averages culture assimilation with religion conversion. A different species does not move. Stratum has no ease of its own. A pop left under one household is removed. Half of the moving share's goods go with them, and the destination's age mix absorbs the arrivals. Not the full migration write-up.
- [x] New Pop generation. The tester starts as one subsistence pop. After migration, the pop with the largest savings founds one new pop. The child is that pop's processes with one process added or one removed. A pop that already runs a specialty drops a non-profitable subsistence process first. Otherwise an open bid, a good that was bid and not offered, adds the process that outputs it. With nothing to add, any non-profitable process can be dropped, subsistence first. Make time is not added. A second pop with the same demographic and the same process set is refused. Technology stays empty. The allow-list is the loaded processes. Dead pop ids are not reused.

Not this cut:

- Stored AMV movement is balancing. The night write already runs, including the zero dead-zone. Changing that movement is not a Pop Tester item.
- Species change. Households do not cross species on the internal gate. The rules that would allow it are not assimilation or conversion.
- Stratum desires. The market is too small for stratification. That is 0.3. The stratum type is already in the tree.
- `PlayState::advance_turn` and the phase stubs.
- A required `init` loader. Load simulation data as a tester needs it. `examples/pop_tester/load.rs` already does that. Game save and load is 0.3, finished in 0.4.

## 0.2.0 Firm Tester Alpha

Focus: firms, their logic and reasoning, and their trade. No `PlayState`.

Enough wages, owners, and firm strategy to scaffold and test. Not the full strategy set. Not company hierarchy (`parent`, `children`, `level` stay links only).

Shells already in tree: `Firm`, `Owners`, `Workforce` (hours, labor, wage basket, profit share; morning settlement is not written), `Contract` (empty), `FirmOrganization` (empty). `reserve_for_day`, `produce`, and `plan` are empty. A firm seller still only checks `seller_can_accept`.

n

## 0.3.0 Full Market Alpha

Focus: environmental effects, plots and land, terrain, dynamic friction, institutions, and a player / nation (AI). One market. This is the first cut that uses `PlayState`.

`Map`, `Region`, `Tile` (claims, occupier), `Plot` (terrain enum only), and `Institution` (kind, markets, firms, level, loyalty, passive effects) are already in the tree. Institution property, ability trees, and mandate AI are marked later on the type. `Players::decay_goods` and `Nation::record_keeping` are `todo`.

Land, as chosen: a tile is made of plots, and a plot is subdivided into units of land, which then get used. Finer detail is in the vault and may change when this cut is tested. Do not open the vault unless a note is named.

Dynamic friction, as chosen: a market-size scalar multiplies bulk transport cost by the size of the market, so a market does not grow for free. This cut is where a growing market can first be simulated, because plots and land arrive here.



## 0.4.0 Multi-Market Alpha

Focus: more than one market. Inter-market firms, institutions, and nations. Trade, travel, and units on the map.

`phase_inter_market_trade` is `todo`. Institution `markets` is already a list. `Unit` is an empty shell. Leave it empty until this cut. It stays thin until a client can show it.

- [ ] A two-market tester.
- [ ] Inter-market trade as concrete routes: specific goods, costs, and distance. Not a market above markets.
- [ ] Travel between markets.
- [ ] A thin `Unit`. Map actors, including military. Behavior stays thin. Do not build a full unit game here.
- [ ] Firms, institutions, and nations that act in more than one market.
- [ ] Migration finished, including inter-market moves.
- [ ] Game save and load finished.

## 0.5.0 Bevy Alpha and human players

The written cut is the first playable client. Graphics and UI for minimal playability. Simulation stays free of Bevy.

This cut may move back. Any steps added before it are functional testers and play interfaces, not a feature dump. Those steps are not named. Do not invent their numbers.

Units stay thin until that client exists, whenever it lands.

- [ ] Do not start a client until the intermediate cuts, if any, are chosen and written into `docs/Overview.md`.
- [ ] When it does start: a client crate, not this library. `advance_turn` wires the phases that exist by then. It does not invent the missing ones.

## 0.9.0 Beta

Focus: visuals done, replace placeholder art, write and balance the opening factuals, refine institutions and cultures.

- [ ] Opening factuals pass. Testers already need `data/world`. This cut is the balance pass, not the first data.
- [ ] Institution and culture refinement.
- [ ] Art replacement. No earlier cut owns first art.

## 1.0.0 Release

Focus: public ready. Balance and bugfix continue after.

- [ ] No feature list. Anything still open below has to land in an earlier cut, or be cut from 1.0.

## Still not placed

Do not schedule these by guessing.

- Conflict. Regions are claimed and fought over in the Ideas list. `Tile` has claims and an occupier. No cut names war.
- Special resources. `phase_extract_special_resources` is empty. Not in the overview.
- Company behavior above one workshop. Links exist. 0.2 does not include them.
- The full set of firm strategies. 0.2 is a scaffold.
- Named cuts between the current 0.5 and 0.9. The intent is to push Bevy back and insert tester and play-interface steps. The names are not chosen.
