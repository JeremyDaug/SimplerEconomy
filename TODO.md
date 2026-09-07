# TODO

Working focus list. Prefer broad strokes; long-form design lives in the EconCiv vault.

- [x] Load world-data goods (`data/world/goods.toml` -> `Factuals::load_from_path`; tester uses it).
- [x] Load world-data processes (`data/world/processes.toml`; tester loads the world folder).
- [x] Load world-data gameplay config (`data/world/config.toml` -> `GameConfig` on Factuals).
- [x] Tester CLI far enough (home / stock / orders / processes / day CSVs). Paused unless asked.
- [ ] Load remaining factuals (species, cultures, religions).
- [ ] Load initialization data (pops, firms, markets, starting prices).
- [ ] Save data (later).

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
    - [ ] Labor contracts fire (pay wages, move Time). `LaborSettlement::settle` exists; not wired into PlayState or the tester CLI.
    - [ ] Work hours cap from culture / class / religion / law (global `work_time_fraction` is the stand-in). Prefer caps so hours can still move underneath.
    - [ ] Market Day
      - [ ] Gather Player / State Orders (In priority buckets)
      - [x] Gather Firm Orders (`create_orders`; institutions not collected yet)
      - [x] Gather Pop Orders (`create_orders`; wealth-rank buy order priority)
      - [ ] Prioritize orders, match buyers and sellers, and get them trading.
        - [x] Order priority field, named slots, sell-weight compose.
        - [x] `Market::match_orders` (one success, multiple front-group failures).
        - [x] `Market::run_market_day` loop: collect, collate, match, deal, finalize, leftover orders.
        - [ ] Multimatch (later, not now): after the first pair, pull that buyer's other buys at similar priority against the same seller's other goods. One trip, one `ProposedDeal`. Variety sellers load the cart; do not mix other buyers or other sellers. `form_buy_proposal` still assumes one target.
        - [ ] Deal / settlement (move goods, AMV, `MarketGood` stats, update leftover orders).
          - [x] `DealMaker` trait, `ProposedDeal`, `buy` / `evaluate` (read-only).
          - [x] Multi-good buy tender: seller counter + high-sal `take_tenders`, low-sal last.
          - [x] Whole-unit offers and proposals (inventory may stay fractional).
          - [x] `Pop` / `Firm` `take_good` (return qty, drop the property row).
          - [x] `finalize` (inventory). Accept from both sides applies; reject washes (drop buy, keep sell).
          - [x] Tester `day` command (`Market::run_market_day` + `MarketDayReport`).
          - [ ] `sell` rewrite, PlayState wire.
        - [x] Set pop wealth ranks on receive (per-household total AMV).
        - [ ] Pop offer orders.
      - [x] Record deal results on `MarketGood` (requests/tender, purchased/payment/average price; volume is purchased + payment).
      - [x] Transport / friction: `TRANSACTION_COST + bulk * market.friction` in transport-tagged units. Wash pays the flat fee; unmatched is unavailable (no fee).
      - [ ] New orders after a fill (`next_shopping_trip`, firm re-emit / reserve toward stock target).
      - [x] AMV drift on accept/reject (live `MarketGood.amv`; frozen history for the day).
      - [x] Salability day-end from payment/tender.
      - [x] AMV history rings on `MarketGood` (opening seed + daily close; tester trail).
      - [ ] Other market clean-up (leftover book carry).
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
    - Partially made. `run_production` exists, not wired into PlayState. `Firm::plan` runs from tester `day` and PlayState record-keeping.
    - [ ] Firms run production (Institutions work through Firms).
    - [ ] Institutions set and modify their plans based on historical data.
      - [ ] Institutions pass down plans to the firms they own, modifying the firm's plans as well.
    - [x] Firms take information from historical data as well as any directives or plans from players or instutions then create or modify their production plans to meet goals and projections.
      - v0: `Firm::plan` restock-modulates line targets from the sell plan, then buy/sell/use/stock/reserve and own AMV. No institution/player directives yet.
      - Production stays before pop consume (wages/payouts from today's work vs only post-shopping stock). Planning is after consume.
      - [x] `Firm::plan` is gather then adjust. Quiet baseline does not move. Deviations nudge sell and/or own quote; production aligns to the sell plan, weighted by productivity. Grow only on strong sell success; profit is realized (sold vs cost).
      - [x] Gather: line productivity, realized profit, sell success, stockpile vs `output_cover`, market AMV. Share/volume/vol/trend when `MarketHistory` has `purchased` / `amv_trails`.
      - [x] `FirmRecords` + confidence (0 cautious .. 1 aggressive) scale lerp and grow/shrink steps.
      - [ ] Competitor quotes: pass other firms into plan; `GoodFacts.competitor_amv` is `None`.
      - [ ] Firm strategy: scale the price-vs-sell split (aggressive keep-out vs defensive quote). Comments in `good_plan_nudge` / `price_cut_share`.
      - [ ] Formula pass: volatility stockpile threshold, exact bands, total-profit objective.
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
    - [x] Firm Record Keeping
      - v0: rolling average + `Firm::plan`. Split snapshot vs plan when the planning phase is wired.
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
  - [x] Planning (v0 `Firm::plan` with records + confidence; tester `day` and PlayState record-keeping)
  - [ ] AMV quotes as own strategy beyond the current nudge; price vs volume on undersell; firm strategy later
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

- [ ] Function comments: what first, why second  
  Many existing `///` on small helpers describe context or the result elsewhere
  instead of the operation. Dedicated pass: lead with what the function does
  (returns, caps, sorts, looks up). Why is optional; the operation often
  explains itself. Deal/bound helpers were done; the rest of `src/game/` is
  still pending. Do not mix into unrelated work unless asked.
- [ ] Household / population change helpers  
  From [this conversation](https://grok.com/share/c2hhcmQtMw_e2b20412-fa4e-4d6e-ad1e-29cf133c819e): simpler household size edits, less hassle when defs change, addresses household total-pop jumps.
- [ ] Store market id on `Pop`  
  Pops do not wander except during migration. A `market_id` field (updated when they move) would replace the evening `pop_to_market` map. Defer until migration leaves write.
