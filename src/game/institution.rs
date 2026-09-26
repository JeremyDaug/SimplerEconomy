use std::collections::HashMap;

use crate::game::{
    actor::Actor,
    deal::DealMaker,
    factuals::Factuals,
    firm::Firm,
    market::{Market, MarketHistory},
    marketorder::MarketOrder,
    pop::Pop,
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
/// Kind, multi-market presence, controlled firms, flat level, loyalty, and
/// passive [`InstitutionEffect`]s. Property, contracts, ability trees,
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

impl Institution {
    /// Creates an institution with the given id and name.
    ///
    /// Defaults: no owner, [`InstitutionKind::Special`], empty markets/firms,
    /// level `0`, loyalty `1.0`, no effects.
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

    /// Adds a passive institution effect.
    pub fn with_effect(mut self, effect: InstitutionEffect) -> Self {
        self.effects.push(effect);
        self
    }

    /// Passive effects are stored on the institution and not applied yet.
    pub fn apply_passive_effects(
        &self,
        pops: &mut HashMap<usize, Pop>,
        firms: &mut HashMap<usize, Firm>,
        markets: &HashMap<usize, Market>,
    ) {
        let _ = (self, pops, firms, markets);
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

impl DealMaker for Institution {
    fn actor(&self) -> Actor {
        Actor::Institution(self.id)
    }

    fn sell_orders(&self, _history: &MarketHistory) -> Vec<MarketOrder> {
        Vec::new()
    }

    fn buy_orders(&self, _history: &MarketHistory) -> Vec<MarketOrder> {
        Vec::new()
    }

    fn free_units(&self, _good: usize) -> f64 {
        0.0
    }
}
