# TODO

The V1 build order from [Design_V1_Market_Game.md](Design_V1_Market_Game.md) (Build Order and Test Milestones). Each step passes its milestone before the next one starts. The design doc owns the rules; a line here is only the step and how to tell it's done.

## Before step 1

- [ ] Prune the Reckoning code to what V1 uses: pops and desires, households, sentiment, AMV pricing, firms, goods, processes and the loaders. V3 founding, migration, the map and the multi-market code leave the V1 branch. They stay in git history.

## Build order

1. [ ] **Ledger and invariant.** Track all coin and every flow. *Milestone:* a test that sums all coin daily passes over 1000 days.
2. [ ] **Pops and one firm with an owner pop, closed market.** No trade, no government, no migration. *Milestone:* total coin never changes. Owner payouts reach the owner pop. Prices settle over 1000 days with a seed.
3. [ ] **Labor.** Firms hire pops for wages. *Milestone:* wages are the main pop income. A test shows a labor shortage stops production when demand for workers exceeds pops.
4. [ ] **External Market, trade facility, longshoremen and traders.** *Milestone:* coin changes only by border flows. Local prices pushed up or down return to the band. External prices move about 1000 times slower than local ones under the same trade. Too few longshoremen caps trade by capacity. Foreign crew wages show up as border outflows.
5. [ ] **Coin decay and minting** as settings. *Milestone:* the invariant holds with each on and off.
6. [ ] **Rival firms with simple AI, buyouts and failures.** *Milestone:* 2 or 3 rivals over 1000 days don't all go bankrupt or all take over. The market doesn't collapse. A buyout and a failure both pay the owner pops, and no coin is lost. Idly wealthy owner pops spend down their savings.
7. [ ] **Simple firm founding.** *Milestone:* a market that loses a firm gets a new one within some number of days when demand is there. Over a long run, firms turn over and new industries appear.
8. [ ] **Government: treasury, taxes, tariffs, welfare, debt and printing.** *Milestone:* the debt equation holds every day. On normal difficulty debt-to-GDP stays bounded over 1000 days. A forced deficit raises prices, angers poor and fixed-income pops, and leaks some new coin out through imports. Interest goes to foreign holders as a border outflow. Runaway debt lets interest eat the budget and trigger the crash signal, and cutting the debt brings stability back.
9. [ ] **Abstract migration.** *Milestone:* better wages and mood pull pops in slowly, worse ones push them out, and migrant coin balances in the ledger.
10. [ ] **Player action layer.** Orders, pause, speed. *Milestone:* a scripted player can run a full game from the command line.
11. [ ] **Tech tree, research and spread.** *Milestone:* a firm that never researches still picks up most techs over a long run. A firm that focuses gets deep in one branch first. Research spending shows up as wages.
12. [ ] **Prestige, Influence, lobbying, diplomacy.** *Milestone:* lobbying moves tax rates and Patent law. Diplomacy moves a rival's personality. Lobbying money lands in the treasury or with government pops.
13. [ ] **Favor, stability, bad actors, game overs, soft endings.** *Milestone:* scripted bad actor runs trigger the right game over. An Influence-heavy run reaches capture, and a bad actor run reaches nationalization. A well-behaved run reaches no game over in N days.
14. [ ] **Market research and the pop sample.** *Milestone:* the player can only see what they've paid for.
15. [ ] **Difficulty presets and balance passes.**
16. [ ] **UI.** Headless first (run N days from a seed and print the ledger), then a terminal UI.

Open questions that a step needs are listed in the design doc's Open Questions (Summary). Answer or adopt the proposal before that step starts.
