use std::collections::{HashMap, HashSet};

use hexx::Hex;

use crate::game::factuals::Factuals;
use crate::game::good::GoodTag;
use crate::game::{actor::Actor, firmorganization::FirmOrganization, workforce::Workforce};

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
    pub property: HashMap<usize, FirmPRow>,
    pub production_line: Vec<ProductionLine>,
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

impl Firm {
    /// # New
    ///
    /// A firm with this id, name, market, and hex.
    ///
    /// No parent, no children, level 0, an empty organization, a blank owner,
    /// and no workforce, property, or production lines.
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
            property: HashMap::new(),
            production_line: vec![],
        }
    }

    /// # With Workforce
    ///
    /// Appends one workforce row and returns this firm.
    pub fn with_workforce(mut self, worker: Workforce) -> Self {
        self.workforce.push(worker);
        self
    }

    /// # With Owner Profit Share
    ///
    /// Limited owner claim: this fraction of profit. Clears remainder liability.
    ///
    /// `profit_share` must be in `0.0..=1.0`.
    pub fn with_owner_profit_share(mut self, profit_share: f64) -> Self {
        debug_assert!(
            (0.0..=1.0).contains(&profit_share),
            "profit_share must be in 0.0..=1.0"
        );
        self.owners.profit_share = profit_share;
        self.owners.liable = false;
        self
    }

    /// # With Owner Liability
    ///
    /// Owner takes the residual and covers losses.
    pub fn with_owner_liability(mut self) -> Self {
        self.owners.liable = true;
        self
    }

    /// # With Owner
    ///
    /// Sets the owner actor and returns this firm.
    pub fn with_owner(mut self, owner: Actor) -> Self {
        self.owners.owner = owner;
        self
    }

    /// # Produced Goods
    ///
    /// Goods these lines output.
    ///
    /// `factuals` supplies the process. A missing process is skipped.
    /// Returns the set of output good ids.
    pub fn produced_goods(&self, factuals: &Factuals) -> HashSet<usize> {
        let mut goods = HashSet::new();
        for line in &self.production_line {
            let Some(process) = factuals.get_process(line.process) else {
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
    /// A missing good returns 0.
    pub fn take_good(&mut self, good: usize) -> f64 {
        self.property
            .remove(&good)
            .map(|row| row.quantity)
            .unwrap_or(0.0)
    }
}

impl Owners {
    /// # Empty
    ///
    /// A blank pop owner, no profit share, and no liability.
    pub fn empty() -> Self {
        Self {
            owner: Actor::Pop(0),
            profit_share: 0.0,
            liable: false,
        }
    }

    /// # Pop Id
    ///
    /// Living pop id, or `None` for a blank or non-pop owner.
    pub fn pop_id(&self) -> Option<usize> {
        match self.owner {
            Actor::Pop(id) if id != 0 => Some(id),
            _ => None,
        }
    }
}

impl FirmPRow {
    /// # New
    ///
    /// A zero ledger. Same value as [`Self::default`].
    pub fn new() -> Self {
        Self::default()
    }

    /// # With Quantity
    ///
    /// Sets quantity. Must be `>= 0.0`.
    pub fn with_quantity(mut self, quantity: f64) -> Self {
        debug_assert!(quantity >= 0.0, "quantity must be >= 0.0");
        self.quantity = quantity;
        self
    }

    /// # With Reserve
    ///
    /// Sets the reserve. Must be `>= 0.0`.
    pub fn with_reserve(mut self, reserve: f64) -> Self {
        debug_assert!(reserve >= 0.0, "reserve must be >= 0.0");
        self.reserve = reserve;
        self
    }

    /// # Fresh Share
    ///
    /// Fraction of `quantity` that is fresh.
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
