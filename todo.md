# SimplerEconomy — TODO

Working checklist of what seems unfinished or next at this stage of the EconCiv
rework. Not a design document: for **names** use `docs/design-vocabulary.md`; for
**open review debt** use `reviewlog.md`; for **long-form intent** use the EconCiv
vault.

Add anything missing. Strike or check off items as they land.

**Last filled:** 2026-07-31 (from pipeline status, reviewlog, stubs, recent work)

---

## P0 — Day pipeline wiring (make the turn runnable and consistent)

- [ ] **Wire `process_satisfaction` into the turn** (after consume; settle order vs growth — see review B5).
  - Today: growth runs; mood/fill pass never does → stored sentiment/satisfaction leak to decay (B3).
- [ ] **Lock growth vs process_satisfaction order** and align docs (`pop.rs` header vs “keep growth arms”).
- [ ] **Wire `demographic_update` / player-bonus phase** (`phase_player_bonuses_and_demographic_updates` is still `todo!()`).
  - Institution effect push → demo flags → `Pop::demographic_update` → clear `household_changed`.
- [ ] **Firm / institution `decay_goods`**: replace `todo!()` with no-ops or real logic before any live firm/institution exists (B6 landmine under `Actors::decay_goods`).
- [ ] **Players / mapdata `decay_goods`**: implement or no-op; prefer `&mut self` (review nit 14).
- [ ] **Start-of-day phase**: time/env goods, market day resets, clear pop `reserved`, etc.
- [ ] **Intra-market day**: partition actors, labor → orders → matching loops, merge.
- [ ] **Inter-market trade**, **production/planning**, **map changes** stubs → real or explicit no-ops.

---

## P1 — Pop day completeness (logic exists, gaps remain)

### Already in good shape (for reference)
- Reservation + satisfaction decay (`initial_reservations_and_update_satisfaction`)
- Consume / satisfy with `ordered_targets`
- Growth phase + stored Birthrate/Mortality drain
- `process_satisfaction` draft (tier fill boosts, sentiment baseline, sentiment effects)
- Pop `decay_goods` + desire/stored BonusGood
- Sentiment type + modifiers
- Design vocabulary + satisfaction-fill proposal

### Still to do
- [ ] **Call reservation + desire resize in the right morning phase** (with demographic_update / start-of-day).
- [ ] **`next_shopping_trip` / buy-loop integration** with markets (create_orders exists; day loop does not).
- [ ] **Migration**: `calculate_migratory_pressure`, internal migration, organized/inter-market (mapdata stubs).
- [ ] **Record keeping** for pops (and firms/markets/institutions/states) — still `todo!()`.
- [ ] **Sentiment → migration / unrest / politics** once process_satisfaction is on the clock.
- [ ] **Household rebuild**: conserve total members when demo household size changes (or phase transitions); use or drop `alter_household_maintain_members`.
- [ ] **`DemographicEffect` → household modifiers** (effects stored but never baked into `*_household_modifiers`).
- [ ] **`update_desires` property scaling** idempotency / Inf guard (`previous_growth`).
- [ ] **Growth common/luxury terms**: confirm sign/intent (penalize fill vs lack) and document (review #3).
- [ ] **Fill-boost docs** on `DesireEffect` / `PopEffect::Satisfaction` match vocabulary (fill boost, no common hard cap).
- [ ] **Baseline sentiment daily pulse**: dampen or return-to-content before trusting sentiment long-term (review #9).
- [ ] **Invalid Satisfaction tier** in release: keep or log instead of silent drop (review #10).

---

## P2 — Markets, firms, production

- [ ] Market day matching (orders, prices, AMV history updates).
- [ ] Firm production planning after trade; apply process results + hire pressure.
- [ ] Institution market slot ordering + controlled-firm direction (no dual ownership).
- [ ] Process spillover → `PopEffect` / stored fill boosts (`output / pop` conversion).
- [ ] Class demographics / desires (`factuals` Class `todo!()`).

---

## P3 — Institutions / state (beyond v0 skeleton)

- [ ] Institution property / contracts when market day needs them.
- [ ] Ability trees / levels beyond flat `level`.
- [ ] Mandates + loyalty scoring.
- [ ] Passive household effects into demographic rebuild path.
- [ ] State registry of controlled institutions (optional).
- [ ] Factuals institution trees (optional early).

See `docs/proposals/institution-draft.md`.

---

## P4 — Design follow-through (documented, not fully built)

From `docs/proposals/satisfaction-ratio-and-boosts.md` + vocabulary:

- [ ] **Unit ledger** alongside tier fill (`recorded_tier_units` or goods used) for wealth / throughput.
- [ ] Ascetic vs affluent metrics (high fill + low units vs high units + middling fill).
- [ ] Retune common fill surplus mood curve (half-above-1 is a draft).
- [ ] Firm/process path that emits **stored fill boosts** as shared comfort/spiritual output.
- [ ] Whether growth-phase common/luxury terms should ever see **boosted** tier fill.
- [ ] Rename code identifiers toward vocabulary when convenient (`tiers_satisfied` → desire fill language in docs first; code rename optional).

---

## P5 — Code quality / hygiene

- [ ] Religion/species field names still say “culture” in places (review nit 11).
- [ ] Orphaned DesireEffect docs in `desire.rs` (nit 15).
- [ ] Unreachable code after Class `todo!` in factuals (nit 13).
- [ ] Growth test comment parentheses (nit 12).
- [ ] Broader tests: growth edge cases, Birthrate malus / Mortality bonus, demographic rebuild edges, turn-loop integration once phases wire.
- [ ] `growth_phase` unused `factuals` param — use or drop.
- [ ] Keep `reviewlog.md` pruned when reviews land.

---

## Suggested near-term sequence

1. Safe stubs for firm/institution decay (stop turn panics).  
2. Wire `process_satisfaction` + lock order vs growth.  
3. Wire demographic_update / player-bonus phase.  
4. Start-of-day + reservation path.  
5. Thin market day slice (enough to buy/consume a loop).  
6. Unit ledger + migration pressure using sentiment/tier fill.

---

## Done recently (do not re-open without cause)

- Pop decay goods; Actors fan-out decay  
- Sentiment + mods; Pop.sentiment  
- process_satisfaction first draft (tier fill boosts, common surplus mood weight)  
- Stored growth effects in growth_phase  
- Design vocabulary + satisfaction-fill proposal  
- Institution v0 skeleton + market membership  
- Reservation/consume target ordering alignment  
- Savings not fencing reserve/consume  
