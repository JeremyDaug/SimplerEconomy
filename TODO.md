# TODO

Working focus list. Prefer broad strokes; long-form design lives in the EconCiv vault.

---

## Major steps

- [ ] Complete `PlayState::advance_turn`
  - [ ] Day start phase
    - [x] Pop Day Start
      - Completed not Connected.
    - [ ] Market Day Start
    - [ ] Firm Day Start
  - [ ] Phase Environment Events
    - [ ] Roll/Fire Events
    - [ ] Apply Events
  - [ ] Phase Player Actions
    - [ ] Read thorugh Player actions and apply.
  - [ ] Phase Player Bonuses and Demographic Updates
    - [ ] Apply to Institutions
    - [ ] Apply to Regions/Markets
    - [ ] Apply to Firms
    - [ ] Apply to Pops
  - [ ] Phase Intramarket Day
    - [ ] Market Day
      - [ ] Gather Institution/Firm Orders
      - [ ] Gather Pop Orders
      - [ ] Prioritize orders, match buyers and sellers, and get them trading.
      - [ ] Be sure to record general results of trades after they are complete.
  - [ ] Phase Intermarket Day
    - [ ] Recalculate travel routes between markets.
      - [ ] Add new port tiles to markets.
      - [ ] Update Travel Routes (modify existing routes to better alterantives and make new ones).
    - [ ] Existing Trader movement. Including Collisions with hostile forces.
    - [ ] Any Shippers that arrive unload their goods at their destination market.
    - [ ] Process Market Goods information to get Surpluses, Shortages, High or Low Prices, and Depth of the good in the market.
    - [ ] Intermarket Firm phase. 
      -  Trade Firms seek out profit opportunities between markets, focusing on those they have trade houses in.
      -  Multi-Market Firms seek out arbitrage within their company they can take advantage of.
    - [ ] Create new Trader Units and move them one step to catch up with Existing Trader Movement.
  - [ ] Phase Production and Planning
    - [ ] Firms run their processes to produce goods based on yesterday's plans and today's shopping success.
    - [ ] Institutions set and modify their plans based on historical data.
      - [ ] Institutions pass down plans to the firms they own, modifying the firm's plans as well.
    - [ ] Firms take information from historical data as well as any directives or plans from players or instutions then create or modify their production plans to meet goals and projections.
  - [ ] Phase Pop Consumption
  - [ ] Phase Pop Growth
  - [ ] Phase Pop Migration
  - [ ] Phase Record Keeping
  - [ ] Phase Map Changes
  - [ ] Phase Good Decay

---

## Refactors and improvements

- [ ] Household / population change helpers  
  From [this conversation](https://grok.com/share/c2hhcmQtMw_e2b20412-fa4e-4d6e-ad1e-29cf133c819e): simpler household size edits, less hassle when defs change, addresses household total-pop jumps.
