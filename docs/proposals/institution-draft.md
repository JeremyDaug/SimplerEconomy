# Institution draft (proposal)

**Status:** draft for discussion — not implemented  
**Date:** 2026-07-24  
**Sources:** EconCiv vault notes + existing `Firm` / `Culture` / `Actors` patterns (direction, not requirements)

---

## What an Institution *is* (three layers)

| Layer | Role | Analogy |
|---|---|---|
| **Kind / template** (factual or semi-static) | “Military”, “Religion”, “Bureaucracy” — tree shape, unlock rules, default mandates | Culture tree definition |
| **Instance** (game actor in `Actors`) | *Your* Admiralty, *their* Church — property, loyalty, levels unlocked | Firm / State sibling |
| **Arms** (optional firms it controls) | Local garrisons, temples-as-employers, tax offices | Firm children, not the institution itself |

Design docs describe both an **ability tree** and a **semi-autonomous organization**. Separating **template** from **instance** avoids stuffing tree definitions into every runtime actor.

---

## Proposed runtime shape

```rust
/// Runtime actor: lives in Actors.institutions
pub struct Institution {
    pub id: usize,
    pub name: String,

    /// Which player/state has high-level control (None = independent / NPC).
    pub owner: Option<usize>, // State id

    /// What kind of institution this is (Military, Religion, …).
    pub kind: InstitutionKind,

    /// Markets where this institution is present / may act.
    /// Multi-market by design (unlike a local firm).
    pub markets: Vec<usize>,

    /// Firms this institution directs (tax farm, regiment, temple economy, …).
    pub firm_ids: Vec<usize>,

    /// Shared treasury / stockpile (intangibles can be goods: Piety, Authority tokens, …).
    pub property: HashMap<usize, InstPRow>, // thin cousin of FirmPRow / PopPRow

    pub contracts: Vec<Contract>,

    /// Development: levels unlocked, branch choices, culture points sunk.
    pub progression: InstitutionProgress,

    /// How content the institution is with the player / its conditions (0–1 or -1..1).
    pub loyalty: f64,
    /// Mandate / “what it wants this era” (simple for alpha).
    pub mandate: InstitutionMandate,

    /// Where it inserts in market-day buy order (before firms, between, after pops, …).
    pub market_slot: MarketSlot,

    /// Consolidated passive bonuses applied to pops/state (like culture household mods).
    pub passive_effects: Vec<InstitutionEffect>,
}

pub enum InstitutionKind {
    StateBranch,   // admin, military, judiciary as formalized arms
    Religion,
    Military,
    Bureaucracy,
    Guild,         // merchant / craft
    Academy,       // research / culture
    Special,       // trade league, mercenary company, …
}

/// Ability tree state — not the full tree definition.
pub struct InstitutionProgress {
    pub level: u32,
    /// Unlocked node ids from the factual tree for this kind.
    pub unlocked: Vec<usize>,
    /// Culture / influence invested (for cost scaling).
    pub investment: f64,
}

/// Alpha-simple “what it wants from you.”
pub enum InstitutionMandate {
    /// Keep average common-need satisfaction above threshold in owned markets.
    PopWelfare { min_common_sat: f64 },
    /// Maintain X military goods / forces.
    ForceProjection { good_id: usize, min_stock: f64 },
    /// Support a religion demographic.
    Faith { religion_id: usize, min_followers: f64 },
    /// Break-even / soft profit on controlled firms.
    Solvency { min_net_amv: f64 },
    None,
}

pub enum MarketSlot {
    BeforeFirms,
    BetweenFirmsAndPops,
    AfterPops,
    /// State/player can split purchases later; alpha: one slot.
    Custom(u8),
}

/// Prefer reusing DemographicEffect ideas + a few institution-only tags.
pub enum InstitutionEffect {
    BirthRate(f64),
    Mortality(f64),
    Legitimacy(f64),
    Authority(f64),
    CultureGeneration(f64),
    ResearchGeneration(f64),
    /// Applied into pop household rebuild path (same add pattern as culture mods).
    Household(HouseholdDef), // or DemographicEffect list
    MarketPriority(MarketSlot),
    // later: LegalTender, MigrationBridge, TaxRule, …
}
```

### Factual side (optional early)

```rust
/// Tree definition — mostly static, like tech nodes.
pub struct InstitutionTree {
    pub kind: InstitutionKind,
    pub nodes: HashMap<usize, InstitutionNode>,
}

pub struct InstitutionNode {
    pub id: usize,
    pub name: String,
    pub cost: f64,              // culture points
    pub prerequisites: Vec<usize>,
    pub exclusive_with: Vec<usize>,
    pub grants: Vec<InstitutionEffect>,
    pub unlocks_mandate: Option<InstitutionMandate>,
}
```

For alpha you can skip full trees and hardcode “level 0–3 packages” per kind.

---

## How it sits with existing architecture

```
Actors
  pops, firms, institutions

Market
  pop_ids, firm_ids
  institution_ids: HashSet<usize>   // membership only

State / Players
  institution_ids they control
  legitimacy, authority, treasury (stay on State; institutions contribute)

Factuals
  institution_trees (optional)
  cultures / religions (institutions may “sponsor” these, not replace them)
```

- **Institutions are not markets’ children** — multi-market presence via `markets: Vec<usize>`.
- **Firms they control** stay in `Actors.firms`; institution only holds ids (same partition story as market day).
- **Demographics** stay on Culture/Religion/Species; institutions *push* `HouseholdDef` / `DemographicEffect`-like bonuses into the same rebuild path used in `demographic_update`.

### Suggested household rebuild (when wired)

```text
Default
  + species mods
  + culture mods
  + religion mods
  + sum(institution.passive_effects.Household
        for institutions that apply to this pop)
```

---

## Turn integration (light)

| Phase | Institution role |
|---|---|
| Player bonuses / demos | Unlock tree node; recompute `passive_effects`; set household/effects dirty flags |
| Intra-market day | Announce/buy with `market_slot`; contracts; not pure profit |
| Organized migration (step 3) | Migratory institution efforts |
| Production / planning | Order controlled firms; institutions rarely `run_production` themselves unless they have no firm arms |
| Record keeping | Loyalty, mandate satisfaction, investment ledger |

### Loyalty sketch

Each day, score the mandate (e.g. avg common sat vs threshold) → nudge `loyalty`. Low loyalty → higher culture/legitimacy costs to use the institution, weaker passive effects, or refusal of certain player orders.

---

## Alpha vs later

| Alpha | Later |
|---|---|
| One **State** institution (player-aligned, low autonomy) | Full tree + branches |
| One **Religion** institution | Multiple competing religions |
| Thin property + firm_ids + passive effects | Intangible goods market (Piety, Charity) |
| Flat `level` + fixed unlock table | Exclusive branches, infinite invest nodes |
| Single `MarketSlot` | Split state purchase queues |

---

## What not to do yet

1. Merge Institution and State into one type (State is the player shell; institution is the power structure).  
2. Give institutions full `ProductionLine` clones of firms — prefer **controlling firms**.  
3. Store full culture trees on every instance.  
4. Dual-own firms on institution *and* market.

---

## Minimal “v0” (shippable soon)

Close to the current skeleton, but ownership and multi-market explicit:

```rust
pub struct Institution {
    pub id: usize,
    pub name: String,
    pub owner: Option<usize>,
    pub kind: InstitutionKind,
    pub markets: Vec<usize>,
    pub firm_ids: Vec<usize>,
    pub level: u32,
    pub loyalty: f64,
    pub market_slot: MarketSlot,
    pub bonuses: Vec<InstitutionBonus>, // expand from BirthRate/Mortality
    // property/contracts when market day needs them
}
```

Enough to plug into `Actors`, market membership, demographic household add, and migration stubs without inventing the whole tree system.

---

## Design tension to decide later

Docs sometimes treat **Culture as proto-institution** and sometimes as separate.

**Draft stance:** Culture/Religion remain demographic identity; Institutions are formal orgs that can sponsor or steer them. Culture points buy institution progress; they don’t turn Culture into an Actor.

---

## Suggested next step if accepted

1. Expand `institution.rs` to the v0 struct + `new` / fluent helpers.  
2. Add `institution_ids` on `Market` (membership only).  
3. Wire passive household effects into `demographic_update` rebuild.  
4. Defer full tree UI and autonomy AI.
