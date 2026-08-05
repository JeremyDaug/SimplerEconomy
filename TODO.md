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
  - [ ] Phase Player Actions
  - [ ] Phase Player Bonuses and Demographic Updates
  - [ ] Phase Intramarket Day
  - [ ] Phase Intermarket Day
  - [ ] Phase Production and Planning
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
