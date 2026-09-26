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

    /// End-of-day decay.
    ///
    /// Returns `used` to `quantity`, decays that quantity (Exposure skips while
    /// owned), credits byproducts, then moves `held` into `quantity`.
    /// `consumed` is a flow counter and is not destroyed again here.
    /// Returns `(decayed, volume)` per good.
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
            let mut lost = 0.0;
            if !exposure && good.decay_rate > 0.0 && row.quantity > 0.0 {
                lost = row.quantity * good.decay_rate;
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

        for row in self.property.values_mut() {
            if row.held != 0.0 {
                row.quantity += row.held;
                row.held = 0.0;
            }
        }
        rot
    }

    /// Zeros today's production flow counters. Leaves stock in place.
    pub fn clear_property_records(&mut self) {
        for row in self.property.values_mut() {
            row.produced = 0.0;
            row.consumed = 0.0;
        }
    }

    /// Removes the row and returns on-hand quantity plus `held`.
    pub fn take_good(&mut self, good: usize) -> f64 {
        self.property
            .remove(&good)
            .map(|row| row.quantity + row.held)
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
            self.move_good(good, sign * qty);
        }
        if proposal.buyer == id {
            self.pay_freight(proposal.freight, factuals);
        }
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
    /// Today's output. Moved to `quantity` after decay.
    pub held: f64,
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
}
