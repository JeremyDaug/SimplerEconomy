use std::collections::HashMap;

use crate::game::{
    factuals::Factuals,
    firm::Firm,
    market::Market,
    pop::{Pop, PopEffect},
};

pub use crate::game::effects::{EffectScope, InstitutionEffect};

/// # Institution
///
/// An organization not purely focused on profit: state branches, religions, guilds,
/// academies, and similar. Runtime actor stored in [`crate::game::actors::Actors`].
///
/// Institutions are semi-autonomous. Players may hold high-level control (`owner`),
/// but day-to-day choices and (later) property stay with the institution.
///
/// Controlled firms live in `Actors.firms`; this type only keeps `firm_ids`.
/// Multi-market presence is via `markets` / market membership sets — institutions
/// are not children of a single market.
///
/// ## v0 scope
///
/// Kind, multi-market presence, controlled firms, flat level, loyalty, market-day
/// slot, and passive [`InstitutionEffect`]s. Property, contracts, ability trees,
/// and mandate AI come later.
#[derive(Debug, Clone)]
pub struct Institution {
    /// Unique id (within `Actors.institutions`).
    pub id: usize,
    /// Display name.
    pub name: String,
    /// State / player with high-level control (`None` = independent / NPC).
    pub owner: Option<usize>,
    /// What kind of institution this is.
    pub kind: InstitutionKind,
    /// Markets where this institution is present / may act.
    pub markets: Vec<usize>,
    /// Firms this institution directs (ids into `Actors.firms`).
    pub firm_ids: Vec<usize>,
    /// Development level (flat for v0; tree nodes later).
    pub level: u32,
    /// How content the institution is with its controller / conditions.
    ///
    /// Typically in `[0.0, 1.0]`; exact scale may be refined with mandate scoring.
    pub loyalty: f64,
    /// Where this institution inserts in market-day buy order.
    pub market_slot: MarketSlot,
    /// Passive bonuses applied by scope (realm members, firm workers, …).
    ///
    /// See [`InstitutionEffect`] / [`EffectScope`] in `effects`.
    pub effects: Vec<InstitutionEffect>,
}

/// What kind of institution this is (template family; factual trees later).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum InstitutionKind {
    /// Formal state arm (admin, military, judiciary as formalized structures).
    StateBranch,
    Religion,
    Military,
    Bureaucracy,
    /// Merchant / craft.
    Guild,
    /// Research / culture.
    Academy,
    /// Trade league, mercenary company, and other specials.
    #[default]
    Special,
}

/// Where an institution places itself in the market-day purchase order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MarketSlot {
    #[default]
    BeforeFirms,
    BetweenFirmsAndPops,
    AfterPops,
    /// Reserved for split state purchase queues; alpha uses a single slot.
    Custom(u8),
}

impl Institution {
    /// Creates an institution with the given id and name.
    ///
    /// Defaults: no owner, [`InstitutionKind::Special`], empty markets/firms,
    /// level `0`, loyalty `1.0`, [`MarketSlot::BeforeFirms`], no effects.
    pub fn new(id: usize, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            owner: None,
            kind: InstitutionKind::Special,
            markets: vec![],
            firm_ids: vec![],
            level: 0,
            loyalty: 1.0,
            market_slot: MarketSlot::BeforeFirms,
            effects: vec![],
        }
    }

    /// Sets the institution's unique id.
    pub fn with_id(mut self, id: usize) -> Self {
        self.id = id;
        self
    }

    /// Sets the display name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets the controlling state id, or `None` for independent / NPC.
    pub fn with_owner(mut self, owner: Option<usize>) -> Self {
        self.owner = owner;
        self
    }

    /// Sets the institution kind.
    pub fn with_kind(mut self, kind: InstitutionKind) -> Self {
        self.kind = kind;
        self
    }

    /// Adds a market id where this institution is present.
    pub fn with_market(mut self, market_id: usize) -> Self {
        self.markets.push(market_id);
        self
    }

    /// Adds a controlled firm id.
    pub fn with_firm(mut self, firm_id: usize) -> Self {
        self.firm_ids.push(firm_id);
        self
    }

    /// Sets the flat development level.
    pub fn with_level(mut self, level: u32) -> Self {
        self.level = level;
        self
    }

    /// Sets loyalty (typically `[0.0, 1.0]`).
    pub fn with_loyalty(mut self, loyalty: f64) -> Self {
        self.loyalty = loyalty;
        self
    }

    /// Sets market-day purchase order slot.
    pub fn with_market_slot(mut self, slot: MarketSlot) -> Self {
        self.market_slot = slot;
        self
    }

    /// Adds a passive institution effect.
    pub fn with_effect(mut self, effect: InstitutionEffect) -> Self {
        self.effects.push(effect);
        self
    }

    /// # Apply Passive Effects
    ///
    /// Push this institution's passive [`InstitutionEffect`]s onto firms and pops
    /// for the day. Called from the player-bonuses / demographic phase **before**
    /// [`Pop::update_desires`](crate::game::pop::Pop::update_desires).
    ///
    /// **Order / reach (v0):**
    /// - [`EffectScope::Members`]: workforce pops of firms in `firm_ids`.
    /// - [`EffectScope::OwnerRealm`]: pops listed on markets in `markets`
    ///   (simple stand-in for realm membership until territory ownership is wired).
    ///
    /// Birth/mortality become same-day [`PopEffect`]s (growth phase consumes them).
    /// Household/desire rewrites go through demographics later (D1); this path does
    /// not set `household_changed` yet. `firms` is mut so later institution control
    /// can attach firm-side modifiers without a signature change.
    pub fn apply_passive_effects(
        &self,
        pops: &mut HashMap<usize, Pop>,
        firms: &mut HashMap<usize, Firm>,
        markets: &HashMap<usize, Market>,
    ) {
        for effect in &self.effects {
            match effect.scope() {
                EffectScope::Members => {
                    for &firm_id in &self.firm_ids {
                        // Collect worker ids first so pops can be mutably borrowed
                        // without holding a firm borrow. `firms` is `&mut` so
                        // firm-side institution modifiers can be applied later.
                        let Some(firm) = firms.get(&firm_id) else {
                            continue;
                        };
                        let worker_ids: Vec<usize> = firm
                            .workforce
                            .iter()
                            .map(|w| w.id)
                            .filter(|&id| id != 0)
                            .collect();
                        for pop_id in worker_ids {
                            if let Some(pop) = pops.get_mut(&pop_id) {
                                Self::push_effect_to_pop(pop, *effect);
                            }
                        }
                    }
                }
                EffectScope::OwnerRealm => {
                    for &market_id in &self.markets {
                        let Some(market) = markets.get(&market_id) else {
                            continue;
                        };
                        for &pop_id in &market.pops {
                            if let Some(pop) = pops.get_mut(&pop_id) {
                                Self::push_effect_to_pop(pop, *effect);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Map one institution effect onto a pop's same-day store.
    fn push_effect_to_pop(pop: &mut Pop, effect: InstitutionEffect) {
        match effect {
            InstitutionEffect::BirthRate { rate, .. } => {
                pop.stored_effects.push(PopEffect::Birthrate(rate));
            }
            InstitutionEffect::MortalityRate { rate, .. } => {
                pop.stored_effects.push(PopEffect::Mortality(rate));
            }
        }
    }

    /// End-of-day bookkeeping for this institution.
    /// Only external input is factuals.
    pub fn record_keeping(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Institution record keeping")
    }

    /// End-of-day good decay for this institution (property when present).
    /// Only external input is factuals.
    pub fn decay_goods(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Institution decay goods")
    }
}

#[cfg(test)]
mod institution_tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    use hexx::Hex;

    use crate::game::{
        effects::EffectScope,
        firm::Firm,
        household::{DemographicRates, Household},
        market::Market,
        pop::{DemoRow, Pop, PopEffect, PopRecords},
        sentiment::Sentiment,
        workforce::Workforce,
    };

    fn make_pop(id: usize) -> Pop {
        Pop {
            id,
            job: 0,
            property: HashMap::new(),
            desires: vec![vec![]; 3],
            working_desires: vec![],
            demographics: DemoRow {
                household: Household::new(),
                species: 0,
                culture: 0,
                class: 0,
                religion: 0,
            },
            current_orders: vec![],
            previous_growth: 0.0,
            stored_effects: vec![],
            sentiment: Sentiment::new(),
            records: PopRecords::default(),
        }
    }

    #[test]
    fn new_defaults_and_fluent_builders() {
        let inst = Institution::new(1, "Admiralty")
            .with_owner(Some(10))
            .with_kind(InstitutionKind::Military)
            .with_market(3)
            .with_market(4)
            .with_firm(100)
            .with_level(2)
            .with_loyalty(0.75)
            .with_market_slot(MarketSlot::BetweenFirmsAndPops)
            .with_effect(InstitutionEffect::realm_birthrate(0.01));

        assert_eq!(inst.id, 1);
        assert_eq!(inst.name, "Admiralty");
        assert_eq!(inst.owner, Some(10));
        assert_eq!(inst.kind, InstitutionKind::Military);
        assert_eq!(inst.markets, vec![3, 4]);
        assert_eq!(inst.firm_ids, vec![100]);
        assert_eq!(inst.level, 2);
        assert_eq!(inst.loyalty, 0.75);
        assert_eq!(inst.market_slot, MarketSlot::BetweenFirmsAndPops);
        assert_eq!(inst.effects.len(), 1);
        assert_eq!(inst.effects[0].scope(), EffectScope::OwnerRealm);
    }

    #[test]
    fn apply_passive_effects_members_pushes_birthrate_to_workforce_pops() {
        let worker_id = 7;
        let firm_id = 100;
        let mut firm = Firm::new(firm_id, "Yard".into(), 1, Hex::new(0, 0));
        let mut worker = Workforce::empty();
        worker.id = worker_id;
        firm.workforce.push(worker);

        let mut firms = HashMap::from([(firm_id, firm)]);
        let mut pops = HashMap::from([(worker_id, make_pop(worker_id))]);
        let markets = HashMap::new();

        let inst = Institution::new(1, "Guild")
            .with_firm(firm_id)
            .with_effect(InstitutionEffect::member_birthrate(0.02));

        inst.apply_passive_effects(&mut pops, &mut firms, &markets);

        assert_eq!(
            pops[&worker_id].stored_effects,
            vec![PopEffect::Birthrate(0.02)]
        );
    }

    #[test]
    fn apply_passive_effects_owner_realm_pushes_to_market_pops() {
        let pop_id = 3;
        let market_id = 5;
        let market = Market {
            id: market_id,
            pops: HashSet::from([pop_id]),
            firms: HashSet::new(),
            institution_ids: HashSet::new(),
            goods: HashMap::new(),
        };
        let markets = HashMap::from([(market_id, market)]);
        let mut pops = HashMap::from([(pop_id, make_pop(pop_id))]);
        let mut firms = HashMap::new();

        let inst = Institution::new(1, "Church")
            .with_market(market_id)
            .with_effect(InstitutionEffect::realm_mortality(0.01));

        inst.apply_passive_effects(&mut pops, &mut firms, &markets);

        assert_eq!(
            pops[&pop_id].stored_effects,
            vec![PopEffect::Mortality(0.01)]
        );
    }
}
