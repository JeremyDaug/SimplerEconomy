use std::collections::{HashMap, HashSet};

use hexx::Hex;

use crate::game::{
    actor::Actor, config::GameConfig, 
    contract::Contract, 
    factuals::Factuals, 
    firmorganization::FirmOrganization, 
    good::{GoodTag, TIME}, 
    market::{Market, MarketHistory}, 
    pop::Pop, process::ProcessEffect, 
    util::whole_units, 
    workforce::Workforce,
};

/// # Firm 
/// 
/// A firm is the smallest unit of business. It deals with Production and local economic
/// calculation. Think of a firm as a building, factory, or facility where everyone is 
/// there and can interact trivially during the day.
/// 
/// When connected together they form a Company, with the firms inside being called 
/// Sub-Firms.
/// 
/// ## Properties
/// 
/// Firms (including sub-firms) should have unique ids and names to help with both
/// navigation and player readability.
/// 
/// All firms have a market which they primarily act in and a location hex where
/// they physically operate. The latter is used for when tiles change regions.
/// 
/// The Organizational Data of a firm is the Parent (ID for it's owning firm if any),
/// children (the sub-firms it owns), level (the importance in a Company Structure),
/// and org_ai_weights for how the firm operates and manages both itself and it's 
/// children.
/// 
/// The Population Data is mostly connections and rules for who has a relationship
/// with the firm. Owner defines who owns it, how it's owned, and how profits and
/// losses are distributed, as well as a few other rules.
#[derive(Debug, Clone)]
pub struct Firm {
    /// Unique Id for the Firm.
    pub id: usize,
    /// The unique name of the firm. If a child of another firm, this is it's 
    /// regional/sub name.
    pub name: String,

    /// Which market this firm is attached to and operating in.
    pub market: usize,
    /// The Specific Hex Tile the firm is centered in, for market splits and joins.
    pub location: Hex,

    /// The Parent firm to this firm (if it has one).
    pub parent: Option<usize>,
    /// The Children Firm for 
    pub children: Vec<usize>,
    /// The organizational level of the firm. 0 is lowest. if part of a larger Company
    /// it may be higher.
    /// 
    /// Firms of an organizational level can only control within 1 level of difference.
    pub level: usize,
    /// What kind of organization scheme the firm is operating under. Can only be
    /// changed by the highest level
    pub org_ai_weights: FirmOrganization,

    /// Information on who own's the firm, profits and risk distribution, and other
    /// such information.
    pub owners: Owners,
    /// Information on the workers, how many there are, how much they're payed, what 
    /// they transer over and back, and similar information.
    pub workforce: Vec<Workforce>,
    /// Contracts are long term deals that the firm has, typically buy or sell orders
    /// to other firms, but it also forms a secondary source of labor in contactors,
    /// as well as connecting to institutions and states for access to their stuff.
    pub contracts: Vec<Contract>,

    /// The Property owned by the firm. In some cases, this can be shared with the owner
    /// if it's an especially small business, but for most purposes, this is separate 
    /// and distinct.
    pub property: HashMap<usize, FirmPRow>,

    /// The details of the processes and work the firm will do.
    /// 
    /// Production lines are ordered by priority, those first in the list get run
    /// first. This should be noted for production lines that feed into each other.
    pub production_line: Vec<ProductionLine>,

    /// Day snapshots. Written in [`Firm::record_keeping`], read by [`Firm::plan`].
    pub records: FirmRecords,
    /// Total Transport spent this day (buyer haul). Cleared at day start.
    pub transport_spent: f64,
}

/// # Firm Records
///
/// Firm-wide memory for planning. Property rows still hold per-good flows;
/// this is the rolled-up picture.
///
/// `profit_ratio` is realized (sold AMV / cost of goods sold), not process
/// AMV-out / AMV-in.
#[derive(Debug, Clone)]
pub struct FirmRecords {
    /// Total AMV received from sales today.
    pub sold_amv: f64,
    /// Total AMV given to workers/owners as wages/profit.
    pub placed_amv: f64,
    /// Total AMV spent on purchases today.
    pub bought_amv: f64,
    /// Cost basis of units sold or fence-capped placed today.
    pub sold_cost_amv: f64,
    /// Realized profit today: disposed AMV / cost. 1.0 if unknown.
    pub profit_ratio: f64,
    /// EMA of [`Self::profit_ratio`].
    pub profit_avg: f64,
    /// Firm-wide sell success today (`(sold + placed_credited) / sell_target`).
    pub sell_success: f64,
    /// EMA of [`Self::sell_success`].
    pub sell_success_avg: f64,
}

impl Default for FirmRecords {
    fn default() -> Self {
        Self {
            sold_amv: 0.0,
            bought_amv: 0.0,
            sold_cost_amv: 0.0,
            profit_ratio: 1.0,
            profit_avg: 1.0,
            sell_success: 1.0,
            sell_success_avg: 1.0,
            placed_amv: 0.0,
        }
    }
}

impl FirmRecords {
    /// Neutral records: unknown profit/success 1.0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Residual profit AMV from the last snapshot (`sold - cost`, floored at 0).
    pub fn yesterday_profit_amv(&self) -> f64 {
        (self.sold_amv - self.sold_cost_amv).max(0.0)
    }
}

impl Firm {
    /// # Apply Passive Bonuses
    ///
    /// Push firm-level bonuses onto related pops (workforce, owners, …) during the
    /// player-bonuses / demographic phase, **after** institutions and **before**
    /// [`Pop::update_desires`](crate::game::pop::Pop::update_desires).
    ///
    /// v0: no firm bonus catalog yet — signature and call site only so later work
    /// can attach effects without rewiring the turn.
    pub fn apply_passive_bonuses(&self, pops: &mut HashMap<usize, Pop>) {
        let _ = (self, pops);
        // Stub: firm → pop passive bonuses (wages-as-effects, owner dividends, …).
    }

    /// Pushes one workforce row.
    pub fn with_workforce(mut self, worker: Workforce) -> Self {
        self.workforce.push(worker);
        self
    }

    /// Sets a limited owner claim: this fraction of yesterday's profit AMV.
    /// Must be in 0..=1. Clears remainder (dividend / partial owner).
    pub fn with_owner_profit_share(mut self, profit_share: f64) -> Self {
        debug_assert!(
            (0.0..=1.0).contains(&profit_share),
            "profit_share must be in 0.0..=1.0"
        );
        self.owners.profit_share = profit_share;
        self.owners.liable = false;
        self
    }

    /// Marks the owner as the residual claimant (owner-operator).
    /// Leftover after wages, worker shares, stock fence, and growth.
    pub fn with_owner_liability(mut self) -> Self {
        self.owners.liable = true;
        self
    }


    /// # Decay Goods
    ///
    /// End-of-day decay for this firm.
    ///
    /// 1. Return `used` capital to `quantity` and clear `used` so it can decay.
    /// 2. Decay on-hand `quantity` by each good's `decay_rate` (skip Exposure while owned).
    ///    `held` does not rot.
    /// 3. Apply decay byproducts. Do not record them as `produced`.
    /// 4. Move `held` into `quantity` last, after every row has decayed.
    /// 5. [`FirmPRow::sync_reserve`] after quantity changes.
    ///
    /// `consumed` on the row is a day-flow counter (goods already left `quantity`
    /// during production, and Consumed-type byproducts were applied there). It is
    /// not destroyed again here. Clear it with [`Firm::clear_day_flows`].
    /// Returns `(decayed, volume)` per good. Volume is on-hand after `used`
    /// is returned, plus `consumed`. `held` is not volume (it did not rot).
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
                debug_assert!(row.quantity >= 0.0, "Quantity should never be negative!");
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

            row.sync_reserve();
        }

        for (good_id, amount) in gains {
            if amount == 0.0 {
                continue;
            }
            let row = self.property.entry(good_id).or_insert_with(FirmPRow::new);
            row.quantity += amount;
            row.sync_reserve();
        }

        for row in self.property.values_mut() {
            if row.held != 0.0 {
                row.quantity += row.held;
                row.held = 0.0;
            }
            row.sync_reserve();
        }
        rot
    }

    /// # Clear Property Records
    ///
    /// Zero today's exchange and production counters on every property row:
    /// `produced`, `consumed`, `bought`, `bought_amv`, `sold`, `sold_amv`.
    ///
    /// Leaves `used` and `held` alone (`decay_goods` returns them) and does not
    /// touch cost basis, prices, or planning targets.
    ///
    /// Intended for day start so the previous day's totals stay visible overnight.
    /// Safe to call from a later phase if we want that window longer.
    pub fn clear_property_records(&mut self) {
        for row in self.property.values_mut() {
            row.produced = 0.0;
            row.consumed = 0.0;
            row.bought = 0.0;
            row.bought_amv = 0.0;
            row.sold = 0.0;
            row.sold_amv = 0.0;
            row.placed = 0.0;
            row.placed_amv = 0.0;
            row.sell_fills = 0.0;
            row.sell_rejects = 0.0;
            row.sell_no_proposal = 0.0;
        }
        self.transport_spent = 0.0;
    }

    /// # Take Good
    ///
    /// Removes this good's property row and returns on-hand `quantity` plus
    /// `held`. Returns 0.0 if the good was not held.
    pub fn take_good(&mut self, good: usize) -> f64 {
        self.property
            .remove(&good)
            .map(|row| row.quantity + row.held)
            .unwrap_or(0.0)
    }

    /// Hiring / expansion pressure that pulls workers into this firm.
    pub fn calculate_hiring_pressure(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Firm calculate hiring pressure")
    }

    /// Local hiring / labor reallocation within the same market.
    pub fn process_internal_labor_migration(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Firm process internal labor migration")
    }

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
            records: FirmRecords::new(),
            transport_spent: 0.0,
        }
    }

    /// Goods this firm's running lines output.
    pub fn produced_goods(&self, factuals: &Factuals) -> HashSet<usize> {
        let mut goods = HashSet::new();
        for line in &self.production_line {
            if matches!(line.target, Some(t) if t <= 0.0) {
                continue;
            }
            let Some(process) = factuals.processes.get(&line.process) else {
                continue;
            };
            for output in &process.outputs {
                goods.insert(output.good);
            }
        }
        goods
    }

    /// Sets the owning actor. `Actor::Pop(0)` is none and is not paid.
    pub fn with_owner(mut self, owner: Actor) -> Self {
        self.owners.owner = owner;
        self
    }

    /// On-hand plus held, minus owner dinner. Time is not reduced.
    fn production_available(&self) -> HashMap<usize, f64> {
        self.property
            .iter()
            .map(|(&good_id, row)| {
                (good_id, row.production_stock())
            })
            .collect()
    }

    /// # Run Production
    /// 
    /// Executes all production plans currently in `production_line` (in order).
    /// Plans are assumed to have already been made for the day.
    /// 
    /// Regardless of whether the firm currently holds everything needed, the processes
    /// will still run (let `do_process` handle throttling and restrictions).
    /// 
    /// Side effects on the firm:
    /// - Applies consumed inputs to `quantity` first, then `held`. Process
    ///   outputs (and Consumed-input decay products) go to `held`, not
    ///   `quantity`. New output goods are auto-created. Later lines in the
    ///   same day may spend `held` after on-hand stock, excluding owner
    ///   dinner.
    /// - Records day-flows on each [`FirmPRow`]: `produced` for positive changes
    ///   (outputs + decay results), `consumed` for destroyed/consumed inputs.
    ///   Those two input types are not distinguished on the row; decay products of
    ///   Consumed inputs show up as `produced` on their result goods.
    /// - Used capital goods are removed from `quantity` then `held` **and**
    ///   recorded into `used` (returned at decay, then that stock rots).
    ///   Capital is never added to `consumed`.
    ///   Later: fold capital cost / maintenance / amortization into output
    ///   `average_cost`. Capital should wear; it is not indestructible. Not
    ///   needed for v0 cost blending.
    /// - Factors are left untouched (not consumed, used, or locked).
    /// - After quantity changes, [`FirmPRow::sync_reserve`] matches `reserve` to
    ///   `min(quantity, reserve_target)`. `held` is not reserved.
    /// - Output `average_cost` blends this run's input AMV (allocated by each
    ///   output's share of `last_amv_produced`) into existing inventory cost basis.
    /// - Records success rate, iterations, effects, missing goods, and AMV 
    ///   of the goods involved on each `ProductionLine`.
    /// 
    /// Returns `ProcessEffect`s (research, culture, growth...) for the caller to
    /// apply elsewhere. Good flows live on the property rows; market
    /// production/consumption totals should sum those rows.
    /// 
    /// Available stock is `quantity + held` minus owner dinner (Time is
    /// not reduced). The `market` parameter is used solely to snapshot
    /// current AMV values for record-keeping.
    /// 
    /// ## Panic
    /// 
    /// Panics if good or process is not found in factuals.
    pub fn run_production(&mut self, factuals: &Factuals, market: &Market) -> Vec<ProcessEffect> {
        let mut effects = Vec::new();

        self.pay_complexity_time(factuals);
        for i in 0..self.production_line.len() {
            // Skip idle lines (target 0). `do_process` requires a positive target when Some.
            if matches!(self.production_line[i].target, Some(t) if t <= 0.0) {
                continue;
            }
            let process_id = self.production_line[i].process;
            let target = self.production_line[i].target;
            let Some(process) = factuals.processes.get(&process_id) else {
                panic!("Process not found!"); return vec![];
            };
            let available = self.production_available();
            let result = process.do_process(&available, target, factuals);
            let line = &mut self.production_line[i];

            // This-run AMV snapshots; leftover values would poison cost blending.
            line.last_amv_consumed = 0.0;
            line.last_amv_produced = 0.0;

            // Apply net changes to property (outputs + consumed inputs + decay)
            // and record produced / consumed day-flows on each row.
            for (&good_id, &delta) in &result.changes {
                let amv = if let Some(good) = market.goods.get(&good_id) {
                    good.amv
                } else { 1.0 };

                if delta > 0.0 {
                    // Produced (outputs + decay results of Consumed inputs)
                    let row = self.property.entry(good_id).or_insert_with(FirmPRow::new);
                    row.held += delta;
                    row.produced += delta;
                    debug_assert!(row.held >= 0.0, "held should never be negative!");
                    line.last_amv_produced += amv * delta;
                } else if delta < 0.0 {
                    // Destroyed or Consumed inputs; both recorded as `consumed`.
                    let consumed_qty = -delta;
                    let Some(row) = self.property.get_mut(&good_id) else {
                        unreachable!("A sanity checkpoint, we should never consume goods we don't have.");
                        return vec![];
                    };
                    row.take_for_production(consumed_qty);
                    row.consumed += consumed_qty;
                    row.sync_reserve();
                    debug_assert!(row.quantity >= 0.0, "Quantity should never be negative!");
                    debug_assert!(row.held >= 0.0, "held should never be negative!");
                    line.last_amv_consumed += amv * consumed_qty;
                }
            }

            // Blend this run's input AMV into each output's inventory cost basis.
            // Allocated by that output's share of produced AMV (joint products split cost).
            for (&good_id, &delta) in &result.changes {
                if delta <= 0.0 {
                    continue;
                }
                let amv = if let Some(good) = market.goods.get(&good_id) {
                    good.amv
                } else { 1.0 };
                let unit_cost = if line.last_amv_produced != 0.0 {
                    line.last_amv_consumed * amv / line.last_amv_produced
                } else {
                    0.0
                };
                if let Some(row) = self.property.get_mut(&good_id) {
                    row.blend_average_cost(delta, unit_cost);
                }
            }

            // Remove used capital from quantity then held; return it at decay.
            for (&good_id, &used) in &result.used_inputs {
                if let Some(row) = self.property.get_mut(&good_id) {
                    row.take_for_production(used);
                    debug_assert!(row.quantity >= 0.0, "Quantity should never be negative.");
                    debug_assert!(row.held >= 0.0, "held should never be negative.");
                    row.used += used;
                    row.sync_reserve();
                }
            }

            // Record success + result details on the production line
            let success = if let Some(t) = line.target {
                if t > 0.0 {
                    (result.iterations / t).min(1.0)
                } else {
                    0.0
                }
            } else {
                if result.iterations > 0.0 { 1.0 } else { 0.0 }
            };
            line.last_success_rate = success;
            line.last_iterations = result.iterations;
            line.last_effects = result.effects.clone();
            line.last_missing_goods = result.missing_goods.clone();

            // Collect effects for the caller to apply elsewhere
            effects.extend(result.effects);
        }

        effects
    }

    /// Destroyed Time for the firm-wide complexity tax, taken before any
    /// line runs so overhead limits how much production can follow.
    fn pay_complexity_time(&mut self, factuals: &Factuals) {
        let need: f64 = 0.0; // todo: implement complexity time need calculation
        if need <= 0.0 {
            return;
        }
        let have = self
            .property
            .get(&TIME)
            .map(|row| row.production_stock())
            .unwrap_or(0.0);
        let take = need.min(have.max(0.0));
        if take <= 0.0 {
            return;
        }
        let row = self.property.entry(TIME).or_insert_with(FirmPRow::new);
        row.take_for_production(take);
        row.consumed += take;
        row.sync_reserve();
    }

    /// Floor a collapsed line at 1 iteration and credit missing inputs for
    /// that iteration. Called immediately before the line runs so a later
    /// line still gets its own inputs after an earlier line consumed stock.
    /// Idle never-run lines (`target` 0, no missing, no iterations) stay idle.
    fn apply_keep_alive_line(&mut self, factuals: &Factuals, i: usize) {
        let process_id = self.production_line[i].process;
        let Some(process) = factuals.processes.get(&process_id) else {
            return;
        };
        let target = self.production_line[i].target.unwrap_or(0.0);
        let collapsed = self.production_line[i].last_iterations <= 0.0
            && (!self.production_line[i].last_missing_goods.is_empty() || target > 0.0);
        if target <= 0.0 && !collapsed {
            return;
        }
        if target < 1.0 {
            self.production_line[i].target = Some(1.0);
        }
        for req in process.requirements() {
            let need = req.amount;
            if need <= 0.0 {
                continue;
            }
            let have = self
                .property
                .get(&req.good)
                .map(|row| row.production_stock())
                .unwrap_or(0.0);
            if have < need {
                self.grant_keep_alive_good(req.good, need - have);
            }
        }
    }

    /// Adds `qty` of `good` with no cost basis (keep-alive subsidy).
    fn grant_keep_alive_good(&mut self, good: usize, qty: f64) {
        debug_assert!(qty.is_finite() && qty >= 0.0, "keep-alive qty must be finite and >= 0");
        if qty <= 0.0 {
            return;
        }
        if good == TIME {
            self.credit_time(qty);
            return;
        }
        let row = self.property.entry(good).or_insert_with(FirmPRow::new);
        row.quantity += qty;
        row.sync_reserve();
    }
}

/// # Owners
/// 
/// Owners defines how a firm is owned, who owns it, profit and loss distribution,
/// and other factors, like some high level business logic.
/// 
/// Currently a placeholder.
#[derive(Debug, Clone)]
pub struct Owners {
    /// The Actor/owner of the firm.
    /// 
    /// Most commonly held by Pops, who get access to profits, but are
    /// also held accountable for losses in most circumstances.
    /// 
    /// If held by another firm, then they are also a child to that firm.
    /// 
    /// If owned by an institution, then they are also under their control, they will
    /// obey that institution who will override the firm's logic with their own.
    /// 
    /// If owned by a state, then it is under the control of the player, and so the
    /// player sets it's goals and rules.
    /// 
    /// Firms owned by an Institution or State can still have a parent and children 
    /// firms, representing logical subdivisions under them. For example, a 'Guilds'
    /// institution could represent mulitple Guilds, and each of these guilds is a
    /// firm with it's own internal structure, keeping them financially independent, but
    /// still able to coordinate and operate together.
    pub owner: Actor,

    /// If the owner is a State or Institution, they may override the market priority 
    /// of the firm.
    pub priority_override: Option<f64>,
    /// Share of yesterday's profit AMV paid after wages and growth retain. 0..=1.
    /// Ignored when [`Self::liable`] is set (owner-operator leftover).
    pub profit_share: f64,
    /// When true, this owner takes leftover till after wages, worker profit
    /// shares, stock fence, and growth. Owner-operator residual claim.
    /// On a loss (yesterday profit AMV <= 0) they also cover an AMV
    /// shortfall vs the firm's needs from their own stock.
    /// When false, [`Self::profit_share`] is a limited percent of yesterday's
    /// profit AMV (dividend / partial owner) and they do not cover losses.
    pub liable: bool,
}

impl Owners {
    pub fn empty() -> Self {
        Owners {
            owner: Actor::Pop(0),
            priority_override: None,
            profit_share: 0.0,
            liable: false,
        }
    }

    /// Living pop id for this owner, or `None` for blank / non-pop owners.
    pub fn pop_id(&self) -> Option<usize> {
        match self.owner {
            Actor::Pop(id) if id != 0 => Some(id),
            _ => None,
        }
    }
}

/// Result of [`Firm::pay_wage_shares`]. Amounts are whole units of the coin good.
#[derive(Debug, Clone)]
pub struct WagePayout {
    /// On-hand coinage before the payout.
    pub coinage: f64,
    /// Units taken for the owner share (may not have been credited).
    pub owner_amount: f64,
    /// Units credited to workers in total.
    pub worker_amount: f64,
    /// Configured owner actor (may be `Pop(0)` / none).
    pub owner: Actor,
    /// True when the owner pop existed and received `owner_amount`.
    pub owner_credited: bool,
    /// Per-pop worker credits `(pop id, units)`.
    pub workers: Vec<(usize, f64)>,
}

impl WagePayout {
    /// Zero payout for this owner and starting till.
    pub fn empty(owner: Actor, coinage: f64) -> Self {
        Self {
            coinage,
            owner_amount: 0.0,
            worker_amount: 0.0,
            owner,
            owner_credited: false,
            workers: vec![],
        }
    }
}

impl Firm {
    /// True if this firm uses any process input (`use_target` or line inputs).
    fn has_process_inputs(&self) -> bool {
        self.property.values().any(|row| row.use_target > 0.0)
            || self
                .production_line
                .iter()
                .any(|line| !line.inputs.is_empty())
    }

    /// True if this firm produces but needs no inputs (mine, well).
    fn is_input_free_producer(&self) -> bool {
        !self.production_line.is_empty() && !self.has_process_inputs()
    }

    /// Owner and worker fractions of on-hand coin for [`Firm::pay_wage_shares`].
    /// Input-free producers split the whole till in the configured ratio.
    fn wage_share_fracs(&self, config: &GameConfig) -> (f64, f64) {
        let owner = config.labor.owner_share;
        let worker = config.labor.worker_share;
        if !self.is_input_free_producer() {
            return (owner, worker);
        }
        let sum = owner + worker;
        if sum <= 0.0 {
            (0.0, 0.0)
        } else {
            (owner / sum, worker / sum)
        }
    }

    /// Adds `qty` of `good`. Time is earmarked for production like
    /// [`Firm::credit_time`]. Other goods get no cost basis.
    pub fn credit_good(&mut self, good: usize, qty: f64) {
        debug_assert!(qty.is_finite() && qty >= 0.0, "credit qty must be finite and >= 0.0");
        if qty <= 0.0 {
            return;
        }
        if good == TIME {
            self.credit_time(qty);
            return;
        }
        let row = self.property.entry(good).or_insert_with(FirmPRow::new);
        row.quantity += qty;
        row.sync_reserve();
    }

    /// Subtracts `qty` from a property row and syncs the stockpile reserve.
    pub fn debit_good(&mut self, good: usize, qty: f64) {
        debug_assert!(qty.is_finite() && qty >= 0.0, "debit qty must be finite and >= 0");
        if qty <= 0.0 {
            return;
        }
        if let Some(row) = self.property.get_mut(&good) {
            row.quantity = (row.quantity - qty).max(0.0);
            row.sync_reserve();
        }
    }

    /// Counts one sell-side meeting on `good` (fill, reject, or no-proposal).
    pub fn note_sell_meet(&mut self, good: usize, fills: f64, rejects: f64, no_proposal: f64) {
        debug_assert!(fills >= 0.0 && rejects >= 0.0 && no_proposal >= 0.0);
        let row = self.property.entry(good).or_insert_with(FirmPRow::new);
        row.sell_fills += fills;
        row.sell_rejects += rejects;
        row.sell_no_proposal += no_proposal;
    }

    /// Records in-kind placement (wage or profit) at `unit_amv`.
    /// Does not move stock; call after [`Self::debit_good`].
    pub fn record_placed(&mut self, good: usize, qty: f64, unit_amv: f64) {
        debug_assert!(qty.is_finite() && qty >= 0.0, "placed qty must be finite and >= 0");
        debug_assert!(unit_amv.is_finite(), "placed unit AMV must be finite");
        if qty <= 0.0 {
            return;
        }
        let row = self.property.entry(good).or_insert_with(FirmPRow::new);
        row.placed += qty;
        row.placed_amv += qty * unit_amv.max(0.0);
    }

    /// Adds committed Time and earmarks it for production (not sale).
    pub fn credit_time(&mut self, qty: f64) {
        debug_assert!(qty.is_finite() && qty >= 0.0, "time qty must be finite and >= 0");
        if qty <= 0.0 {
            return;
        }
        let row = self.property.entry(TIME).or_insert_with(FirmPRow::new);
        row.quantity += qty;
        row.reserve = (row.reserve + qty).min(row.quantity);
        row.use_target = row.use_target.max(row.quantity);
    }

    /// Planned day's output of `good`: sum of line targets times recipe
    /// output amount. Missing processes contribute 0.
    pub fn daily_output(&self, good: usize, factuals: &Factuals) -> f64 {
        let mut qty = 0.0;
        for line in &self.production_line {
            let Some(process) = factuals.processes.get(&line.process) else {
                continue;
            };
            let iters = match line.target {
                Some(target) => target.max(0.0),
                None => line.last_iterations.max(0.0),
            };
            if iters <= 0.0 {
                continue;
            }
            for output in &process.outputs {
                if output.good == good {
                    qty += output.amount * iters;
                }
            }
        }
        qty
    }

    /// Sell qty posted and fenced from remainder: `min(sell_target, max_sal *
    /// daily output)` when this firm makes the good. Unconstrained
    /// `sell_target` when it does not (merchants / leftover input sales).
    /// Sell-success still uses the row's `sell_target`.
    pub fn posted_sell_qty(&self, good: usize, history: &MarketHistory, factuals: &Factuals) -> f64 {
        let sell = self
            .property
            .get(&good)
            .map(|row| row.sell_target.max(0.0))
            .unwrap_or(0.0);
        if sell <= 0.0 {
            return 0.0;
        }
        let made = self.daily_output(good, factuals);
        if made <= 0.0 {
            return sell;
        }
        let velocity = history.max_salability() * made;
        let decay = factuals
            .goods
            .get(&good)
            .map(|g| g.decay_rate)
            .unwrap_or(1.0);
        let hold = made
            * FirmPRow::operations_hold_days(factuals.config.firm.output_cover, decay);
        let qty = self
            .property
            .get(&good)
            .map(|row| row.shelf())
            .unwrap_or(0.0);
        let excess = (qty - hold).max(0.0);
        sell.min(velocity.max(excess))
    }

    /// AMV of on-hand goods above stock, growth, and posted sell, skipping Time.
    pub fn leftover_profit_amv(&self, history: &MarketHistory, factuals: &Factuals) -> f64 {
        self.property
            .iter()
            .filter(|(good, _)| **good != TIME)
            .map(|(good, row)| {
                let fence = self.posted_sell_qty(*good, history, factuals);
                row.profit_spendable_above_sell(fence) * history.price(*good).max(0.0)
            })
            .sum()
    }
}

/// # Production Line
/// 
/// A Production line is a process and the information around it. This includes
/// targets, and input restrictions.
#[derive(Debug, Clone)]
pub struct ProductionLine {
    /// The process being run.
    pub process: usize,
    /// The target being sought. If None, then the firm wants to do as many as possible.
    pub target: Option<f64>,
    /// Expected recurring scale in iterations. Plan lerps this toward
    /// throughput evidence; `target` (the day's quota) steps around it.
    pub aim: f64,
    /// What goods are going to go into the process. Used to restrict optional inputs.
    pub inputs: Vec<usize>,
    /// A record of the average productivity (amv out / amv in) of the process.
    pub historical_productivity: f64,

    /// Success rate of the most recent production run (clamped 0.0–1.0 when a
    /// target was provided).
    pub last_success_rate: f64,
    /// How many iterations were actually completed in the last run.
    pub last_iterations: f64,
    /// Effects (research, culture, growth, etc.) produced by the last run.
    pub last_effects: Vec<ProcessEffect>,
    /// Which goods ran out and caused the process to stop early.
    pub last_missing_goods: Vec<usize>,

    /// Snapshot of Abstract Market Value (AMV) for every good that was **consumed**
    /// (non-capital inputs) during the last production run.
    pub last_amv_consumed: f64,
    /// Snapshot of Abstract Market Value (AMV) for every good that was **produced**
    /// (outputs + decay) during the last production run.
    pub last_amv_produced: f64,
    /// Consecutive plan days this line sat at `target` 0 with no leftover-buy
    /// demand. Specialized lines drop after [`FirmConfig::abandon_idle_days`].
    pub idle_days: u32,
}

/// # Firm AMV Bound
/// Recipe-derived AMV bounds for a [`FirmPRow`] (planning).
///
/// Planning writes these from process recipes: residual WTP as a buy cap
/// on inputs, input-cost rollup as a sell floor on outputs. They are
/// guidestones for later growth / margin reads, not a gate on posting or
/// settling trades. Headroom vs market AMV is a later growth / margin read.
///
/// [`FirmAmvBound::None`] is exchange-only stock (barter, till, merchant
/// restock) that is not a process input or output here.
/// [`FirmAmvBound::MinMax`] is an in-firm intermediate (produced and used
/// in this firm).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FirmAmvBound {
    /// Not a process input or output for this firm.
    #[default]
    None,
    /// Output sell floor: unit AMV below which the firm does not want to sell.
    Minimum(f64),
    /// Input buy cap: unit AMV above which the firm does not want to pay.
    Maximum(f64),
    /// Both a sell floor and a buy cap `(minimum, maximum)`.
    MinMax(f64, f64),
}

impl FirmAmvBound {
    /// Returns the sell-floor AMV, or `None` if this bound has no floor.
    pub fn minimum(self) -> Option<f64> {
        match self {
            Self::None | Self::Maximum(_) => None,
            Self::Minimum(v) | Self::MinMax(v, _) => Some(v),
        }
    }

    /// Returns the buy-cap AMV, or `None` if this bound has no cap.
    pub fn maximum(self) -> Option<f64> {
        match self {
            Self::None | Self::Minimum(_) => None,
            Self::Maximum(v) | Self::MinMax(_, v) => Some(v),
        }
    }

    /// Caps `bid` to this bound's buy cap when one is set; otherwise returns `bid`.
    /// Planning helper. Order emission does not clamp posted AMV.
    pub fn clamp_bid(self, bid: f64) -> f64 {
        match self.maximum() {
            Some(cap) => bid.min(cap),
            None => bid,
        }
    }

    /// Raises `ask` to this bound's sell floor when one is set; otherwise returns `ask`.
    /// Planning helper. Order emission does not clamp posted AMV.
    pub fn clamp_ask(self, ask: f64) -> f64 {
        match self.minimum() {
            Some(floor) => ask.max(floor),
            None => ask,
        }
    }

    /// Returns true if this bound has a buy cap and `market_amv` is above it.
    /// Planning query. `create_orders` does not skip the buy when this is true.
    pub fn market_above_buy_cap(self, market_amv: f64) -> bool {
        match self.maximum() {
            Some(cap) => market_amv > cap,
            None => false,
        }
    }

    /// Multiplies bound AMVs by `scale`. `None` stays `None`.
    pub fn scaled(self, scale: f64) -> Self {
        match self {
            Self::None => Self::None,
            Self::Minimum(min) => Self::Minimum(min * scale),
            Self::Maximum(max) => Self::Maximum(max * scale),
            Self::MinMax(min, max) => Self::MinMax(min * scale, max * scale),
        }
    }

    /// Returns [`FirmAmvBound::None`], [`FirmAmvBound::Minimum`],
    /// [`FirmAmvBound::Maximum`], or [`FirmAmvBound::MinMax`] from an optional
    /// sell floor and optional buy cap.
    pub fn from_parts(minimum: Option<f64>, maximum: Option<f64>) -> Self {
        match (minimum, maximum) {
            (None, None) => Self::None,
            (Some(min), None) => Self::Minimum(min),
            (None, Some(max)) => Self::Maximum(max),
            (Some(min), Some(max)) => Self::MinMax(min, max),
        }
    }
}

/// Snapshot of line and output-good facts for [`Firm::plan`].

/// # Firm Property Row
/// 
/// A row of property data for a Firm. Includes data for management, oversight, and 
/// targeting for both purchasing and use in production.
/// 
/// ## Target notes
/// 
/// After Market, a firm wants to keep it's Stock Target, which should contain everything 
/// they need, for use (consumption/capital), growth, reserves, and wages as well 
/// as some excess to cover decay.
/// 
/// Reserves are a warehousing goal, meant to help smooth out inconsistent supply for 
/// our uses. Production in particular, but also other things if needed.
/// 
/// Growth is a target used for a firm's expansion. If a firm wants to expand, it should 
/// add to this target to build up to it's new size goal, and remove from it as the firm
/// actually grows, effectively moving from growth to use, stock, or reserve.
#[derive(Debug, Clone, Copy, Default)]
pub struct FirmPRow {
    // unit info and budgeting data
    /// The amount currently owned.
    pub quantity: f64,
    /// The number of units of quantity which are currently reserved and thus won't be
    /// offered for sale. Meant to reserve for production or between buying and selling
    /// for merchants.
    pub reserve: f64,
    /// The average ownership of the good over the last 30 days at the end of the day
    /// to include both mercantile buy/sell and productive consumption/output.
    pub rolling_average: f64,

    /// How many the firm wants to purchase from the market. Mercantile firms will try
    /// to purchase this amonut before they turn around and sell.
    pub purchase_target: f64,
    /// If selling, how many units they wish to sell each day. A soft Minimum.
    /// May undershoot through no fault of their own, would gladly overshoot so long
    /// as it doesn't interfere with production plans.
    pub sell_target: f64,
    /// How much we want/expect to use in a given day, used/consumed/destroyed.
    pub use_target: f64,
    /// The target amount the firm wants to have after all purchases have been made.
    /// For production oriented firms, this is what they will have before production.
    /// For mercantile firms, this is what they want to have before they
    /// turn around and sell.
    pub stock_target: f64,
    /// The target for reservation, how much they want to keep on hand. This is a backup
    /// target, meant to help inconsistent supply. Goes up or down depending on the 
    /// success of reaching purchase, sell, and use targets, modulated by the firm's
    /// uncertainty.
    pub reserve_target: f64,
    /// Extra units above stock to retain for expansion. Wages may raid this;
    /// owner and worker profit shares may not.
    pub growth_target: f64,
    /// Recipe-derived buy cap / sell floor. See [`FirmAmvBound`]. Planning data;
    /// default [`FirmAmvBound::None`].
    /// Not a hard cap/floor, but useful for estimating the productivity/profitability of 
    /// the good in production.
    pub amv_bound: FirmAmvBound,

    // market exchange data
    /// The average cost to get these good so far. Updated after each purchase and
    /// productive process. Equal to the AMV of purchase, or the AMV of the goods which 
    /// went into producing it.
    /// Used for value production efficiency calculations.
    pub average_cost: f64,
    /// If being sold, this is the average AMV price they've been able to get for it.
    /// Used for value efficiency calculations.
    pub average_price: f64,
    /// How many units were purchased today.
    pub bought: f64,
    /// The Total AMV cost for bought today. `Unit cost = bought_amv / bought`.
    pub bought_amv: f64,
    /// How many units were sold today.
    pub sold: f64,
    /// The total AMV gained for sales today. `Unit price = sold_amv / sold`.
    pub sold_amv: f64,
    /// How many units were handed to workers or the owner today (in-kind), not a
    /// market deal. Counted at market AMV. Planning credits at most
    /// `stock_fence` of this toward sell success.
    pub placed: f64,
    /// Market AMV of today's in-kind placements (`placed * unit AMV`).
    pub placed_amv: f64,
    /// Number of matched sell meetings that filled today.
    pub sell_fills: f64,
    /// Number of seller-rejected sell meetings today.
    pub sell_rejects: f64,
    /// Number of matched sells where the buyer named no basket.
    pub sell_no_proposal: f64,
    /// EMA of units `sold` to the market (not profit placed). Plan walk uses this so
    /// one bad day does not wind a line down.
    pub sold_avg: f64,
    /// EMA of `sell_fills`, times we matched and sold.
    pub sell_fills_avg: f64,
    /// EMA of `sell_rejects`, times we matched and rejected.
    pub sell_rejects_avg: f64,
    /// EMA of `sell_no_proposal`, times we matched but had no proposal.
    pub sell_no_proposal_avg: f64,
    /// The targeted unit AMV for Buying and/or Selling. If the row has both purchase 
    /// and sell targets, then this is a midpoint price, and the difference between
    /// buying and selling is defined by the Margin
    pub amv_target: f64,
    /// If being bought and sold, this modifies the buy and sell prices off of the 
    /// [`FirmPRow::amv_target`] appropriately. A simple multiplier to the AMV.
    /// Buy Price = amv_target * (1.0 - margin)
    /// Sell Price = amv_target * (1.0 + margin)
    /// 
    /// Allowed to be negative.
    pub margin: f64,

    // Production data
    /// Amount of this good currently tied up as capital in active production runs.
    /// Removed from `quantity` (then `held`) during `run_production`; returned
    /// to `quantity` at decay, then that stock decays.
    pub used: f64,
    /// Today's process output. Not moved to `quantity` until after decay occurs, giving
    /// firms a day of grace for their output.
    pub held: f64,
    /// Amount of the good that was consumed or destroyed today in production.
    /// We do not need to distinguish between consumed and destroyed as the output of 
    /// production procesess includes the output of consumed.
    pub consumed: f64,
    /// How many units of the good were produced today either directly through processes
    /// or indirectly through consumption/decay outputs.
    pub produced: f64,
}

impl FirmPRow {
    /// Empty row; all fields 0. Same as [`Default`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets on-hand quantity.
    /// Must be `>= 0.0`.
    pub fn with_quantity(mut self, quantity: f64) -> Self {
        debug_assert!(quantity >= 0.0, "quantity must be >= 0.0");
        self.quantity = quantity;
        self
    }

    /// Sets units earmarked and not offered for sale.
    /// Must be `>= 0.0`.
    pub fn with_reserve(mut self, reserve: f64) -> Self {
        debug_assert!(reserve >= 0.0, "reserve must be >= 0.0");
        self.reserve = reserve;
        self
    }

    /// Sets the rolling average of on-hand quantity.
    /// Must be `>= 0.0`.
    pub fn with_rolling_average(mut self, rolling_average: f64) -> Self {
        debug_assert!(rolling_average >= 0.0, "rolling_average must be >= 0.0");
        self.rolling_average = rolling_average;
        self
    }

    /// Sets how many units the firm wants to buy today.
    /// Must be `>= 0.0`.
    pub fn with_purchase_target(mut self, purchase_target: f64) -> Self {
        debug_assert!(purchase_target >= 0.0, "purchase_target must be >= 0.0");
        self.purchase_target = purchase_target;
        self
    }

    /// Sets how many units the firm wants to sell today.
    /// Must be `>= 0.0`.
    pub fn with_sell_target(mut self, sell_target: f64) -> Self {
        debug_assert!(sell_target >= 0.0, "sell_target must be >= 0.0");
        self.sell_target = sell_target;
        self
    }

    /// Sets how many units the firm wants to use, consume, or destroy today.
    /// Must be `>= 0.0`.
    pub fn with_use_target(mut self, use_target: f64) -> Self {
        debug_assert!(use_target >= 0.0, "use_target must be >= 0.0");
        self.use_target = use_target;
        self
    }

    /// Sets the operating inventory target after shopping.
    /// Must be `>= 0.0`.
    pub fn with_stock_target(mut self, stock_target: f64) -> Self {
        debug_assert!(stock_target >= 0.0, "stock_target must be >= 0.0");
        self.stock_target = stock_target;
        self
    }

    /// Sets the sell-floor / backup stockpile target.
    /// Must be `>= 0.0`.
    pub fn with_reserve_target(mut self, reserve_target: f64) -> Self {
        debug_assert!(reserve_target >= 0.0, "reserve_target must be >= 0.0");
        self.reserve_target = reserve_target;
        self
    }

    /// Sets units kept for growth after wages, above the stock fence.
    /// Must be `>= 0.0`.
    pub fn with_growth_target(mut self, growth_target: f64) -> Self {
        debug_assert!(growth_target >= 0.0, "growth_target must be >= 0.0");
        self.growth_target = growth_target;
        self
    }

    /// Sets recipe-derived AMV bounds (buy cap / sell floor).
    /// Bound values must be finite.
    pub fn with_amv_bound(mut self, amv_bound: FirmAmvBound) -> Self {
        match amv_bound {
            FirmAmvBound::None => {}
            FirmAmvBound::Minimum(v) | FirmAmvBound::Maximum(v) => {
                debug_assert!(v.is_finite(), "amv_bound value must be finite");
            }
            FirmAmvBound::MinMax(min, max) => {
                debug_assert!(min.is_finite(), "amv_bound minimum must be finite");
                debug_assert!(max.is_finite(), "amv_bound maximum must be finite");
            }
        }
        self.amv_bound = amv_bound;
        self
    }

    /// Sets inventory cost basis (AMV). May be negative for bads.
    pub fn with_average_cost(mut self, average_cost: f64) -> Self {
        self.average_cost = average_cost;
        self
    }

    /// Sets realized average sale AMV. May be negative for bads.
    pub fn with_average_price(mut self, average_price: f64) -> Self {
        self.average_price = average_price;
        self
    }

    /// Sets units purchased today.
    /// Must be `>= 0.0`.
    pub fn with_bought(mut self, bought: f64) -> Self {
        debug_assert!(bought >= 0.0, "bought must be >= 0.0");
        self.bought = bought;
        self
    }

    /// Sets total AMV spent on today's purchases.
    pub fn with_bought_amv(mut self, bought_amv: f64) -> Self {
        self.bought_amv = bought_amv;
        self
    }

    /// Sets units sold today.
    /// Must be `>= 0.0`.
    pub fn with_sold(mut self, sold: f64) -> Self {
        debug_assert!(sold >= 0.0, "sold must be >= 0.0");
        self.sold = sold;
        self
    }

    /// Sets total AMV received from today's sales.
    pub fn with_sold_amv(mut self, sold_amv: f64) -> Self {
        self.sold_amv = sold_amv;
        self
    }

    /// Sets units placed in-kind today.
    /// Must be `>= 0.0`.
    pub fn with_placed(mut self, placed: f64) -> Self {
        debug_assert!(placed >= 0.0, "placed must be >= 0.0");
        self.placed = placed;
        self
    }

    /// Sets total market AMV of today's in-kind placements.
    pub fn with_placed_amv(mut self, placed_amv: f64) -> Self {
        self.placed_amv = placed_amv;
        self
    }

    /// Days of daily flow to hold, given `operations_cover` and tonight's decay.
    ///
    /// If a full cover pile would lose more than one day's output to decay,
    /// shrink the hold (`cover * (1 - decay)`). Otherwise overshoot so
    /// `cover` days survive one night (`cover / (1 - decay)`). Full daily
    /// decay (1.0) holds nothing.
    pub fn operations_hold_days(operations_cover: f64, decay_rate: f64) -> f64 {
        debug_assert!(
            operations_cover.is_finite() && operations_cover >= 0.0,
            "operations_cover must be finite and >= 0.0"
        );
        debug_assert!(decay_rate.is_finite(), "decay_rate must be finite");
        let cover = operations_cover.max(0.0);
        let decay = decay_rate.clamp(0.0, 1.0);
        if cover <= 0.0 {
            return 0.0;
        }
        let survive = 1.0 - decay;
        if survive <= 0.0 {
            return 0.0;
        }
        let waste_days = cover * decay;
        if waste_days <= 1.0 {
            cover / survive
        } else {
            cover * survive
        }
    }

    /// Opening producer stock: decay-adjusted operations hold, one day to sell.
    pub fn operations_opening(
        daily_output: f64,
        operations_cover: f64,
        decay_rate: f64,
    ) -> Self {
        debug_assert!(
            daily_output >= 0.0 && daily_output.is_finite(),
            "daily_output must be finite and >= 0.0"
        );
        let daily = daily_output.max(0.0);
        let buffer = daily * Self::operations_hold_days(operations_cover, decay_rate);
        Self::new()
            .with_quantity(buffer)
            .with_stock_target(buffer)
            .with_sell_target(daily)
    }

    /// Sets the standing unit AMV for buying and/or selling.
    pub fn with_amv_target(mut self, amv_target: f64) -> Self {
        self.amv_target = amv_target;
        self
    }

    /// Sets the current price margin up and down from the amv_target for buy and sell
    /// orders.
    pub fn with_margin(mut self, margin: f64) -> Self {
        self.margin = margin;
        self
    }

    /// Sets capital currently locked in production.
    /// Must be `>= 0.0`.
    pub fn with_used(mut self, used: f64) -> Self {
        debug_assert!(used >= 0.0, "used must be >= 0.0");
        self.used = used;
        self
    }

    /// Sets today's process output waiting to join `quantity`.
    /// Must be `>= 0.0`.
    pub fn with_held(mut self, held: f64) -> Self {
        debug_assert!(held >= 0.0, "held must be >= 0.0");
        self.held = held;
        self
    }

    /// Sets units consumed or destroyed in production today.
    /// Must be `>= 0.0`.
    pub fn with_consumed(mut self, consumed: f64) -> Self {
        debug_assert!(consumed >= 0.0, "consumed must be >= 0.0");
        self.consumed = consumed;
        self
    }

    /// Sets units produced today (direct outputs and decay results).
    /// Must be `>= 0.0`.
    pub fn with_produced(mut self, produced: f64) -> Self {
        debug_assert!(produced >= 0.0, "produced must be >= 0.0");
        self.produced = produced;
        self
    }

    /// Match `reserve` to the stockpile guarantee: `min(quantity, reserve_target)`.
    /// Never negative.
    pub fn sync_reserve(&mut self) {
        debug_assert!(self.quantity >= 0.0, "quantity must be >= 0.0");
        self.reserve = self.quantity.min(self.reserve_target.max(0.0)).max(0.0);
    }

    /// On-hand plus today's holding slot. What a production line may spend.
    pub fn production_stock(&self) -> f64 {
        self.quantity.max(0.0) + self.held.max(0.0)
    }

    /// Spends `qty` from `quantity` first, then `held`.
    pub fn take_for_production(&mut self, qty: f64) {
        debug_assert!(qty.is_finite() && qty >= 0.0, "take qty must be finite and >= 0.0");
        if qty <= 0.0 {
            return;
        }
        let from_qty = qty.min(self.quantity.max(0.0));
        self.quantity -= from_qty;
        let rest = qty - from_qty;
        if rest > 0.0 {
            debug_assert!(
                self.held + 1e-12 >= rest,
                "held must cover leftover production take"
            );
            self.held = (self.held - rest).max(0.0);
        }
    }

    /// Blend `added` units at `unit_cost` into inventory cost basis.
    /// `quantity + held` must already include `added`.
    pub fn blend_average_cost(&mut self, added: f64, unit_cost: f64) {
        debug_assert!(self.quantity >= 0.0, "quantity must be >= 0.0");
        debug_assert!(self.held >= 0.0, "held must be >= 0.0");
        let stock = self.production_stock();
        if stock > 0.0 {
            let previous = (stock - added).max(0.0);
            self.average_cost =
                (previous * self.average_cost + added * unit_cost) / stock;
        }
    }

    /// Unreserved stock: `quantity - reserve`.
    pub fn available(&self) -> f64 {
        self.quantity - self.reserve
    }

    /// Shelf for sale or decay-skip: on-hand plus today's `held`.
    pub fn shelf(&self) -> f64 {
        self.quantity.max(0.0) + self.held.max(0.0)
    }

    /// Units that can be offered for sale.
    /// `shelf - max(reserve, reserve_target)`, floored at 0.
    /// `reserve_target` is the stockpile guarantee; `reserve` is the live copy.
    pub fn sellable(&self) -> f64 {
        let floor = self.reserve.max(self.reserve_target).max(0.0);
        (self.shelf() - floor).max(0.0)
    }

    /// Shelf units not fenced by reserve, reserve target, or (for production
    /// inputs) stock target / use target. Includes today's `held`.
    pub fn free_for_market(&self) -> f64 {
        let mut floor = self.reserve.max(self.reserve_target);
        if self.use_target > 0.0 {
            floor = floor.max(self.stock_target).max(self.use_target);
        }
        (self.shelf() - floor).max(0.0)
    }

    /// Units to buy today from current targets and stock.
    /// Producer inputs cap at the stock-target shortfall when stock_target is set.
    /// Merchants (no use_target) emit the full purchase_target.
    pub fn purchase_qty(&self) -> f64 {
        if self.purchase_target <= 0.0 {
            0.0
        } else if self.use_target > 0.0 && self.stock_target > 0.0 {
            self.purchase_target.min((self.stock_target - self.quantity).max(0.0))
        } else {
            self.purchase_target
        }
    }

    /// Mid AMV for orders: `amv_target` when set, otherwise `fallback`.
    pub fn mid_amv(&self, fallback: f64) -> f64 {
        if self.amv_target != 0.0 {
            self.amv_target
        } else {
            fallback
        }
    }

    /// Standing bid AMV: `mid * (1 - margin)` when the row both buys and sells,
    /// else `mid`.
    pub fn bid_amv(&self, mid: f64) -> f64 {
        if self.purchase_target > 0.0 && self.sell_target > 0.0 {
            mid * (1.0 - self.margin)
        } else {
            mid
        }
    }

    /// Standing ask AMV: `mid * (1 + margin)` when the row both buys and sells,
    /// else `mid`.
    pub fn ask_amv(&self, mid: f64) -> f64 {
        if self.purchase_target > 0.0 && self.sell_target > 0.0 {
            mid * (1.0 + self.margin)
        } else {
            mid
        }
    }

    /// Unit AMV paid today: `bought_amv / bought`. 0 if nothing was bought.
    pub fn bought_unit_amv(&self) -> f64 {
        if self.bought > 0.0 {
            self.bought_amv / self.bought
        } else {
            0.0
        }
    }

    /// Unit AMV received today: `sold_amv / sold`. 0 if nothing was sold.
    pub fn sold_unit_amv(&self) -> f64 {
        if self.sold > 0.0 {
            self.sold_amv / self.sold
        } else {
            0.0
        }
    }
}

impl FirmPRow {
    /// Units profit will not take: the operations want plus reserve.
    /// Wages may raid this; see [`Self::wage_fence`].
    pub fn stock_fence(&self) -> f64 {
        self.stock_target.max(self.reserve_target).max(0.0)
    }

    /// Primary floor wages will not spend: today's use or sell plan, plus
    /// reserve. Operations `stock_target` is secondary and may be raided.
    pub fn wage_fence(&self) -> f64 {
        let operating = if self.use_target > 0.0 {
            self.use_target
        } else {
            self.sell_target
        };
        operating.max(self.reserve_target).max(0.0)
    }

    /// On-hand units above the wage floor (growth and the operations buffer
    /// may be raided for wages).
    pub fn wage_spendable(&self) -> f64 {
        (self.quantity - self.wage_fence()).max(0.0)
    }

    /// On-hand units above stock, growth, and `sell_target`.
    pub fn profit_spendable(&self) -> f64 {
        self.profit_spendable_above_sell(self.sell_target)
    }

    /// On-hand units above stock, growth, and `sell_fence`.
    /// Profit leftover uses posted sell, not unconstrained `sell_target`.
    pub fn profit_spendable_above_sell(&self, sell_fence: f64) -> f64 {
        (self.quantity
            - self.stock_fence()
            - self.growth_target.max(0.0)
            - sell_fence.max(0.0))
        .max(0.0)
    }

    /// In-kind units that count toward sell success: `placed` capped at the
    /// operations fence. Profit above the fence is a dump, not a hit.
    pub fn placed_credited(&self) -> f64 {
        self.placed.max(0.0).min(self.stock_fence())
    }

    /// Market AMV of [`Self::placed_credited`].
    pub fn placed_credited_amv(&self) -> f64 {
        let placed = self.placed.max(0.0);
        if placed <= 0.0 {
            0.0
        } else {
            self.placed_amv.max(0.0) * (self.placed_credited() / placed)
        }
    }
}

#[cfg(test)]
mod firm {
    use crate::game::factuals::Factuals;
    use crate::game::good::Good; // if you need Good defs
    use crate::game::market::{Market, MarketGood};
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput, ProcessEffect};
    use std::collections::{HashMap, HashSet};
    use crate::game::firm::{Firm, FirmAmvBound, FirmPRow, ProductionLine};

    fn make_good(id: usize, name: &str, decay_result: HashMap<usize, f64>) -> Good {
        Good {
            id,
            name: name.to_string(),
            class: None,
            tags: Default::default(),
            decay_rate: 0.0,
            decay_result,
            mass: 1.0,
            volume: 1.0,
            categories: vec![],
            // add any other fields your Good actually has
        }
    }

    // Helper to build a minimal Factuals with one process
    fn make_factuals_with_process(process: Process) -> Factuals {
        let mut factuals = Factuals::new();
        factuals.processes.insert(process.id, process);
        factuals
    }

    // Helper to build a Market with AMV data for the goods we care about
    fn make_market_with_amvs(amvs: &[(usize, f64)]) -> Market {
        let mut goods = HashMap::new();
        for &(id, amv) in amvs {
            goods.insert(id, MarketGood::new().with_amv(amv));
        }
        Market {
            id: 42,
            pops: HashSet::new(),
            firms: HashSet::new(),
            institution_ids: HashSet::new(),
            goods,
            friction: 0.0,
            unavailable_goods: HashSet::new(),
            market_days: 0,
            leftover_buy: HashMap::new(),
        }
    }

    fn empty_firm_row(quantity: f64) -> FirmPRow {
        FirmPRow::new().with_quantity(quantity)
    }

    fn empty_production_line(process_id: usize) -> ProductionLine {
        ProductionLine {
            process: process_id,
            target: None,
            inputs: vec![],
            historical_productivity: 0.0,
            aim: 0.0,
            last_success_rate: 0.0,
            last_iterations: 0.0,
            last_effects: vec![],
            last_missing_goods: vec![],
            last_amv_consumed: 0.0,
            last_amv_produced: 0.0,
            idle_days: 0,
        }
    }


    mod run_production_should {
        use crate::game::process::InputEffect;
        use super::*;

        #[test]
        fn test_basic_production_run() {
            // Simple process: 2 wood -> 1 plank (Consumed input, fixed output)
            let process = Process::new(1, "sawmill", 0)
                .with_input(ProcessInput::new(10, 2.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(20, 1.0, true));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(10, make_good(10, "wood", HashMap::new()));
            factuals.goods.insert(20, make_good(20, "plank", HashMap::new()));

            let mut firm = Firm::new(1, "Test Sawmill".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(10, FirmPRow::new().with_quantity(10.0));

            // Add a production line
            firm.production_line.push(ProductionLine {
                process: 1,
                target: None,
                inputs: vec![10],
                historical_productivity: 0.0,
                aim: 0.0,
                last_success_rate: 0.0,
                last_iterations: 0.0,
                last_effects: vec![],
                last_missing_goods: vec![],
                last_amv_consumed: 0.0,
                last_amv_produced: 0.0,
                idle_days: 0,
            });

            let market = make_market_with_amvs(&[(10, 5.0), (20, 12.0)]);

            let effects = firm.run_production(&factuals, &market);

            // Property should be updated
            assert_eq!(firm.property[&10].quantity, 0.0);
            assert_eq!(firm.property[&20].quantity, 0.0);
            assert_eq!(firm.property[&20].held, 5.0); // 5 iterations * 1.0
            assert_eq!(firm.property[&10].consumed, 10.0);
            assert_eq!(firm.property[&20].produced, 5.0);
            assert_eq!(firm.property[&10].used, 0.0);
            // 10 wood * AMV 5 = 50 in; 5 planks * AMV 12 = 60 out -> unit cost 10.
            assert_eq!(firm.property[&20].average_cost, 10.0);
            assert!(effects.is_empty());

            // Line should have recorded success + AMV snapshots
            let line = &firm.production_line[0];
            assert_eq!(line.last_success_rate, 1.0);
            assert_eq!(line.last_iterations, 5.0);
            assert_eq!(line.last_amv_consumed, 50.0);
            assert_eq!(line.last_amv_produced, 60.0);
        }

        #[test]
        fn skips_a_zero_target_line() {
            let process = Process::new(1, "sawmill", 0)
                .with_input(ProcessInput::new(10, 2.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(20, 1.0, true));
            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(10, make_good(10, "wood", HashMap::new()));
            factuals.goods.insert(20, make_good(20, "plank", HashMap::new()));

            let mut firm = Firm::new(1, "Idle mill".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(10, FirmPRow::new().with_quantity(10.0));
            let mut line = empty_production_line(1);
            line.target = Some(0.0);
            firm.production_line.push(line);

            let market = make_market_with_amvs(&[(10, 5.0), (20, 12.0)]);
            let effects = firm.run_production(&factuals, &market);

            assert!(effects.is_empty());
            assert_eq!(firm.property[&10].quantity, 10.0);
            assert!(!firm.property.contains_key(&20));
            assert_eq!(firm.production_line[0].last_iterations, 0.0);
        }

        #[test]
        fn clamps_reserve_when_consumed_quantity_falls_below_it() {
            let process = Process::new(1, "sawmill", 0)
                .with_input(ProcessInput::new(10, 1.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(20, 1.0, true));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(10, make_good(10, "wood", HashMap::new()));
            factuals.goods.insert(20, make_good(20, "plank", HashMap::new()));

            let mut firm = Firm::new(1, "Reserved Sawmill".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_quantity(10.0)
                    .with_reserve(8.0)
                    .with_reserve_target(8.0),
            );
            firm.production_line.push(empty_production_line(1));
            firm.production_line[0].inputs = vec![10];
            firm.production_line[0].target = Some(4.0);

            let market = make_market_with_amvs(&[(10, 1.0), (20, 1.0)]);
            firm.run_production(&factuals, &market);

            // 4 consumed: quantity 6, reserve synced to min(6, target 8).
            assert_eq!(firm.property[&10].quantity, 6.0);
            assert_eq!(firm.property[&10].reserve, 6.0);
            assert_eq!(firm.property[&10].consumed, 4.0);
        }


        #[test]
        fn keep_alive_off_does_not_feed_starved_line() {
            let process = Process::new(2, "limited_craft", 0)
                .with_input(ProcessInput::new(30, 3.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(40, 1.0, true));
            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(30, make_good(30, "wood", HashMap::new()));
            factuals.goods.insert(40, make_good(40, "plank", HashMap::new()));
            factuals.goods.insert(5, make_good(5, "coin", HashMap::new()));

            let mut firm = Firm::new(2, "Starved Shop".into(), 42, hexx::Hex::new(0, 0));
            firm.production_line.push(empty_production_line(2));
            firm.production_line[0].target = Some(4.0);
            let market = make_market_with_amvs(&[(30, 1.0), (40, 1.0), (5, 0.21)]);
            firm.run_production(&factuals, &market);

            assert_eq!(firm.production_line[0].last_iterations, 0.0);
            assert!(!firm.property.contains_key(&40));
            assert!(!firm.property.contains_key(&5));
        }

        #[test]
        fn keep_alive_leaves_never_run_idle_line_alone() {
            let process = Process::new(2, "limited_craft", 0)
                .with_input(ProcessInput::new(30, 3.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(40, 1.0, true));
            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(30, make_good(30, "wood", HashMap::new()));
            factuals.goods.insert(40, make_good(40, "plank", HashMap::new()));
            factuals.goods.insert(5, make_good(5, "coin", HashMap::new()));
            factuals.config.firm.keep_alive = true;

            let mut firm = Firm::new(2, "Idle Shop".into(), 42, hexx::Hex::new(0, 0));
            firm.production_line.push(empty_production_line(2));
            firm.production_line[0].target = Some(0.0);
            let market = make_market_with_amvs(&[(30, 1.0), (40, 1.0), (5, 0.21)]);
            firm.run_production(&factuals, &market);

            assert_eq!(firm.production_line[0].target, Some(0.0));
            assert_eq!(firm.production_line[0].last_iterations, 0.0);
            assert!(!firm.property.contains_key(&40));
        }


        #[test]
        fn syncs_reserve_up_toward_target_on_new_output() {
            let process = Process::new(1, "sawmill", 0)
                .with_input(ProcessInput::new(10, 1.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(20, 1.0, true));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(10, make_good(10, "wood", HashMap::new()));
            factuals.goods.insert(20, make_good(20, "plank", HashMap::new()));

            let mut firm = Firm::new(1, "Stockpile Mill".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(10, FirmPRow::new().with_quantity(4.0));
            firm.property.insert(
                20,
                FirmPRow::new().with_quantity(0.0).with_reserve_target(10.0),
            );
            firm.production_line.push(empty_production_line(1));
            firm.production_line[0].inputs = vec![10];
            firm.production_line[0].target = Some(4.0);

            let market = make_market_with_amvs(&[(10, 1.0), (20, 1.0)]);
            firm.run_production(&factuals, &market);

            assert_eq!(firm.property[&20].quantity, 0.0);
            assert_eq!(firm.property[&20].held, 4.0);
            assert_eq!(firm.property[&20].reserve, 0.0);
        }

        #[test]
        fn blends_input_amv_into_existing_output_cost() {
            let process = Process::new(1, "sawmill", 0)
                .with_input(ProcessInput::new(10, 2.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(20, 1.0, true));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(10, make_good(10, "wood", HashMap::new()));
            factuals.goods.insert(20, make_good(20, "plank", HashMap::new()));

            let mut firm = Firm::new(1, "Blend Mill".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(10, FirmPRow::new().with_quantity(4.0));
            firm.property.insert(
                20,
                FirmPRow::new().with_quantity(2.0).with_average_cost(4.0),
            );
            firm.production_line.push(empty_production_line(1));
            firm.production_line[0].inputs = vec![10];
            firm.production_line[0].target = Some(2.0);

            // 4 wood * AMV 5 = 20 in; 2 planks * AMV 10 = 20 out -> unit cost 10.
            // (2 * 4 + 2 * 10) / 4 = 7.
            let market = make_market_with_amvs(&[(10, 5.0), (20, 10.0)]);
            firm.run_production(&factuals, &market);

            assert_eq!(firm.property[&20].quantity, 2.0);
            assert_eq!(firm.property[&20].held, 2.0);
            assert_eq!(firm.property[&20].average_cost, 7.0);
        }

        #[test]
        fn test_partial_run_with_target_and_missing_goods() {
            let process = Process::new(2, "limited_craft", 0)
                .with_input(ProcessInput::new(30, 3.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(40, 1.0, true));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(30, make_good(30, "wood", HashMap::new()));
            factuals.goods.insert(40, make_good(40, "plank", HashMap::new()));

            let mut firm = Firm::new(2, "Limited Workshop".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(30, FirmPRow {
                quantity: 6.0, // only enough for 2 iterations (need 3 per iter)
                ..Default::default() // we'll add used_capital etc. via insert if needed
            });

            firm.production_line.push(ProductionLine {
                process: 2,
                target: Some(10.0), // wants 10, will only get ~2
                inputs: vec![30],
                historical_productivity: 0.0,
                aim: 0.0,
                last_success_rate: 0.0,
                last_iterations: 0.0,
                last_effects: vec![],
                last_missing_goods: vec![],
                last_amv_consumed: 0.0,
                last_amv_produced: 0.0,
                idle_days: 0,
            });

            let market = make_market_with_amvs(&[(30, 2.0), (40, 8.0)]);

            firm.run_production(&factuals, &market);

            // check property changes
            assert_eq!(firm.property[&30].quantity, 0.0);
            assert_eq!(firm.property[&40].quantity, 0.0);
            assert_eq!(firm.property[&40].held, 2.0);

            let line = &firm.production_line[0];
            //assert!((line.last_success_rate - 0.233333).abs() < 0.01);
            assert_eq!(line.last_success_rate, 0.2);
            assert_eq!(line.last_iterations, 2.0);
            assert_eq!(line.last_missing_goods, vec![30]);
            assert_eq!(line.last_amv_consumed, 12.0);
            assert_eq!(line.last_amv_produced, 16.0);

            assert_eq!(firm.property[&30].consumed, 6.0);
            assert_eq!(firm.property[&40].produced, 2.0);
        }

        #[test]
        fn test_capital_goods_not_counted_as_consumed() {
            // Process that uses a Capital good (e.g. saw blade) + consumes wood
            let process = Process::new(3, "capital_test", 0)
                .with_input(ProcessInput::new(50, 1.0, true, InputType::Capital, false)) // saw
                .with_input(ProcessInput::new(10, 2.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(20, 1.0, true));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(10, make_good(10, "wood", HashMap::new()));
            factuals.goods.insert(20, make_good(20, "wood", HashMap::new()));
            factuals.goods.insert(50, make_good(50, "plank", HashMap::new()));

            let mut firm = Firm::new(3, "Capital Test Firm".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(10, FirmPRow { quantity: 10.0, ..Default::default() });
            firm.property.insert(50, FirmPRow { quantity: 1.0, ..Default::default() });

            firm.production_line.push(ProductionLine {
                process: 3,
                target: None,
                inputs: vec![50, 10],
                historical_productivity: 0.0,
                aim: 0.0,
                last_success_rate: 0.0,
                last_iterations: 0.0,
                last_effects: vec![],
                last_missing_goods: vec![],
                last_amv_consumed: 0.0,
                last_amv_produced: 0.0,
                idle_days: 0,
            });

            let market = make_market_with_amvs(&[(10, 5.0), (20, 12.0), (50, 100.0)]);

            firm.run_production(&factuals, &market);

            // Capital good should be recorded in used, not in consumed
            assert_eq!(firm.property[&50].used, 1.0);
            assert_eq!(firm.property[&50].consumed, 0.0);
            assert_eq!(firm.property[&50].quantity, 0.0);
            assert_eq!(firm.property[&10].quantity, 8.0);
            assert_eq!(firm.property[&10].consumed, 2.0);
            assert_eq!(firm.property[&20].produced, 1.0);
            assert_eq!(firm.property[&20].held, 1.0);
            assert_eq!(firm.property[&20].quantity, 0.0);
        }

        #[test]
        fn test_effects_and_new_output_good() {
            let process = Process::new(4, "researchy", 0)
                .with_input(ProcessInput::new(10, 1.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(99, 2.0, true))
                .with_effect(ProcessEffect::Research(10.0));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(10, make_good(10, "wood", HashMap::new()));
            factuals.goods.insert(20, make_good(99, "plank", HashMap::new()));

            let mut firm = Firm::new(4, "Research Lab".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(10, FirmPRow { quantity: 5.0, ..Default::default() });

            firm.production_line.push(ProductionLine {
                process: 4,
                target: None,
                inputs: vec![10],
                historical_productivity: 0.0,
                aim: 0.0,
                last_success_rate: 0.0,
                last_iterations: 0.0,
                last_effects: vec![],
                last_missing_goods: vec![],
                last_amv_consumed: 0.0,
                last_amv_produced: 0.0,
                idle_days: 0,
            });

            let market = make_market_with_amvs(&[(10, 3.0), (99, 50.0)]);

            let effects = firm.run_production(&factuals, &market);

            assert_eq!(effects.len(), 1);
            assert!(matches!(effects[0], ProcessEffect::Research(50.0)));

            // New good 99 should have been created in property
            assert!(firm.property.contains_key(&99));
            assert_eq!(firm.property[&99].quantity, 0.0);
            assert_eq!(firm.property[&99].held, 10.0);
            assert_eq!(firm.property[&99].produced, 10.0);
            assert_eq!(firm.property[&10].consumed, 5.0);
        }

        #[test]
        #[should_panic(expected = "Process not found!")]
        fn test_unknown_process_panics() {
            let factuals = Factuals::new();

            let mut firm = Firm::new(5, "Broken Firm".into(), 42, hexx::Hex::new(0, 0));
            firm.production_line.push(ProductionLine {
                process: 999, // does not exist
                target: Some(5.0),
                inputs: vec![],
                historical_productivity: 0.0,
                aim: 0.0,
                last_success_rate: 0.42,
                last_iterations: 3.0,
                last_effects: vec![ProcessEffect::Culture(1.0)],
                last_missing_goods: vec![1],
                last_amv_consumed: 10.0,
                last_amv_produced: 0.0,
                idle_days: 0,
            });

            let market = make_market_with_amvs(&[]);

            firm.run_production(&factuals, &market);
        }
    
        #[test]
        fn test_multi_line_chain_with_shared_capital() {
            // Line 1: wood (Consumed) + saw (Capital) → planks
            // Line 2: planks (Consumed) + saw (Capital) → furniture
            let sawmill = Process::new(10, "sawmill", 0)
                .with_input(ProcessInput::new(100, 1.0, true, InputType::Destroyed, false)) // wood
                .with_input(ProcessInput::new(200, 1.0, true, InputType::Capital, false))  // saw
                .with_output(ProcessOutput::new(110, 1.0, true)); // planks

            let workshop = Process::new(11, "workshop", 0)
                .with_input(ProcessInput::new(110, 1.0, true, InputType::Destroyed, false)) // planks
                .with_input(ProcessInput::new(200, 1.0, true, InputType::Capital, false))  // same saw
                .with_output(ProcessOutput::new(120, 1.0, true)); // furniture

            let mut factuals = make_factuals_with_process(sawmill);
            factuals.processes.insert(11, workshop);
            factuals.goods.insert(100, make_good(100, "wood", HashMap::new()));
            factuals.goods.insert(110, make_good(110, "plank", HashMap::new()));
            factuals.goods.insert(120, make_good(120, "table", HashMap::new()));
            factuals.goods.insert(200, make_good(200, "saw", HashMap::new()));

            let mut firm = Firm::new(1, "Integrated Workshop".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(100, empty_firm_row(20.0)); // wood
            firm.property.insert(200, empty_firm_row(20.0));  // saw (shared capital)
            firm.property.insert(110, empty_firm_row(0.0));  // planks (will be produced then consumed)

            // Two lines in priority order
            firm.production_line.push(empty_production_line(10)); // sawmill
            firm.production_line[0].inputs = vec![100, 200];
            firm.production_line[0].target = Some(5.0);

            firm.production_line.push(empty_production_line(11)); // workshop
            firm.production_line[1].inputs = vec![110, 200];
            firm.production_line[1].target = Some(3.0);

            let market = make_market_with_amvs(&[(100, 2.0), (110, 5.0), (120, 15.0), (200, 50.0)]);

            firm.run_production(&factuals, &market);

            // Property assertions
            assert_eq!(firm.property[&100].quantity, 15.0);   // 20 - 5
            assert_eq!(firm.property[&110].quantity, 0.0);
            assert_eq!(firm.property[&110].held, 2.0); // produced 5, consumed 3 from held 
            assert_eq!(firm.property[&200].used, 8.0); // 5 + 3
            assert_eq!(firm.property[&200].consumed, 0.0); // capital never in consumed
            assert_eq!(firm.property[&200].quantity, 12.0);    // 20- 5 - 3
            // (adjust expected numbers based on exact per-iter costs you want)

            // Row day-flows aggregated across both lines
            assert_eq!(firm.property[&110].produced, 5.0); // planks created
            assert_eq!(firm.property[&110].consumed, 3.0); // planks consumed in line 2
            assert_eq!(firm.property[&120].produced, 3.0); // tables created
            assert_eq!(firm.property[&120].held, 3.0);
            assert_eq!(firm.property[&120].quantity, 0.0);
            assert_eq!(firm.property[&100].consumed, 5.0); // wood

            // Both lines recorded AMV snapshots
            assert_eq!(firm.production_line[0].last_amv_consumed, 10.0);
            assert_eq!(firm.production_line[0].last_amv_produced, 25.0);
            assert_eq!(firm.production_line[1].last_amv_consumed, 15.0);
            assert_eq!(firm.production_line[1].last_amv_produced, 45.0);
        }

        #[test]
        fn test_required_and_optional_factors() {
            // Required factor (water) + optional factor (skilled labor bonus)
            let process = Process::new(20, "factor_test", 0)
                .with_input(ProcessInput::new(100, 1.0, true, InputType::Destroyed, false))
                .with_input(ProcessInput::new(110, 1.0, false, InputType::Destroyed, false))
                .with_input(ProcessInput::new(300, 1.0, true, InputType::Factor, false)) // required water
                .with_input(ProcessInput::new(301, 1.0, true, InputType::Factor, true)   // optional skilled
                    .with_optional(InputEffect::Throughput(0.5)))
                .with_output(ProcessOutput::new(120, 1.0, false));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(100, make_good(100, "wood", HashMap::new()));
            factuals.goods.insert(110, make_good(110, "planks", HashMap::new()));
            factuals.goods.insert(120, make_good(120, "ash", HashMap::new()));
            factuals.goods.insert(300, make_good(300, "sunlight", HashMap::new()));
            factuals.goods.insert(301, make_good(301, "clear skys", HashMap::new()));

            let mut firm = Firm::new(2, "Factor Firm".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(100, empty_firm_row(20.0));
            firm.property.insert(110, empty_firm_row(40.0));
            firm.property.insert(300, empty_firm_row(1.0)); // has required factor
            // 301 (skilled) deliberately missing

            firm.production_line.push(empty_production_line(20));
            firm.production_line[0].inputs = vec![100, 110, 300, 301];
            firm.production_line[0].target = None;

            let market = make_market_with_amvs(&[(100, 2.0), (110, 6.0), (120, 20.0)]);

            firm.run_production(&factuals, &market);

            // Should run (required factor present) but without the optional throughput bonus
            assert!(firm.production_line[0].last_success_rate > 0.9);
            assert_eq!(firm.production_line[0].last_iterations, 20.0);
            assert_eq!(firm.production_line[0].last_missing_goods.len(), 1);
            assert!(firm.production_line[0].last_missing_goods.contains(&100));
            assert_eq!(firm.production_line[0].last_amv_consumed, 160.0);
            assert_eq!(firm.production_line[0].last_amv_produced, 400.0);
            assert_eq!(firm.property[&100].consumed, 20.0);
            assert_eq!(firm.property[&110].consumed, 20.0);
            assert_eq!(firm.property[&120].produced, 20.0);
            // Factors are present but not consumed, used, or locked.
            assert_eq!(firm.property[&300].quantity, 1.0);
            assert_eq!(firm.property[&300].consumed, 0.0);
            assert_eq!(firm.property[&300].used, 0.0);

            // test with optional factor included
            firm.property.insert(301, empty_firm_row(1.0));
            firm.property.get_mut(&100).unwrap().quantity += 20.0;
            firm.property.get_mut(&100).unwrap().consumed = 0.0;
            firm.property.get_mut(&110).unwrap().quantity += 20.0;
            firm.property.get_mut(&110).unwrap().consumed = 0.0;
            firm.property.get_mut(&120).unwrap().produced = 0.0;
            firm.production_line[0].last_amv_consumed = 0.0;
            firm.production_line[0].last_amv_produced = 0.0;
            firm.production_line[0].last_iterations = 0.0;
            firm.production_line[0].last_success_rate = 0.0;

            firm.run_production(&factuals, &market);

            // Should run (required factor present) but without the optional throughput bonus
            assert!(firm.production_line[0].last_success_rate > 0.9);
            assert_eq!(firm.production_line[0].last_iterations, 20.0);
            assert_eq!(firm.production_line[0].last_missing_goods.len(), 1);
            assert!(firm.production_line[0].last_missing_goods.contains(&100));
            assert_eq!(firm.production_line[0].last_amv_consumed, 220.0);
            assert_eq!(firm.production_line[0].last_amv_produced, 600.0);
            assert_eq!(firm.property[&100].consumed, 20.0);
            assert_eq!(firm.property[&110].consumed, 30.0);
            assert_eq!(firm.property[&120].produced, 30.0);
            assert_eq!(firm.property[&301].quantity, 1.0);
            assert_eq!(firm.property[&301].consumed, 0.0);
            assert_eq!(firm.property[&301].used, 0.0);
        }

        #[test]
        fn test_optional_inputs_and_bonuses() {
            let process = Process::new(30, "optional_bonus", 0)
                .with_input(ProcessInput::new(100, 1.0, true, InputType::Destroyed, false))
                .with_input(ProcessInput::new(400, 1.0, true, InputType::Destroyed, true) // optional catalyst
                    .with_optional(InputEffect::Output(0.25))) // +25% output
                .with_output(ProcessOutput::new(110, 1.0, false));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(100, make_good(100, "wood", HashMap::new()));
            factuals.goods.insert(400, make_good(400, "ash", HashMap::new()));
            factuals.goods.insert(110, make_good(110, "treated wood", HashMap::new()));

            let mut firm = Firm::new(3, "Catalyst Tester".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(100, empty_firm_row(10.0));
            firm.property.insert(400, empty_firm_row(3.0)); // present → bonus applies

            firm.production_line.push(empty_production_line(30));
            firm.production_line[0].inputs = vec![100, 400];
            firm.production_line[0].target = None;

            let market = make_market_with_amvs(&[(100, 2.0), (110, 7.0), (400, 10.0)]);

            firm.run_production(&factuals, &market);

            // With catalyst bonus we should get more than the base 5 iterations worth of output
            assert_eq!(firm.production_line[0].last_iterations, 10.0);
            assert_eq!(firm.production_line[0].last_amv_consumed, 50.0);
            assert_eq!(firm.production_line[0].last_amv_produced, 75.25);
            assert_eq!(firm.property[&100].consumed, 10.0);
            assert_eq!(firm.property[&400].consumed, 3.0);
            assert_eq!(firm.property[&110].produced, 10.75);
        }

        #[test]
        fn test_decay_results_recorded_in_produced() {
            // Wood (Consumed) decays into sawdust
            let process = Process::new(40, "decay_test", 0)
                .with_input(ProcessInput::new(100, 1.0, true, InputType::Consumed, false))
                .with_output(ProcessOutput::new(110, 1.0, true));

            let mut factuals = make_factuals_with_process(process);
            // Add decay info to the good definition (even if goods map is mostly empty)
            let wood = Good {
                id: 100,
                name: "Wood".into(),
                class: None,
                mass: 1.0,
                volume: 1.0,
                decay_rate: 0.25,
                decay_result: HashMap::from([(130, 0.5)]), // 50% becomes sawdust
                tags: Default::default(),
                categories: vec![],
            };
            factuals.goods.insert(100, wood);
            factuals.goods.insert(130, make_good(110, "nice wood", HashMap::new()));
            factuals.goods.insert(130, make_good(130, "ash", HashMap::new()));

            let mut firm = Firm::new(4, "Decay Workshop".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(100, empty_firm_row(8.0));

            firm.production_line.push(empty_production_line(40));
            firm.production_line[0].inputs = vec![100];
            firm.production_line[0].target = None;

            let market = make_market_with_amvs(&[(100, 2.0), (110, 6.0), (130, 0.5)]);

            firm.run_production(&factuals, &market);

            assert_eq!(firm.property[&110].produced, 8.0);  // main output
            assert_eq!(firm.property[&130].produced, 4.0);  // decay result (8 iters * 0.5)
            assert_eq!(firm.property[&100].consumed, 8.0);
            assert_eq!(firm.production_line[0].last_amv_consumed, 16.0);
            assert_eq!(firm.production_line[0].last_amv_produced, 50.0);
            assert_eq!(firm.production_line[0].last_iterations, 8.0);
        }

        #[test]
        fn test_target_with_throughput_bonus_overshoot() {
            // Throughput bonus from optional input should allow more iterations than target
            // (per do_process rules: target is scaled on fixed inputs only)
            let process = Process::new(50, "throughput_target", 0)
                .with_input(ProcessInput::new(100, 1.0, true, InputType::Destroyed, false))
                .with_input(ProcessInput::new(110, 1.0, false, InputType::Destroyed, false))
                .with_input(ProcessInput::new(500, 1.0, true, InputType::Destroyed, true)
                    .with_optional(InputEffect::Throughput(1.0))) // doubles throughput
                .with_output(ProcessOutput::new(120, 1.0, true))
                .with_output(ProcessOutput::new(130, 1.0, false));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(100, make_good(100, "fixed good", HashMap::new()));
            factuals.goods.insert(110, make_good(110, "normal good", HashMap::new()));
            factuals.goods.insert(120, make_good(120, "fixed output", HashMap::new()));
            factuals.goods.insert(130, make_good(130, "normal output", HashMap::new()));
            factuals.goods.insert(500, make_good(500, "bonus good", HashMap::new()));

            let mut firm = Firm::new(5, "Throughput Lab".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(100, empty_firm_row(20.0));
            firm.property.insert(110, empty_firm_row(40.0));
            firm.property.insert(500, empty_firm_row(5.0)); // enough for bonus

            firm.production_line.push(empty_production_line(50));
            firm.production_line[0].inputs = vec![100, 110, 500];
            firm.production_line[0].target = Some(8.0); // would be 8 without bonus, more with it

            let market = make_market_with_amvs(&[(100, 2.0), (110, 3.0), (120, 10.0), (130, 5.0), (500, 1.0)]);

            firm.run_production(&factuals, &market);

            assert_eq!(firm.property[&120].produced, 8.0);  // main output
            assert_eq!(firm.property[&130].produced, 13.0);
            assert_eq!(firm.property[&100].consumed, 8.0);
            assert_eq!(firm.property[&110].consumed, 13.0);
            assert_eq!(firm.property[&500].consumed, 5.0);
            assert_eq!(firm.production_line[0].last_amv_consumed, 2.0*8.0 + 3.0*13.0 + 5.0*1.0);
            assert_eq!(firm.production_line[0].last_amv_produced, 8.0*10.0 + 13.0*5.0);
            assert_eq!(firm.production_line[0].last_iterations, 8.0);
            assert_eq!(firm.property[&100].quantity, 12.0);
            assert_eq!(firm.property[&110].quantity, 27.0);
            assert_eq!(firm.property[&120].held, 8.0);
            assert_eq!(firm.property[&130].held, 13.0);
            assert_eq!(firm.property[&120].quantity, 0.0);
            assert_eq!(firm.property[&130].quantity, 0.0);
            assert_eq!(firm.property[&500].quantity, 0.0);
        }

        #[test]
        fn test_amv_fallback_uses_one_point_zero() {
            // Good 999 is deliberately missing from the Market
            let process = Process::new(60, "missing_good_amv", 0)
                .with_input(ProcessInput::new(999, 1.0, true, InputType::Consumed, false))
                .with_output(ProcessOutput::new(110, 1.0, true));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(999, make_good(999, "missing market good", HashMap::new()));
            factuals.goods.insert(110, make_good(110, "output good", HashMap::new()));

            let mut firm = Firm::new(6, "Mystery Good Firm".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(999, empty_firm_row(5.0));

            firm.production_line.push(empty_production_line(60));
            firm.production_line[0].inputs = vec![999];
            firm.production_line[0].target = None;

            // Market does NOT contain good 999
            let market = make_market_with_amvs(&[(110, 4.0)]);

            firm.run_production(&factuals, &market);

            // Should fall back to the economic default of 1.0
            assert_eq!(
                firm.production_line[0].last_amv_consumed, 5.0,
                "Missing goods should default to AMV 1.0"
            );
            assert_eq!(firm.property[&999].consumed, 5.0);
            assert_eq!(firm.property[&110].produced, 5.0);
        }

        #[test]
        fn prefers_on_hand_quantity_before_held() {
            let process = Process::new(1, "finish", 0)
                .with_input(ProcessInput::new(20, 1.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(30, 1.0, true));
            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(20, make_good(20, "plank", HashMap::new()));
            factuals.goods.insert(30, make_good(30, "table", HashMap::new()));

            let mut firm = Firm::new(1, "Joinery".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(
                20,
                FirmPRow::new().with_quantity(3.0).with_held(5.0),
            );
            firm.production_line.push(empty_production_line(1));
            firm.production_line[0].inputs = vec![20];
            firm.production_line[0].target = Some(4.0);

            let market = make_market_with_amvs(&[(20, 1.0), (30, 1.0)]);
            firm.run_production(&factuals, &market);

            assert_eq!(firm.property[&20].quantity, 0.0);
            assert_eq!(firm.property[&20].held, 4.0);
            assert_eq!(firm.property[&20].consumed, 4.0);
            assert_eq!(firm.property[&30].held, 4.0);
            assert_eq!(firm.property[&30].quantity, 0.0);
        }
    }

    mod firm_prow_should {
        use super::*;

        #[test]
        fn available_is_quantity_minus_reserve() {
            let row = FirmPRow::new().with_quantity(10.0).with_reserve(4.0);
            assert_eq!(row.available(), 6.0);
        }

        #[test]
        fn sellable_uses_the_larger_of_reserve_and_reserve_target() {
            let live_higher = FirmPRow::new()
                .with_quantity(20.0)
                .with_reserve(6.0)
                .with_reserve_target(5.0);
            assert_eq!(live_higher.sellable(), 14.0);

            let target_higher = FirmPRow::new()
                .with_quantity(20.0)
                .with_reserve(3.0)
                .with_reserve_target(8.0);
            assert_eq!(target_higher.sellable(), 12.0);
        }

        #[test]
        fn sellable_floors_at_zero() {
            let row = FirmPRow::new()
                .with_quantity(3.0)
                .with_reserve(1.0)
                .with_reserve_target(10.0);
            assert_eq!(row.sellable(), 0.0);
        }

        #[test]
        fn sync_reserve_matches_min_of_quantity_and_target() {
            let mut row = FirmPRow::new()
                .with_quantity(20.0)
                .with_reserve(1.0)
                .with_reserve_target(5.0);
            row.sync_reserve();
            assert_eq!(row.reserve, 5.0);
            assert_eq!(row.sellable(), 15.0);

            row.quantity = 3.0;
            row.sync_reserve();
            assert_eq!(row.reserve, 3.0);
            assert_eq!(row.sellable(), 0.0);
        }

        #[test]
        fn blend_average_cost_weights_old_stock_and_new_units() {
            let mut row = FirmPRow::new()
                .with_quantity(5.0)
                .with_average_cost(4.0);
            row.quantity = 10.0;
            row.blend_average_cost(5.0, 10.0);
            assert_eq!(row.average_cost, 7.0);
        }

        #[test]
        fn bought_unit_amv_divides_total_spend_by_units() {
            let row = FirmPRow::new().with_bought(4.0).with_bought_amv(10.0);
            assert_eq!(row.bought_unit_amv(), 2.5);
        }

        #[test]
        fn sold_unit_amv_is_zero_when_nothing_sold() {
            let row = FirmPRow::new().with_sold_amv(99.0);
            assert_eq!(row.sold_unit_amv(), 0.0);
        }

        #[test]
        fn sold_unit_amv_divides_total_by_units() {
            let row = FirmPRow::new().with_sold(2.0).with_sold_amv(9.0);
            assert_eq!(row.sold_unit_amv(), 4.5);
        }

        #[test]
        fn dual_sided_row_splits_mid_by_margin() {
            let row = FirmPRow::new()
                .with_purchase_target(1.0)
                .with_sell_target(1.0)
                .with_margin(0.2);
            assert!((row.bid_amv(10.0) - 8.0).abs() < 1e-12);
            assert!((row.ask_amv(10.0) - 12.0).abs() < 1e-12);
        }

        #[test]
        fn one_sided_row_uses_mid_as_bid_and_ask() {
            let buy_only = FirmPRow::new().with_purchase_target(1.0).with_margin(0.2);
            assert_eq!(buy_only.bid_amv(10.0), 10.0);
            assert_eq!(buy_only.ask_amv(10.0), 10.0);
        }

        #[test]
        fn purchase_qty_caps_producer_inputs_at_stock_shortfall() {
            let row = FirmPRow::new()
                .with_quantity(4.0)
                .with_purchase_target(8.0)
                .with_use_target(5.0)
                .with_stock_target(10.0);
            assert_eq!(row.purchase_qty(), 6.0);

            let full = row.with_quantity(12.0);
            assert_eq!(full.purchase_qty(), 0.0);
        }

        #[test]
        fn purchase_qty_lets_merchants_buy_the_full_target() {
            let row = FirmPRow::new()
                .with_quantity(20.0)
                .with_purchase_target(8.0)
                .with_stock_target(20.0);
            assert_eq!(row.purchase_qty(), 8.0);
        }

        #[test]
        fn free_for_market_fences_producer_stock_and_use() {
            let row = FirmPRow::new()
                .with_quantity(15.0)
                .with_use_target(5.0)
                .with_stock_target(10.0);
            assert_eq!(row.free_for_market(), 5.0);
        }

        #[test]
        fn free_for_market_includes_held() {
            let mut row = FirmPRow::new()
                .with_quantity(2.0)
                .with_use_target(5.0)
                .with_stock_target(5.0);
            row.held = 8.0;
            assert_eq!(row.free_for_market(), 5.0);
        }

        #[test]
        fn profit_spendable_leaves_the_sell_plan() {
            let row = FirmPRow::new()
                .with_quantity(20.0)
                .with_stock_target(2.0)
                .with_growth_target(3.0)
                .with_sell_target(10.0);
            assert_eq!(row.profit_spendable(), 5.0);
        }

        #[test]
        fn amv_bound_defaults_to_none() {
            let row = FirmPRow::new();
            assert_eq!(row.amv_bound, FirmAmvBound::None);
            assert_eq!(row.amv_bound.minimum(), None);
            assert_eq!(row.amv_bound.maximum(), None);
        }

        #[test]
        fn amv_bound_minimum_is_sell_floor_only() {
            let row = FirmPRow::new().with_amv_bound(FirmAmvBound::Minimum(25.0));
            assert_eq!(row.amv_bound.minimum(), Some(25.0));
            assert_eq!(row.amv_bound.maximum(), None);
        }

        #[test]
        fn amv_bound_maximum_is_buy_cap_only() {
            let row = FirmPRow::new().with_amv_bound(FirmAmvBound::Maximum(22.5));
            assert_eq!(row.amv_bound.minimum(), None);
            assert_eq!(row.amv_bound.maximum(), Some(22.5));
        }

        #[test]
        fn amv_bound_minmax_keeps_both() {
            let row = FirmPRow::new().with_amv_bound(FirmAmvBound::MinMax(25.0, 40.0));
            assert_eq!(row.amv_bound.minimum(), Some(25.0));
            assert_eq!(row.amv_bound.maximum(), Some(40.0));
        }

        #[test]
        fn amv_bound_from_parts_matches_the_four_shapes() {
            assert_eq!(FirmAmvBound::from_parts(None, None), FirmAmvBound::None);
            assert_eq!(
                FirmAmvBound::from_parts(Some(2.0), None),
                FirmAmvBound::Minimum(2.0)
            );
            assert_eq!(
                FirmAmvBound::from_parts(None, Some(3.0)),
                FirmAmvBound::Maximum(3.0)
            );
            assert_eq!(
                FirmAmvBound::from_parts(Some(2.0), Some(3.0)),
                FirmAmvBound::MinMax(2.0, 3.0)
            );
        }

        #[test]
        fn amv_bound_clamps_bid_down_and_ask_up() {
            let cap = FirmAmvBound::Maximum(22.5);
            assert_eq!(cap.clamp_bid(40.0), 22.5);
            assert_eq!(cap.clamp_bid(10.0), 10.0);
            assert!(!cap.market_above_buy_cap(10.0));
            assert!(cap.market_above_buy_cap(22.6));

            let floor = FirmAmvBound::Minimum(25.0);
            assert_eq!(floor.clamp_ask(20.0), 25.0);
            assert_eq!(floor.clamp_ask(30.0), 30.0);
            assert_eq!(floor.clamp_bid(40.0), 40.0);

            let both = FirmAmvBound::MinMax(25.0, 22.5);
            assert_eq!(both.clamp_bid(40.0), 22.5);
            assert_eq!(both.clamp_ask(20.0), 25.0);
        }
    }

    mod decay_goods_should {
        use super::*;
        use crate::game::good::GoodTag;

        #[test]
        fn returns_used_then_decays_quantity_with_byproducts() {
            let mut wood = make_good(10, "wood", HashMap::from([(11, 0.5)]));
            wood.decay_rate = 0.2;
            let mut factuals = Factuals::new();
            factuals.goods.insert(10, wood);
            factuals.goods.insert(11, make_good(11, "ash", HashMap::new()));

            let mut firm = Firm::new(1, "Yard".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_quantity(10.0)
                    .with_used(5.0)
                    .with_reserve_target(20.0),
            );

            firm.decay_goods(&factuals);

            // used 5 returned -> 15, then 20% decay -> 12, ash 1.5 (lost 3 * 0.5).
            assert_eq!(firm.property[&10].used, 0.0);
            assert_eq!(firm.property[&10].quantity, 12.0);
            assert_eq!(firm.property[&10].reserve, 12.0);
            assert_eq!(firm.property[&11].quantity, 1.5);
        }

        #[test]
        fn does_not_destroy_consumed_counter_as_stock() {
            let mut factuals = Factuals::new();
            factuals.goods.insert(
                10,
                make_good(10, "wood", HashMap::from([(11, 1.0)])),
            );

            let mut firm = Firm::new(1, "Yard".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(
                10,
                FirmPRow::new().with_quantity(4.0).with_consumed(10.0),
            );
            firm.decay_goods(&factuals);

            assert_eq!(firm.property[&10].quantity, 4.0);
            assert_eq!(firm.property[&10].consumed, 10.0);
            assert!(!firm.property.contains_key(&11));
        }

        #[test]
        fn skips_exposure_decay_while_owned() {
            let mut land = make_good(10, "land", HashMap::new());
            land.decay_rate = 1.0;
            land.tags.insert(GoodTag::Exposure);
            let mut factuals = Factuals::new();
            factuals.goods.insert(10, land);

            let mut firm = Firm::new(1, "Farm".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(10, FirmPRow::new().with_quantity(8.0));
            firm.decay_goods(&factuals);

            assert_eq!(firm.property[&10].quantity, 8.0);
        }

        #[test]
        fn reports_volume_as_on_hand_plus_consumed() {
            let mut factuals = Factuals::new();
            factuals.goods.insert(10, make_good(10, "wood", HashMap::new()));

            let mut firm = Firm::new(1, "Yard".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(
                10,
                FirmPRow::new().with_quantity(4.0).with_consumed(10.0),
            );
            let rot = firm.decay_goods(&factuals);

            assert_eq!(rot[&10], (0.0, 14.0));
        }

        #[test]
        fn reports_leftover_rot_after_used_returns() {
            let mut wood = make_good(10, "wood", HashMap::new());
            wood.decay_rate = 0.2;
            let mut factuals = Factuals::new();
            factuals.goods.insert(10, wood);

            let mut firm = Firm::new(1, "Yard".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(
                10,
                FirmPRow::new().with_quantity(10.0).with_used(5.0),
            );
            let rot = firm.decay_goods(&factuals);

            // used 5 returned -> 15 on hand, 20% decay -> lost 3, volume 15.
            assert_eq!(rot[&10], (3.0, 15.0));
        }

        #[test]
        fn releases_held_after_quantity_has_decayed() {
            let mut wood = make_good(10, "wood", HashMap::new());
            wood.decay_rate = 0.5;
            let mut factuals = Factuals::new();
            factuals.goods.insert(10, wood);

            let mut firm = Firm::new(1, "Yard".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(
                10,
                FirmPRow::new().with_quantity(10.0).with_held(8.0),
            );
            let rot = firm.decay_goods(&factuals);

            // quantity 10 decays 50% -> 5, then held 8 joins. held did not rot.
            assert_eq!(firm.property[&10].quantity, 13.0);
            assert_eq!(firm.property[&10].held, 0.0);
            assert_eq!(rot[&10], (5.0, 10.0));
        }
    }

    mod operations_hold_days_should {
        use super::*;

        #[test]
        fn durables_overshoot_so_cover_survives_decay() {
            let days = FirmPRow::operations_hold_days(5.0, 0.01);
            assert!((days - 5.0 / 0.99).abs() < 1e-12);
        }

        #[test]
        fn perishables_shrink_instead_of_fighting_rot() {
            let days = FirmPRow::operations_hold_days(5.0, 0.4);
            assert!((days - 3.0).abs() < 1e-12);
        }

        #[test]
        fn full_decay_holds_nothing() {
            assert_eq!(FirmPRow::operations_hold_days(5.0, 1.0), 0.0);
        }
    }

    mod clear_day_flows_should {
        use super::*;

        #[test]
        fn zeros_counters_and_keeps_stock_cost_and_used() {
            let mut firm = Firm::new(1, "Shop".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_quantity(7.0)
                    .with_used(2.0)
                    .with_produced(3.0)
                    .with_consumed(4.0)
                    .with_bought(1.0)
                    .with_bought_amv(5.0)
                    .with_sold(2.0)
                    .with_sold_amv(8.0)
                    .with_placed(3.0)
                    .with_placed_amv(9.0)
                    .with_average_cost(3.0)
                    .with_reserve_target(1.0)
                    .with_reserve(1.0),
            );
            firm.property.get_mut(&10).unwrap().sold_avg = 2.0;
            firm.property.get_mut(&10).unwrap().sell_fills_avg = 1.0;

            firm.clear_property_records();

            let row = &firm.property[&10];
            assert_eq!(row.produced, 0.0);
            assert_eq!(row.consumed, 0.0);
            assert_eq!(row.bought, 0.0);
            assert_eq!(row.bought_amv, 0.0);
            assert_eq!(row.sold, 0.0);
            assert_eq!(row.sold_amv, 0.0);
            assert_eq!(row.placed, 0.0);
            assert_eq!(row.placed_amv, 0.0);
            assert_eq!(row.sell_fills, 0.0);
            assert_eq!(row.sell_rejects, 0.0);
            assert_eq!(row.sell_no_proposal, 0.0);
            assert_eq!(row.sold_avg, 2.0);
            assert_eq!(row.sell_fills_avg, 1.0);
            assert_eq!(row.quantity, 7.0);
            assert_eq!(row.used, 2.0);
            assert_eq!(row.average_cost, 3.0);
            assert_eq!(row.reserve, 1.0);
        }
    }

    mod posted_sell_qty_should {
        use super::*;
        use crate::game::market::MarketHistory;

        fn history_sal(pairs: &[(usize, f64)]) -> MarketHistory {
            let mut history = MarketHistory::new();
            for &(id, sal) in pairs {
                history.prices.insert(id, 1.0);
                history.salability.insert(id, sal);
            }
            history
        }

        #[test]
        fn caps_produced_goods_by_max_sal_times_daily_output() {
            let process = Process::new(1, "mill", 0)
                .with_input(ProcessInput::new(10, 1.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(20, 15.0, true));
            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(10, make_good(10, "wood", HashMap::new()));
            factuals.goods.insert(20, make_good(20, "plank", HashMap::new()));
            let mut firm = Firm::new(1, "mill".into(), 42, hexx::Hex::new(0, 0));
            let mut line = empty_production_line(1);
            line.target = Some(10.0);
            firm.production_line.push(line);
            firm.property.insert(20, FirmPRow::new().with_sell_target(150.0));
            let history = history_sal(&[(20, 0.3)]);
            assert!((firm.daily_output(20, &factuals) - 150.0).abs() < 1e-12);
            assert!((firm.posted_sell_qty(20, &history, &factuals) - 45.0).abs() < 1e-12);
        }

        #[test]
        fn posts_full_sell_target_when_excess_beats_the_sal_cap() {
            let process = Process::new(1, "mill", 0)
                .with_input(ProcessInput::new(10, 1.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(20, 15.0, true));
            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(10, make_good(10, "wood", HashMap::new()));
            factuals.goods.insert(20, make_good(20, "plank", HashMap::new()));
            let mut firm = Firm::new(1, "mill".into(), 42, hexx::Hex::new(0, 0));
            let mut line = empty_production_line(1);
            line.target = Some(10.0);
            firm.production_line.push(line);
            firm.property.insert(
                20,
                FirmPRow::new().with_sell_target(150.0).with_quantity(400.0),
            );
            let history = history_sal(&[(20, 0.3)]);
            assert!((firm.posted_sell_qty(20, &history, &factuals) - 150.0).abs() < 1e-12);
        }

        #[test]
        fn leaves_sell_target_when_the_firm_does_not_make_the_good() {
            let factuals = Factuals::new();
            let mut firm = Firm::new(1, "shop".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(5, FirmPRow::new().with_sell_target(20.0));
            let history = history_sal(&[(5, 0.3)]);
            assert!((firm.posted_sell_qty(5, &history, &factuals) - 20.0).abs() < 1e-12);
        }
    }

    mod take_good_should {
        use super::*;

        #[test]
        fn returns_quantity_and_removes_the_row() {
            let mut firm = Firm::new(7, "Shop".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_quantity(2.3)
                    .with_purchase_target(4.0),
            );
            firm.property.insert(11, FirmPRow::new().with_quantity(5.0));
            assert!((firm.take_good(10) - 2.3).abs() < 1e-12);
            assert!(!firm.property.contains_key(&10));
            assert_eq!(firm.property[&11].quantity, 5.0);
        }

        #[test]
        fn returns_zero_when_the_good_is_not_held() {
            let mut firm = Firm::new(7, "Shop".into(), 42, hexx::Hex::new(0, 0));
            assert_eq!(firm.take_good(10), 0.0);
        }
    }
}
