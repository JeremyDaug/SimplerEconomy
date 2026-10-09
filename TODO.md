# TODO

Working list from the version cuts in `docs/Overview.md`, plus the placements chosen after that draft. A line here is a known hole or a chosen placement. It is not a new mechanic.

Pop and firm testers do not use `PlayState`. That type starts in 0.3. Do not pull later day phases back into the current tester.

Buildings and upkeep are goods, not a separate system. They show up in whatever cut needs them. They are not a milestone.

## 0.1.0 Pop Tester Alpha (current)

Focus: pops, jobs, goods, processes, and one market. The cut ends when there is no more work that can be done with only pops and jobs. `examples/pop_tester` already runs one day. Cottage work, satisfy, consume, `match_deals`, and the night card are in. `docs/handoff/pops.md` is the status note.

The day stays on the tester and `Market::market_day`. Do not wire `PlayState` here.

Still open, and still pop or job work:

- [ ] Close the consume trap in the handoff. A normal day counts a reserved level twice.
- [ ] Savings between tiers, then between luxury levels.
- [ ] `Desire.decay`. The field is unused. The morning reset still clears satisfaction.
- [ ] `ordered_targets`. Satisfy picks a bucket good at random.
- [ ] Skills as goods: priced, not traded like other goods. `Workforce.labor` is already keyed by skill or good id. No skill good exists yet.
- [ ] Time as a good with market status, not only the morning grant. The grant sits in the tester. Leave it there unless the tester day needs it in the library. Do not move it into `PlayState`.
- [ ] Emergent money selection, early, so it can be tested with pops. The night write already restates the card in one unit. Nothing marks a good as money. This piece may slip to a later cut. The selection test should not wait on that slip.
- [ ] Decide how stored AMV moves, aside from the zero dead-zone. The night write exists. The rule is still open.

Not this cut:

- Stratum desires. The market is too small for stratification. That is 0.3. The stratum type is already in the tree.
- `PlayState::advance_turn` and the phase stubs.
- A required `init` loader. Load simulation data as a tester needs it. `examples/pop_tester/load.rs` already does that. Game save and load is 0.3, finished in 0.4.

## 0.2.0 Firm Tester Alpha

Focus: firms, their logic and reasoning, and their trade. No `PlayState`.

Enough wages, owners, and firm strategy to scaffold and test. Not the full strategy set. Not company hierarchy (`parent`, `children`, `level` stay links only).

Shells already in tree: `Firm`, `Owners`, `Workforce` (hours, labor, wage basket, profit share; morning settlement is not written), `Contract` (empty), `FirmOrganization` (empty). `reserve_for_day`, `produce`, and `plan` are empty. A firm seller still only checks `seller_can_accept`.

- [ ] A firm tester, same shape as `pop_tester`: one market, firms that produce and trade, printed meetings.
- [ ] Firm reserve, produce, and plan.
- [ ] Firm propose / evaluate, not only the stock check.
- [ ] Wage settlement and owner profit, enough to test. Morning settlement is not written.
- [ ] One or a few firm strategies, as scaffolding. Not every strategy in the Ideas list.
- [ ] First parts of decentralized innovation, on firms. Not a beaker pile. `Technology.cost` still reads like one. Do not finish the tree here. States and institutions finish it in 0.3. Leave `TechTree` empty until this cut is the work.
- [ ] Sentiment, migration, and contracts may start. They stay secondary. They do not block the firm tester. Migration is not finished until 0.4.

## 0.3.0 Full Market Alpha

Focus: environmental effects, plots and land, terrain, dynamic friction, institutions, and a player / state (AI). One market. This is the first cut that uses `PlayState`.

`Map`, `Region`, `Tile` (claims, occupier), `Plot` (terrain enum only), and `Institution` (kind, markets, firms, level, loyalty, passive effects) are already in the tree. Institution property, ability trees, and mandate AI are marked later on the type. `Players::decay_goods` and `State::record_keeping` are `todo`.

Land, as chosen: a tile is made of plots, and a plot is subdivided into units of land, which then get used. Finer detail is in the vault and may change when this cut is tested. Do not open the vault unless a note is named.

Dynamic friction, as chosen: a market-size scalar multiplies bulk transport cost by the size of the market, so a market does not grow for free. This cut is where a growing market can first be simulated, because plots and land arrive here.

- [ ] `PlayState` becomes the day for this tester. Call the market day once. Do not also grow those pops inside `market_day`.
- [ ] Environment refresh and random effects.
- [ ] Plots on tiles, and land units on plots. The terrain enum is not that model.
- [ ] The market-size scalar on bulk transport cost.
- [ ] Stratum desires and stratification. The type is already in the tree. Not before this cut.
- [ ] Tech finished here, once states and institutions can direct it. Still not a beaker pile.
- [ ] Sentiment, migration, and contracts become meaningful. Migration is still not finished.
- [ ] Institution day that does not panic. The AI modifies institutions. It does not need the full mandate tree on day one.
- [ ] State AI for internal market management and for modifying institutions. `phase_player_actions` still says unit and map actions. That comment is ahead of this cut. Units are 0.4.
- [ ] Game save and load starts. It finishes in 0.4. Simulation data loading stays as-needed.

## 0.4.0 Multi-Market Alpha

Focus: more than one market. Inter-market firms, institutions, and states. Trade, travel, and units on the map.

`phase_inter_market_trade` is `todo`. Institution `markets` is already a list. `Unit` is an empty shell. Leave it empty until this cut. It stays thin until a client can show it.

- [ ] A two-market tester.
- [ ] Inter-market trade as concrete routes: specific goods, costs, and distance. Not a market above markets.
- [ ] Travel between markets.
- [ ] A thin `Unit`. Map actors, including military. Behavior stays thin. Do not build a full unit game here.
- [ ] Firms, institutions, and states that act in more than one market.
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
