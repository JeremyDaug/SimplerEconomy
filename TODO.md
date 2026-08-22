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
  - [x] Phase Player Bonuses and Demographic Updates
    - [x] Apply to Institutions
    - [ ] Apply to Regions/Markets
    - [ ] Apply to Firms
    - [x] Apply to Pops
  - [ ] Phase Intramarket Day
    - [ ] Market Day
      - [ ] Gather Player / State Orders (In priority buckets)
      - [ ] Gather Institution/Firm Orders
      - [ ] Gather Pop Orders
      - [ ] Prioritize orders, match buyers and sellers, and get them trading.
        - [x] Order priority field, named slots, sell-weight compose.
        - [x] `Market::match_orders` (one success, multiple front-group failures).
        - [ ] Deal / settlement (move goods, AMV, `MarketGood` stats, restamp).
        - [ ] Stamp pop wealth ranks on receive; pop offer orders; PlayState wire.
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
    - Partially made. run_production made, not wired in.
    - [ ] Firms run production (Institutions work through Firms).
    - [ ] Institutions set and modify their plans based on historical data.
      - [ ] Institutions pass down plans to the firms they own, modifying the firm's plans as well.
    - [ ] Firms take information from historical data as well as any directives or plans from players or instutions then create or modify their production plans to meet goals and projections.
  - [x] Phase Pop Consumption
  - [x] Phase Pop Growth
  - [ ] Phase Pop Migration
    - [ ] Calculate Per-pop Emigration pressure
    - [ ] Calculate Per-Firm Hiring Pressure
    - [ ] Calculate Market-region pressure (sum of Emigration and hiring pressure)
    - [ ] Do Organized / Mass Migrations
    - [ ] Market Internal Market Migration
    - [ ] Inter-market personal migration
  - [X] Phase Record Keeping - Wired, but not complete.
    - [ ] Market Record Keeping
    - [x] Pop Record Keeping
    - [ ] Firm Record Keeping
    - [ ] Institution Record Keeping
    - [ ] State Record Keeping
  - [ ] Phase Map Changes
    - [ ] Process Player Claims.
    - [ ] Move tiles into/out of regions/markets.
    - [ ] Military movement and combat
    - [ ] Complete any non-random environmental changes.
  - [x] Phase Good Decay - Wired, but not complete.
    - [ ] Decay Map Goods
    - [ ] Decay Actor Goods
    - [ ] Decay Player Goods
    - [ ] Decay Institution Goods ? (may not be necissary as their property is contained in their firms.)

## Structure Completions

- [ ] Institution
  - [ ] Ability Trees, features and abilities for institutions
  - [ ] Mandates, Requests/Demands to the government for their approval.
  - [ ] Loyalty System, Institution Mood and Loyalty to their parent Player.
- [ ] Class Demographics
  - [ ] Connect into Culture
  - [ ] Define how it modifies culture
  - [ ] Define how it selects members
  - [ ] Other Special features
  - [ ] Default/baseline Classes
    - [ ] Underclass/Poor
    - [ ] Middle Class
    - [ ] Upperclass/Rich/Aristocrats/etc
    - [ ] Priesthood/Monastics
- [ ] Firm
  - [ ] Planning
  - [ ] Management Logic
  - [ ] Internal Organization and structure
- [ ] The Graphics
  - [ ] All of it, just... all of it. (Backburner until most game logic is made as graphics are secondary)

## Balancing and Testing

- [ ] Sentiment Tuning
- [ ] Standard of Living Tuning
- [ ] Luxury consume leveling  
  Luxury currently loops until stock runs out (`Pop::consume`). Later: cap or pace extra luxury passes so one desire does not eat the whole leftover pile and so reserved/consume stay aligned across the luxury ladder. Separate from 'bads' / ejection.

---

## Refactors and improvements

- [ ] Household / population change helpers  
  From [this conversation](https://grok.com/share/c2hhcmQtMw_e2b20412-fa4e-4d6e-ad1e-29cf133c819e): simpler household size edits, less hassle when defs change, addresses household total-pop jumps.
- [ ] Store market id on `Pop`  
  Pops do not wander except during migration. A `market_id` field (updated when they move) would replace the evening `pop_to_market` map. Defer until migration leaves write.
