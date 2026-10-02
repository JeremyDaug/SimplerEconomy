use std::collections::{HashMap, HashSet};

use hexx::Hex;

use crate::game::{
    actor::Actor, contract::Contract, deal::DealMaker, factuals::Factuals,
    firmorganization::FirmOrganization, good::GoodTag, market::MarketHistory,
    marketorder::MarketOrder, pop::Pop, workforce::Workforce,
};

/// A firm is one workshop: production, stock, and the people tied to it.
///
/// Parent, children, and level are the company links. How a larger company
/// acts is not written yet.
#[derive(Debug, Clone)]
pub struct Firm {
    pub id: usize,
    pub name: String,
    /// Market this firm operates in.
    pub market: usize,
    /// Hex the firm sits on.
    pub location: Hex,
    /// Owning firm, if this is a sub-firm.
    pub parent: Option<usize>,
    /// Sub-firms this firm owns.
    pub children: Vec<usize>,
    /// 0 is the lowest organizational level.
    pub level: usize,
    /// How the firm is run. Placeholder weights.
    pub org_ai_weights: FirmOrganization,
    pub owners: Owners,
    pub workforce: Vec<Workforce>,
    pub contracts: Vec<Contract>,
    pub property: HashMap<usize, FirmPRow>,
    pub production_line: Vec<ProductionLine>,
}

impl Firm {
    pub fn new(id: usize, name: String, market: usize, location: Hex) -> Self {
        Self {
            id,
            name,
            market,
            location,
            parent: None,
            children: vec![],
            level: 0,
            org_ai_weights: FirmOrganization::empty(),
            owners: Owners::empty(),
            workforce: vec![],
            contracts: vec![],
            property: HashMap::new(),
            production_line: vec![],
        }
    }

    pub fn with_workforce(mut self, worker: Workforce) -> Self {
        self.workforce.push(worker);
        self
    }

    /// Limited owner claim: this fraction of profit. Clears remainder liability.
    pub fn with_owner_profit_share(mut self, profit_share: f64) -> Self {
        debug_assert!(
            (0.0..=1.0).contains(&profit_share),
            "profit_share must be in 0.0..=1.0"
        );
        self.owners.profit_share = profit_share;
        self.owners.liable = false;
        self
    }

    /// Owner takes the residual and covers losses.
    pub fn with_owner_liability(mut self) -> Self {
        self.owners.liable = true;
        self
    }

    pub fn with_owner(mut self, owner: Actor) -> Self {
        self.owners.owner = owner;
        self
    }

    /// Reserve stock for today's production and for savings. Not written yet.
    pub fn reserve_for_day(&mut self, _factuals: &Factuals) {}

    /// Run today's production lines. Not written yet.
    pub fn produce(&mut self, _factuals: &Factuals) {}

    /// End-of-day planning. Not written yet.
    pub fn plan(&mut self, _factuals: &Factuals, _history: &MarketHistory) {}

    /// # Record Keeping
    ///
    /// Zeros each row's consumed counter through [`Self::clear_property_records`].
    pub fn record_keeping(&mut self, _factuals: &Factuals) {
        self.clear_property_records();
    }

    /// Firm bonuses pushed onto pops. No catalog yet.
    pub fn apply_passive_bonuses(&self, pops: &mut HashMap<usize, Pop>) {
        let _ = (self, pops);
    }

    /// Goods these lines output.
    pub fn produced_goods(&self, factuals: &Factuals) -> HashSet<usize> {
        let mut goods = HashSet::new();
        for line in &self.production_line {
            let Some(process) = factuals.processes.get(&line.process) else {
                continue;
            };
            for output in &process.outputs {
                goods.insert(output.good);
            }
        }
        goods
    }

    /// # Decay Goods
    ///
    /// End-of-day decay for this firm's stock.
    ///
    /// Returns `used` to `quantity`, then decays the aging part of `quantity`
    /// by the good's rate. [`FirmPRow::fresh`] is the fresh portion and stays
    /// in place. [`GoodTag::Exposure`] skips decay while the good is owned.
    /// Byproducts are credited into `quantity`. `consumed` is a flow counter
    /// and is not destroyed here.
    ///
    /// Returns `(decayed, volume)` per good. Volume is on-hand after `used`
    /// returns, plus `consumed`, including fresh stock.
    pub fn decay_goods(&mut self, factuals: &Factuals) -> HashMap<usize, (f64, f64)> {
        let mut gains: HashMap<usize, f64> = HashMap::new();
        let mut rot: HashMap<usize, (f64, f64)> = HashMap::new();

        for (&good_id, row) in self.property.iter_mut() {
            if row.used != 0.0 {
                row.quantity += row.used;
                row.used = 0.0;
            }

            let volume = (row.quantity.max(0.0) + row.consumed.max(0.0)).max(0.0);
            let good = factuals.find_good(good_id);
            let exposure = good.tags.contains(&GoodTag::Exposure);
            // Fresh stock stays whole. Only the older pile rots.
            let fresh = row.fresh.max(0.0).min(row.quantity.max(0.0));
            let aging = (row.quantity - fresh).max(0.0);
            let mut lost = 0.0;
            if !exposure && good.decay_rate > 0.0 && aging > 0.0 {
                lost = aging * good.decay_rate;
                row.quantity -= lost;
                for (&byproduct, &ratio) in &good.decay_result {
                    if ratio != 0.0 && lost != 0.0 {
                        *gains.entry(byproduct).or_insert(0.0) += lost * ratio;
                    }
                }
            }
            if volume > 0.0 || lost > 0.0 {
                let entry = rot.entry(good_id).or_insert((0.0, 0.0));
                entry.0 += lost;
                entry.1 += volume;
            }
        }

        for (good_id, amount) in gains {
            if amount == 0.0 {
                continue;
            }
            self.property
                .entry(good_id)
                .or_insert_with(FirmPRow::new)
                .quantity += amount;
        }
        rot
    }

    /// # Reset Day
    ///
    /// Clears yesterday's fresh marker and yesterday's production record.
    ///
    /// `fresh` and `produced` on every property row are set to 0. Quantity
    /// stays, so those units can rot on the coming night.
    pub fn reset_day(&mut self) {
        for row in self.property.values_mut() {
            row.fresh = 0.0;
            row.produced = 0.0;
        }
    }

    /// # Clear Property Records
    ///
    /// Zeros today's consumed counter. Leaves stock, `fresh`, and `produced`.
    pub fn clear_property_records(&mut self) {
        for row in self.property.values_mut() {
            row.consumed = 0.0;
        }
    }

    /// # Take Good
    ///
    /// Removes this good's row and returns the on-hand quantity.
    ///
    /// `fresh` is a portion of that quantity, so it is not added again.
    pub fn take_good(&mut self, good: usize) -> f64 {
        self.property
            .remove(&good)
            .map(|row| row.quantity)
            .unwrap_or(0.0)
    }

    /// Hiring pressure. Not written yet.
    pub fn calculate_hiring_pressure(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Firm calculate hiring pressure")
    }

    /// Labor moves inside this market. Not written yet.
    pub fn process_internal_labor_migration(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Firm process internal labor migration")
    }
}

impl DealMaker for Firm {
    fn actor(&self) -> Actor {
        Actor::Firm(self.id)
    }

    /// Free stock (`quantity - reserve`) listed with no named payment good.
    fn sell_orders(&self, _history: &MarketHistory) -> Vec<MarketOrder> {
        let mut orders = Vec::new();
        for (&good, row) in &self.property {
            let units = (row.quantity - row.reserve).floor();
            if units >= 1.0 {
                orders.push(MarketOrder::sell(self.actor(), good, units));
            }
        }
        orders
    }

    /// Firms do not post buys until they plan purchases again.
    fn buy_orders(&self, _history: &MarketHistory) -> Vec<MarketOrder> {
        Vec::new()
    }

    fn free_units(&self, good: usize) -> f64 {
        self.property
            .get(&good)
            .map(|row| (row.quantity - row.reserve).max(0.0))
            .unwrap_or(0.0)
    }

    /// # Fresh Share
    ///
    /// `good`'s [`FirmPRow::fresh_share`], or `0` when this firm does not hold it.
    fn fresh_share(&self, good: usize) -> f64 {
        self.property
            .get(&good)
            .map(FirmPRow::fresh_share)
            .unwrap_or(0.0)
    }

    fn evaluate(
        &self,
        proposal: &crate::game::deal::ProposedDeal,
        history: &MarketHistory,
        _factuals: &Factuals,
    ) -> crate::game::deal::DealResponse {
        if crate::game::deal::seller_can_accept(self, proposal, history) {
            crate::game::deal::DealResponse::Accept
        } else {
            crate::game::deal::DealResponse::Reject
        }
    }

    fn finalize(&mut self, proposal: &crate::game::deal::ProposedDeal, factuals: &Factuals) {
        let id = self.actor();
        let sign = if proposal.buyer == id {
            1.0
        } else if proposal.seller == id {
            -1.0
        } else {
            return;
        };
        for (&good, &qty) in &proposal.goods {
            let delta = sign * qty;
            let fresh_units = delta * proposal.fresh_share(good);
            self.move_good(good, delta);
            self.move_fresh(good, fresh_units);
        }
        if proposal.buyer == id {
            self.pay_freight(proposal.freight, factuals);
        }
    }

    fn reset_day(&mut self) {
        Firm::reset_day(self);
    }

    fn reserve(&mut self, factuals: &Factuals, _rng: &mut dyn rand::RngCore) {
        Firm::reserve_for_day(self, factuals);
    }

    fn produce(&mut self, factuals: &Factuals) {
        Firm::produce(self, factuals);
    }

    fn decay_goods(&mut self, factuals: &Factuals) -> HashMap<usize, (f64, f64)> {
        Firm::decay_goods(self, factuals)
    }

    fn record_keeping(&mut self, factuals: &Factuals, _history: &MarketHistory) {
        Firm::record_keeping(self, factuals);
    }

    fn plan(&mut self, factuals: &Factuals, history: &MarketHistory) {
        Firm::plan(self, factuals, history);
    }
}

impl Firm {
    fn pay_freight(&mut self, amount: f64, factuals: &Factuals) {
        if amount <= 0.0 {
            return;
        }
        let mut ids: Vec<usize> = self.property.keys().copied().collect();
        ids.sort_unstable();
        let mut left = amount;
        for id in ids {
            if left <= 0.0 {
                break;
            }
            let Some(good) = factuals.goods.get(&id) else {
                continue;
            };
            let efficiency = good.transport_efficiency();
            if efficiency <= 0.0 {
                continue;
            }
            let free = self.free_units(id);
            if free <= 0.0 {
                continue;
            }
            let take = (left / efficiency).min(free);
            self.move_good(id, -take);
            if let Some(row) = self.property.get_mut(&id) {
                row.consumed += take;
            }
            left -= take * efficiency;
        }
    }
}

impl Firm {
    fn move_good(&mut self, good: usize, delta: f64) {
        let row = self.property.entry(good).or_insert_with(FirmPRow::new);
        row.quantity = (row.quantity + delta).max(row.reserve).max(0.0);
    }

    /// # Move Fresh
    ///
    /// Adds `delta` to `fresh` for `good`.
    ///
    /// Creates the row when it is missing. The result is clamped at `0`.
    fn move_fresh(&mut self, good: usize, delta: f64) {
        if delta == 0.0 {
            return;
        }
        let row = self.property.entry(good).or_insert_with(FirmPRow::new);
        row.fresh = (row.fresh + delta).max(0.0);
    }
}

/// Who owns the firm and whether they carry the residual.
#[derive(Debug, Clone)]
pub struct Owners {
    pub owner: Actor,
    /// Share of profit when [`Self::liable`] is false. `0..=1`.
    pub profit_share: f64,
    /// Residual claimant. Covers losses. Ignores [`Self::profit_share`].
    pub liable: bool,
}

impl Owners {
    pub fn empty() -> Self {
        Self {
            owner: Actor::Pop(0),
            profit_share: 0.0,
            liable: false,
        }
    }

    /// Living pop id, or `None` for a blank or non-pop owner.
    pub fn pop_id(&self) -> Option<usize> {
        match self.owner {
            Actor::Pop(id) if id != 0 => Some(id),
            _ => None,
        }
    }
}

/// One process the firm can run, plus the day's quota.
#[derive(Debug, Clone)]
pub struct ProductionLine {
    pub process: usize,
    /// Iterations sought. `None` means as many as inputs allow.
    pub target: Option<f64>,
    /// Optional inputs the line is allowed to draw.
    pub inputs: Vec<usize>,
}

/// Stock of one good held by a firm.
#[derive(Debug, Clone, Copy, Default)]
pub struct FirmPRow {
    pub quantity: f64,
    /// Units earmarked and not for sale.
    pub reserve: f64,
    /// Capital tied up in a run. Returned to `quantity` at decay.
    pub used: f64,
    /// Fresh units in `quantity`. [`Firm::decay_goods`] leaves this portion in place.
    pub fresh: f64,
    /// Destroyed in production today. Already left `quantity`.
    pub consumed: f64,
    /// Units produced today, including decay byproducts when a run records them.
    pub produced: f64,
}

impl FirmPRow {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_quantity(mut self, quantity: f64) -> Self {
        debug_assert!(quantity >= 0.0, "quantity must be >= 0.0");
        self.quantity = quantity;
        self
    }

    pub fn with_reserve(mut self, reserve: f64) -> Self {
        debug_assert!(reserve >= 0.0, "reserve must be >= 0.0");
        self.reserve = reserve;
        self
    }

    /// # Fresh Share
    ///
    /// Fraction of `quantity` that [`Firm::decay_goods`] skips.
    ///
    /// `fresh` is clamped into `0..=quantity`, then divided by `quantity`.
    /// Returns `0` when `quantity` is `0`.
    pub fn fresh_share(&self) -> f64 {
        let quantity = self.quantity.max(0.0);
        if quantity == 0.0 {
            return 0.0;
        }
        self.fresh.max(0.0).min(quantity) / quantity
    }
}

#[cfg(test)]
mod firm_should {
    use std::collections::{HashMap, HashSet};

    use hexx::Hex;

    use super::{DealMaker, Firm, FirmPRow};
    use crate::game::actor::Actor;
    use crate::game::deal::ProposedDeal;
    use crate::game::factuals::Factuals;
    use crate::game::good::Good;

    fn mill(id: usize) -> Firm {
        Firm::new(id, format!("mill {id}"), 1, Hex::new(0, 0))
    }

    fn bread() -> Good {
        Good {
            id: 2,
            name: "bread".into(),
            class: None,
            decay_rate: 0.5,
            decay_result: HashMap::new(),
            mass: 1.0,
            volume: 0.0,
            tags: HashSet::new(),
            categories: Vec::new(),
        }
    }

    /// Buyer firm 1 receives `goods`. Seller firm 2 pays the negative lines.
    fn deal(goods: HashMap<usize, f64>, fresh: HashMap<usize, f64>) -> ProposedDeal {
        ProposedDeal {
            buyer: Actor::Firm(1),
            seller: Actor::Firm(2),
            match_good: 2,
            goods,
            fresh,
            freight: 0.0,
        }
    }

    #[test]
    fn finalize_moves_the_givers_fresh_share() {
        let mut buyer = mill(1);
        let mut seller = mill(2);
        let mut row = FirmPRow::new().with_quantity(10.0);
        row.fresh = 3.0;
        row.produced = 3.0;
        seller.property.insert(2, row);

        assert!((seller.fresh_share(2) - 0.3).abs() < 1e-12);
        assert_eq!(buyer.fresh_share(2), 0.0);

        let proposal = deal(HashMap::from([(2, 4.0)]), HashMap::from([(2, 0.3)]));
        buyer.finalize(&proposal, &Factuals::new());
        seller.finalize(&proposal, &Factuals::new());

        assert!((buyer.property[&2].quantity - 4.0).abs() < 1e-12);
        assert!((buyer.property[&2].fresh - 1.2).abs() < 1e-12);
        assert_eq!(buyer.property[&2].produced, 0.0);
        assert!((seller.property[&2].quantity - 6.0).abs() < 1e-12);
        assert!((seller.property[&2].fresh - 1.8).abs() < 1e-12);
        assert_eq!(seller.property[&2].produced, 3.0);
    }

    #[test]
    fn decay_spares_fresh_inside_quantity() {
        let mut firm = mill(1);
        let mut row = FirmPRow::new().with_quantity(10.0);
        row.fresh = 4.0;
        firm.property.insert(2, row);
        let factuals = Factuals::new().with_good(bread());

        let rot = firm.decay_goods(&factuals);

        assert_eq!(firm.property[&2].quantity, 7.0);
        assert_eq!(firm.property[&2].fresh, 4.0);
        assert_eq!(rot[&2].0, 3.0);
        assert_eq!(rot[&2].1, 10.0);
    }

    #[test]
    fn reset_day_clears_fresh_and_produced_and_keeps_quantity() {
        let mut firm = mill(1);
        let mut row = FirmPRow::new().with_quantity(10.0);
        row.fresh = 4.0;
        row.produced = 4.0;
        firm.property.insert(2, row);

        DealMaker::reset_day(&mut firm);

        assert_eq!(firm.property[&2].quantity, 10.0);
        assert_eq!(firm.property[&2].fresh, 0.0);
        assert_eq!(firm.property[&2].produced, 0.0);
    }

    #[test]
    fn fresh_share_caps_when_fresh_exceeds_quantity() {
        let mut row = FirmPRow::new().with_quantity(10.0);
        row.fresh = 15.0;
        assert!((row.fresh_share() - 1.0).abs() < 1e-12);

        row.fresh = 0.0;
        assert_eq!(row.fresh_share(), 0.0);

        let mut empty = FirmPRow::new();
        empty.fresh = 4.0;
        assert_eq!(empty.fresh_share(), 0.0);
    }

    #[test]
    fn take_good_returns_quantity_once() {
        let mut firm = mill(1);
        let mut row = FirmPRow::new().with_quantity(10.0);
        row.fresh = 4.0;
        firm.property.insert(2, row);

        assert_eq!(firm.take_good(2), 10.0);
        assert!(firm.property.get(&2).is_none());
    }
}
