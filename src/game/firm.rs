use std::collections::HashMap;

mod plan;
mod orders;

use hexx::Hex;

use crate::game::{
    actor::Actor, config::GameConfig, contract::Contract, deal::{
        collect_tenders, deal_goods_tradeable, evaluate_amv_floor, form_buy_proposal,
        transport_cover_on_hand, transport_spend_plan, with_transport_budget, DealMaker,
        DealResponse, DealRole, ProposedDeal,
    }, factuals::Factuals, firmorganization::FirmOrganization, good::{GoodTag, TIME}, market::{Market, MarketHistory}, marketorder::MarketOrder, pop::Pop, process::ProcessEffect, util::{whole_units, whole_units_up}, workforce::{LaborSettlement, Workforce},
};

/// # Firm 
/// 
/// A firm is the smallest unit of business. It deals with Production and local economic
/// calculation
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
    /// Transport units spent this day (buyer haul). Cleared at day start.
    /// Labor budget uses this plus 1 as the next-day Time buffer.
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
    /// Total AMV spent on purchases today.
    pub bought_amv: f64,
    /// Cost basis of units sold today (`sold * average_cost` per row).
    pub sold_cost_amv: f64,
    /// Realized profit today: `sold_amv / sold_cost_amv`. 1.0 if unknown.
    pub profit_ratio: f64,
    /// EMA of [`Self::profit_ratio`].
    pub profit_avg: f64,
    /// Firm-wide sell success today (`sold / sell_target` over rows with a sell plan).
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

    /// # Pay Wage Shares
    ///
    /// Pays a share of on-hand `coin` to owners and workers. Stand-in for
    /// wage contracts: living owners get [`labor_constants::OWNER_SHARE`],
    /// workers get [`labor_constants::WORKER_SHARE`], both rounded up to
    /// whole units. Owners are paid first; workers split what remains of
    /// their share.
    ///
    /// A producer with no process inputs (mine, well) pays the whole till,
    /// split in the same owner:worker ratio, so coin does not pool there.
    ///
    /// Owner `Actor::Pop(0)` (none / blank) or a missing owner pop is not
    /// paid and does not drain the till. Worker pops missing from `pops`
    /// are skipped and that slice stays in the firm. Credits
    /// `PopRecords::income_amv` at `coin_amv` per unit.
    pub fn pay_wage_shares(
        &mut self,
        pops: &mut HashMap<usize, Pop>,
        coin: usize,
        coin_amv: f64,
        config: &GameConfig,
    ) -> WagePayout {
        let coinage = self
            .property
            .get(&coin)
            .map(|row| row.quantity.max(0.0))
            .unwrap_or(0.0);
        let mut payout = WagePayout::empty(self.owners.owner, coinage);
        if coinage <= 0.0 {
            return payout;
        }

        let (owner_frac, worker_frac) = self.wage_share_fracs(config);
        let owner_want = whole_units_up(coinage * owner_frac);
        let worker_want = whole_units_up(coinage * worker_frac);
        let mut remaining = coinage;

        let owner_pay = owner_want.min(remaining);
        if owner_pay > 0.0 {
            if let Some(pop_id) = self.owners.pop_id() {
                if let Some(pop) = pops.get_mut(&pop_id) {
                    remaining -= owner_pay;
                    payout.owner_amount = owner_pay;
                    pop.credit_good(coin, owner_pay, coin_amv);
                    payout.owner_credited = true;
                }
            }
        }

        let worker_pay = worker_want.min(remaining);
        if worker_pay > 0.0 {
            let mut recipients: Vec<usize> = self
                .workforce
                .iter()
                .map(|w| w.id)
                .filter(|id| *id != 0 && pops.contains_key(id))
                .collect();
            recipients.sort_unstable();
            recipients.dedup();
            if !recipients.is_empty() {
                remaining -= worker_pay;
                payout.worker_amount = worker_pay;
                let n = recipients.len() as f64;
                let mut left = worker_pay;
                for (i, pop_id) in recipients.iter().copied().enumerate() {
                    let share = if i + 1 == recipients.len() {
                        left
                    } else {
                        whole_units(worker_pay / n).min(left)
                    };
                    if share <= 0.0 {
                        continue;
                    }
                    left -= share;
                    if let Some(pop) = pops.get_mut(&pop_id) {
                        pop.credit_good(coin, share, coin_amv);
                        payout.workers.push((pop_id, share));
                    }
                }
            }
        }

        let paid = coinage - remaining;
        if paid > 0.0 {
            if let Some(row) = self.property.get_mut(&coin) {
                row.quantity = remaining;
                row.sync_reserve();
            }
        }
        payout
    }

    /// # Settle Labor Contracts
    ///
    /// Delegates to [`LaborSettlement::settle`].
    pub fn settle_labor_contracts(
        &mut self,
        pops: &mut HashMap<usize, Pop>,
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> LaborSettlement {
        LaborSettlement::settle(self, pops, history, factuals)
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
        self.owners.remainder = false;
        self
    }

    /// Marks the owner as the residual claimant (owner-operator).
    /// Leftover after wages, worker shares, stock fence, and growth.
    pub fn with_owner_remainder(mut self) -> Self {
        self.owners.remainder = true;
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

    /// # Clear Day Flows
    ///
    /// Zero today's exchange and production counters on every property row:
    /// `produced`, `consumed`, `bought`, `bought_amv`, `sold`, `sold_amv`.
    ///
    /// Leaves `used` and `held` alone (`decay_goods` returns them) and does not
    /// touch cost basis, prices, or planning targets.
    ///
    /// Intended for day start so the previous day's totals stay visible overnight.
    /// Safe to call from a later phase if we want that window longer.
    pub fn clear_day_flows(&mut self) {
        for row in self.property.values_mut() {
            row.produced = 0.0;
            row.consumed = 0.0;
            row.bought = 0.0;
            row.bought_amv = 0.0;
            row.sold = 0.0;
            row.sold_amv = 0.0;
        }
        self.transport_spent = 0.0;
    }

    /// # Scale AMV Unit
    ///
    /// Multiplies AMV-denominated quotes, cost basis, and today's AMV
    /// totals by `scale`. Same factor as market AMV rescale.
    /// `amv_target` 0 stays unset. No-op when `scale` is 1.
    pub fn scale_amv_unit(&mut self, scale: f64) {
        debug_assert!(scale.is_finite() && scale > 0.0, "scale must be finite and > 0.0");
        if !scale.is_finite() || scale <= 0.0 || (scale - 1.0).abs() < 1e-15 {
            return;
        }
        for row in self.property.values_mut() {
            if row.amv_target != 0.0 {
                row.amv_target *= scale;
            }
            row.average_cost *= scale;
            row.average_price *= scale;
            row.bought_amv *= scale;
            row.sold_amv *= scale;
            row.amv_bound = row.amv_bound.scaled(scale);
        }
        for line in &mut self.production_line {
            line.last_amv_consumed *= scale;
            line.last_amv_produced *= scale;
        }
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

    /// Sets the owning actor. `Actor::Pop(0)` is none and is not paid.
    pub fn with_owner(mut self, owner: Actor) -> Self {
        self.owners.owner = owner;
        self
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
    ///   same day may spend `held` after on-hand stock.
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
    /// Available stock is `quantity + held`. The `market` parameter is
    /// used solely to snapshot current AMV values for record-keeping.
    /// 
    /// ## Panic
    /// 
    /// Panics if good or process is not found in factuals.
    pub fn run_production(&mut self, factuals: &Factuals, market: &Market) -> Vec<ProcessEffect> {
        let mut effects = Vec::new();
        if factuals.config.firm.keep_alive {
            self.apply_keep_alive_float(factuals);
        }

        for i in 0..self.production_line.len() {
            if factuals.config.firm.keep_alive {
                self.apply_keep_alive_line(factuals, i);
            }
            let line = &mut self.production_line[i];
            // if process is not found, panic
            let Some(process) = factuals.processes.get(&line.process) else {
                panic!("Process not found!");
            };

            // Skip idle lines (target 0). `do_process` requires a positive target when Some.
            if matches!(line.target, Some(t) if t <= 0.0) {
                continue;
            }

            // Snapshot of spendable stock: on-hand plus today's holding slot.
            let available: HashMap<usize, f64> = self
                .property
                .iter()
                .map(|(&gid, row)| (gid, row.production_stock()))
                .collect();

            let result = process.do_process(&available, line.target, factuals);

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

    /// Emergency keep-alive wage float: floor hours at 1 and credit coin.
    /// Off unless `firm.keep_alive`.
    fn apply_keep_alive_float(&mut self, factuals: &Factuals) {
        let hours: f64 = self
            .workforce
            .iter()
            .filter(|w| w.id != 0)
            .map(|w| w.hours.max(0.0))
            .sum();
        if hours < 1.0 {
            for worker in &mut self.workforce {
                if worker.id != 0 && worker.hours < 1.0 {
                    worker.hours = 1.0;
                }
            }
        }
        if let Some(coin) = factuals
            .goods
            .values()
            .find(|good| good.name.eq_ignore_ascii_case("coin"))
            .map(|good| good.id)
        {
            let have = self
                .property
                .get(&coin)
                .map(|row| row.quantity)
                .unwrap_or(0.0);
            let want = hours.max(1.0) + 10.0;
            if have < want {
                self.grant_keep_alive_good(coin, want - have);
            }
        }
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
    /// Ignored when [`Self::remainder`] is set (owner-operator leftover).
    pub profit_share: f64,
    /// When true, this owner takes leftover till after wages, worker profit
    /// shares, stock fence, and growth. Owner-operator residual claim.
    /// On a loss (yesterday profit AMV <= 0) they also cover an AMV
    /// shortfall vs the firm's needs from their own stock.
    /// When false, [`Self::profit_share`] is a limited percent of yesterday's
    /// profit AMV (dividend / partial owner) and they do not cover losses.
    pub remainder: bool,
}

impl Owners {
    pub fn empty() -> Self {
        Owners {
            owner: Actor::Pop(0),
            priority_override: None,
            profit_share: 0.0,
            remainder: false,
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
        sell.min(history.max_salability() * made)
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

    /// Spends leftover till (above stock, growth, and posted sell) toward
    /// `want_amv`, high salability first.
    pub fn pay_profit_share_amv(
        &mut self,
        pop: &mut Pop,
        want_amv: f64,
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> (f64, HashMap<usize, f64>) {
        if want_amv <= 0.0 {
            return (0.0, HashMap::new());
        }
        let mut goods: Vec<usize> = self
            .property
            .keys()
            .copied()
            .filter(|&good| good != TIME)
            .collect();
        goods.sort_by(|a, b| {
            history
                .salability(*b)
                .partial_cmp(&history.salability(*a))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.cmp(b))
        });
        let mut remaining = want_amv;
        let mut paid_amv = 0.0;
        let mut paid: HashMap<usize, f64> = HashMap::new();
        for good in goods {
            if remaining <= 0.0 {
                break;
            }
            let price = history.price(good);
            if price <= 0.0 {
                continue;
            }
            let spendable = self.property.get(&good).map(|row| {
                row.profit_spendable_above_sell(self.posted_sell_qty(good, history, factuals))
            }).unwrap_or(0.0);
            let give = whole_units((remaining / price).min(spendable));
            if give <= 0.0 {
                continue;
            }
            self.debit_good(good, give);
            pop.credit_good(good, give, price);
            *paid.entry(good).or_insert(0.0) += give;
            let got = give * price;
            paid_amv += got;
            remaining -= got;
        }
        (paid_amv, paid)
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
    /// If selling, how many units they wish to sell each day. 
    pub sell_target: f64,
    /// How much we want to use in a given day, used/consumed/destroyed.
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
    /// How many were purchased today.
    pub bought: f64,
    /// The Total AMV cost for bought today. Unit cost = bought_amv / bought.
    pub bought_amv: f64,
    /// How many were sold today.
    pub sold: f64,
    /// The total AMV gained for sales today. Unit cost = sold_amv / sold.
    pub sold_amv: f64,
    /// The targeted unit AMV for Buying and/or Selling. If the row has both purchase 
    /// and sell targets, then this is a midpoint price, and the difference between
    /// buying and selling is defined by the Margin
    pub amv_target: f64,
    /// If being bought and sold, this modifies the buy and sell prices off of the 
    /// [`FirmPRow::amv_target`] appropriately. A simple multiplier to the AMV.
    /// Buy Price = amv_target * (1.0 - margin)
    /// Sell Price = amv_target * (1.0 + margin)
    /// 
    /// Should never be negative, but not enforced as that should be self-correcting.
    pub margin: f64,

    // Production data
    /// Amount of this good currently tied up as capital in active production runs.
    /// Removed from `quantity` (then `held`) during `run_production`; returned
    /// to `quantity` at decay, then that stock decays.
    pub used: f64,
    /// Today's process output waiting to join `quantity`. Not on the market.
    /// Later lines may spend it after on-hand `quantity`. Decay moves it into
    /// `quantity` last, after on-hand stock has already rotted.
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

    /// Units that can be offered for sale.
    /// `quantity - max(reserve, reserve_target)`, floored at 0.
    /// `reserve_target` is the stockpile guarantee; `reserve` is the live copy.
    pub fn sellable(&self) -> f64 {
        let floor = self.reserve.max(self.reserve_target).max(0.0);
        (self.quantity - floor).max(0.0)
    }

    /// On-hand units not fenced by reserve, reserve target, or (for production
    /// inputs) stock target / use target.
    pub fn free_for_market(&self) -> f64 {
        let mut floor = self.reserve.max(self.reserve_target);
        if self.use_target > 0.0 {
            floor = floor.max(self.stock_target).max(self.use_target);
        }
        (self.quantity - floor).max(0.0)
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
    /// Units that must stay for stock; never paid as wages or profit share.
    pub fn stock_fence(&self) -> f64 {
        self.stock_target.max(self.reserve_target).max(0.0)
    }

    /// On-hand units above the stock fence (growth may be raided for wages).
    pub fn wage_spendable(&self) -> f64 {
        (self.quantity - self.stock_fence()).max(0.0)
    }

    /// On-hand units above stock, growth, and `sell_target`.
    pub fn profit_spendable(&self) -> f64 {
        self.profit_spendable_above_sell(self.sell_target)
    }

    /// On-hand units above stock, growth, and `sell_fence`.
    /// Remainder leftover uses posted sell, not unconstrained `sell_target`.
    pub fn profit_spendable_above_sell(&self, sell_fence: f64) -> f64 {
        (self.quantity
            - self.stock_fence()
            - self.growth_target.max(0.0)
            - sell_fence.max(0.0))
        .max(0.0)
    }
}


impl DealMaker for Firm {
    /// # Buy
    ///
    /// Returns a proposed basket as buyer, or `None` if no tender can be named.
    /// Tenders [`FirmPRow::free_for_market`]. Seller's named counter first
    /// (any salability), then other free stock by salability. Highly
    /// salable goods (and that counter) cover the fill first; lower
    /// salability only if those cannot. Shrinks the fill if still short.
    /// Does not tender units `create_orders` would put on sell or offer
    /// (current classify: sell and liquidate slices). Exchange leftover on
    /// a sell-plan good is still tenderable. AMV bounds do not void the
    /// basket. Does not move stock.
    fn buy(
        &self,
        own_order: &MarketOrder,
        other_order: &MarketOrder,
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> Option<ProposedDeal> {
        debug_assert_eq!(own_order.origin, Actor::Firm(self.id));
        let targeted_good = own_order.target;
        let live = firm_live_tenders(self, targeted_good, history, factuals);
        let deal = form_buy_proposal(
            Actor::Firm(self.id),
            own_order,
            other_order,
            history,
            factuals.config.deal.high_salability,
            |good| firm_tenderable(self, good, targeted_good, factuals, history),
            &live,
        )?;
        let deal = with_transport_budget(
            deal,
            Actor::Firm(self.id),
            own_order,
            other_order,
            history,
            factuals,
            |good| firm_tenderable(self, good, targeted_good, factuals, history),
            &live,
            transport_cover_on_hand(
                self.property.iter().map(|(id, row)| (*id, row.quantity)),
                factuals,
            ),
        )?;
        Some(deal)
    }

    /// # Evaluate
    ///
    /// Returns Accept or Reject for this deal as this firm.
    /// Keep must meet the firm AMV floor, with the need catch (purchase or
    /// use target on a received good) down to the need keep. Process inputs
    /// skip salability; other received goods are haircut. Buyers accept
    /// windfalls. Does not move stock.
    fn evaluate(
        &self,
        deal: &ProposedDeal,
        own_order: &MarketOrder,
        other_order: &MarketOrder,
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> DealResponse {
        let _ = other_order;
        debug_assert_eq!(own_order.origin, Actor::Firm(self.id));
        let Some(role) = deal.role_of(Actor::Firm(self.id)) else {
            debug_assert!(false, "firm must be a party to the deal");
            return DealResponse::Reject;
        };
        if !deal_goods_tradeable(deal, factuals) {
            return DealResponse::Reject;
        }
        let needs_received = deal.goods_received(role).any(|(good, _)| {
            self.property
                .get(&good)
                .is_some_and(|row| row.purchase_target > 0.0 || row.use_target > 0.0)
        });
        evaluate_amv_floor(
            deal,
            role,
            history,
            factuals.config.deal.firm_amv_min_keep,
            factuals.config.deal.firm_amv_need_keep,
            needs_received,
            |good| firm_uses_good(self, good),
        )
    }

    /// # Finalize
    ///
    /// Applies `deal` to this firm's on-hand `quantity`. Seller adds the map,
    /// buyer subtracts it. Incoming units blend into `average_cost` at market
    /// AMV. Buyer receipts also add `bought` / `bought_amv`; seller outflows
    /// add `sold` / `sold_amv`. Syncs reserve after each good. Does not edit
    /// orders or raise reserve toward stock target.
    fn finalize(&mut self, deal: &ProposedDeal, history: &MarketHistory) {
        let Some(role) = deal.role_of(Actor::Firm(self.id)) else {
            debug_assert!(false, "firm must be a party to the deal");
            return;
        };
        for (&good, _) in &deal.goods {
            let delta = deal.signed_qty(role, good);
            if delta == 0.0 {
                continue;
            }
            let price = history.price(good);
            let row = self.property.entry(good).or_insert_with(FirmPRow::new);
            row.quantity += delta;
            debug_assert!(
                row.quantity >= 0.0,
                "quantity must be >= 0.0 (firm {} good {} qty {} delta {})",
                self.id,
                good,
                row.quantity - delta,
                delta
            );
            if delta > 0.0 {
                row.blend_average_cost(delta, price);
                if role == DealRole::Buyer {
                    row.bought += delta;
                    row.bought_amv += delta * price;
                }
            } else if role == DealRole::Seller {
                let sold = -delta;
                row.sold += sold;
                row.sold_amv += sold * price;
            }
            row.sync_reserve();
        }
    }

    fn pay_transport(&mut self, amount: f64, factuals: &Factuals) {
        let on_hand: Vec<(usize, f64)> = self
            .property
            .iter()
            .map(|(id, row)| (*id, row.quantity))
            .collect();
        for (id, sub) in transport_spend_plan(amount, factuals, on_hand) {
            debug_assert!(sub >= 0.0 && sub.is_finite(), "transport spend must be >= 0.0");
            if let Some(row) = self.property.get_mut(&id) {
                row.quantity = (row.quantity - sub).max(0.0);
                row.consumed += sub;
                row.sync_reserve();
            }
            self.transport_spent += sub;
        }
    }
}

/// Returns true if this firm has `use_target` on `good`.
/// Those goods skip the salability haircut when received.
fn firm_uses_good(firm: &Firm, good: usize) -> bool {
    firm.property
        .get(&good)
        .is_some_and(|row| row.use_target > 0.0)
}

/// Returns how many units of `good` this firm can tender (0 if it is `targeted_good`).
/// Excludes units `create_orders` would put on sell or offer (still in `free_for_market`).
fn firm_tenderable(
    firm: &Firm,
    good: usize,
    targeted_good: usize,
    factuals: &Factuals,
    history: &MarketHistory,
) -> f64 {
    if good == targeted_good {
        return 0.0;
    }
    if !factuals.find_good(good).is_buyable() {
        return 0.0;
    }
    let Some(row) = firm.property.get(&good) else {
        return 0.0;
    };
    // Tender the exchange slice only. Sell/liquidate stay for those orders.
    // A sell-plan good with no proven money still needs that leftover as
    // payment. Morning sell qty plus a later reclassify can overdraw if
    // salability rose; freeze the morning split later if that bites.
    let sell_plan = firm.posted_sell_qty(good, history, factuals);
    let split = orders::classify_on_hand(
        row,
        history.salability(good),
        &factuals.config.market,
        sell_plan,
    );
    (row.free_for_market() - split.sell - split.liquidate).max(0.0)
}

/// Returns this firm's tenderable goods as `(id, salability, qty)`, highest salability 
/// first.
fn firm_live_tenders(
    firm: &Firm,
    targeted_good: usize,
    history: &MarketHistory,
    factuals: &Factuals,
) -> Vec<(usize, f64)> {
    collect_tenders(firm.property.keys().copied(), history, |good| {
        firm_tenderable(firm, good, targeted_good, factuals, history)
    })
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
            last_success_rate: 0.0,
            last_iterations: 0.0,
            last_effects: vec![],
            last_missing_goods: vec![],
            last_amv_consumed: 0.0,
            last_amv_produced: 0.0,
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
                last_success_rate: 0.0,
                last_iterations: 0.0,
                last_effects: vec![],
                last_missing_goods: vec![],
                last_amv_consumed: 0.0,
                last_amv_produced: 0.0,
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
        fn keep_alive_feeds_inputs_and_runs_one_iteration() {
            let process = Process::new(2, "limited_craft", 0)
                .with_input(ProcessInput::new(30, 3.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(40, 1.0, true));
            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(30, make_good(30, "wood", HashMap::new()));
            factuals.goods.insert(40, make_good(40, "plank", HashMap::new()));
            factuals.goods.insert(5, make_good(5, "coin", HashMap::new()));
            factuals.config.firm.keep_alive = true;

            let mut firm = Firm::new(2, "Starved Shop".into(), 42, hexx::Hex::new(0, 0));
            firm.production_line.push(empty_production_line(2));
            firm.production_line[0].target = Some(4.0);
            let market = make_market_with_amvs(&[(30, 1.0), (40, 1.0), (5, 0.21)]);
            firm.run_production(&factuals, &market);

            assert!(
                firm.production_line[0].last_iterations >= 1.0,
                "iters {}",
                firm.production_line[0].last_iterations
            );
            assert!(firm.property.get(&40).map(|r| r.held).unwrap_or(0.0) >= 1.0);
            assert!(firm.property.get(&5).map(|r| r.quantity).unwrap_or(0.0) >= 10.0);
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
        fn keep_alive_runs_each_collapsed_line_when_they_share_an_input() {
            let cut = Process::new(5, "cut jewelry", 0)
                .with_input(ProcessInput::new(4, 3.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(6, 5.0, true));
            let mint = Process::new(4, "mint coin", 0)
                .with_input(ProcessInput::new(4, 1.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(5, 40.0, true));
            let mut factuals = make_factuals_with_process(cut);
            factuals.processes.insert(4, mint);
            factuals.goods.insert(4, make_good(4, "gold", HashMap::new()));
            factuals.goods.insert(5, make_good(5, "coin", HashMap::new()));
            factuals.goods.insert(6, make_good(6, "jewelry", HashMap::new()));
            factuals.config.firm.keep_alive = true;

            let mut firm = Firm::new(5, "Starved Jeweler".into(), 42, hexx::Hex::new(0, 0));
            firm.production_line.push(empty_production_line(5));
            firm.production_line[0].target = Some(1.0);
            firm.production_line.push(empty_production_line(4));
            firm.production_line[1].target = Some(1.0);
            let market = make_market_with_amvs(&[(4, 4.0), (5, 0.21), (6, 60.0)]);
            firm.run_production(&factuals, &market);

            assert!(
                firm.production_line[0].last_iterations >= 1.0,
                "cut iters {}",
                firm.production_line[0].last_iterations
            );
            assert!(
                firm.production_line[1].last_iterations >= 1.0,
                "mint iters {}",
                firm.production_line[1].last_iterations
            );
            assert!(firm.property.get(&6).map(|r| r.held).unwrap_or(0.0) >= 5.0);
            assert!(firm.property.get(&5).map(|r| r.held).unwrap_or(0.0) >= 40.0);
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
                last_success_rate: 0.0,
                last_iterations: 0.0,
                last_effects: vec![],
                last_missing_goods: vec![],
                last_amv_consumed: 0.0,
                last_amv_produced: 0.0,
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
                last_success_rate: 0.0,
                last_iterations: 0.0,
                last_effects: vec![],
                last_missing_goods: vec![],
                last_amv_consumed: 0.0,
                last_amv_produced: 0.0,
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
                last_success_rate: 0.0,
                last_iterations: 0.0,
                last_effects: vec![],
                last_missing_goods: vec![],
                last_amv_consumed: 0.0,
                last_amv_produced: 0.0,
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
                last_success_rate: 0.42,
                last_iterations: 3.0,
                last_effects: vec![ProcessEffect::Culture(1.0)],
                last_missing_goods: vec![1],
                last_amv_consumed: 10.0,
                last_amv_produced: 0.0,
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
                    .with_average_cost(3.0)
                    .with_reserve_target(1.0)
                    .with_reserve(1.0),
            );

            firm.clear_day_flows();

            let row = &firm.property[&10];
            assert_eq!(row.produced, 0.0);
            assert_eq!(row.consumed, 0.0);
            assert_eq!(row.bought, 0.0);
            assert_eq!(row.bought_amv, 0.0);
            assert_eq!(row.sold, 0.0);
            assert_eq!(row.sold_amv, 0.0);
            assert_eq!(row.quantity, 7.0);
            assert_eq!(row.used, 2.0);
            assert_eq!(row.average_cost, 3.0);
            assert_eq!(row.reserve, 1.0);
        }
    }

    mod scale_amv_unit_should {
        use super::*;

        #[test]
        fn multiplies_quotes_and_cost_basis() {
            let mut firm = Firm::new(1, "Shop".into(), 42, hexx::Hex::new(0, 0));
            let mut row = FirmPRow::new()
                .with_amv_target(9.0)
                .with_average_cost(8.0)
                .with_average_price(10.0)
                .with_bought_amv(4.0)
                .with_sold_amv(6.0);
            row.amv_bound = FirmAmvBound::Minimum(3.0);
            firm.property.insert(10, row);

            firm.scale_amv_unit(0.5);

            let row = &firm.property[&10];
            assert!((row.amv_target - 4.5).abs() < 1e-12);
            assert!((row.average_cost - 4.0).abs() < 1e-12);
            assert!((row.average_price - 5.0).abs() < 1e-12);
            assert!((row.bought_amv - 2.0).abs() < 1e-12);
            assert!((row.sold_amv - 3.0).abs() < 1e-12);
            assert_eq!(row.amv_bound, FirmAmvBound::Minimum(1.5));
        }

        #[test]
        fn leaves_unset_amv_target_at_zero() {
            let mut firm = Firm::new(1, "Shop".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(10, FirmPRow::new().with_average_cost(8.0));
            firm.scale_amv_unit(0.5);
            assert_eq!(firm.property[&10].amv_target, 0.0);
            assert!((firm.property[&10].average_cost - 4.0).abs() < 1e-12);
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
        fn leaves_sell_target_when_the_firm_does_not_make_the_good() {
            let factuals = Factuals::new();
            let mut firm = Firm::new(1, "shop".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(5, FirmPRow::new().with_sell_target(20.0));
            let history = history_sal(&[(5, 0.3)]);
            assert!((firm.posted_sell_qty(5, &history, &factuals) - 20.0).abs() < 1e-12);
        }
    }


    mod plan_should {
        use super::*;
        use crate::game::good::GoodTag;
        use crate::game::market::MarketHistory;

        fn make_history(entries: &[(usize, f64)]) -> MarketHistory {
            let mut history = MarketHistory::new();
            for &(id, price) in entries {
                history.prices.insert(id, price);
            }
            history
        }

        fn miller_world() -> (Factuals, MarketHistory) {
            let process = Process::new(1, "mill", 0)
                .with_input(ProcessInput::new(10, 2.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(20, 1.0, true));
            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(10, make_good(10, "wood", HashMap::new()));
            factuals.goods.insert(20, make_good(20, "plank", HashMap::new()));
            factuals.config.firm.planning_lerp_rate = 1.0;
            (factuals, make_history(&[(10, 1.0), (20, 5.0)]))
        }

        fn miller_firm(target: f64) -> Firm {
            let mut firm = Firm::new(1, "mill".into(), 42, hexx::Hex::new(0, 0));
            let mut line = empty_production_line(1);
            line.target = Some(target);
            firm.production_line.push(line);
            firm.property.insert(10, FirmPRow::new().with_quantity(10.0));
            firm.property.insert(20, FirmPRow::new().with_quantity(10.0));
            firm.property.insert(5, FirmPRow::new().with_quantity(50.0));
            firm
        }

        fn mark_hit(firm: &mut Firm, sold: f64, produced: f64, sell_target: f64) {
            let line = &mut firm.production_line[0];
            line.last_success_rate = 1.0;
            line.last_iterations = line.target.unwrap_or(0.0);
            line.last_amv_consumed = 8.0;
            line.last_amv_produced = 20.0;
            if let Some(row) = firm.property.get_mut(&20) {
                row.sold = sold;
                row.produced = produced;
                row.sell_target = sell_target;
            }
        }

        #[test]
        fn cold_start_keeps_target_and_rolls_up_recipe() {
            let (factuals, history) = miller_world();
            let mut firm = miller_firm(4.0);
            firm.plan(&factuals, &history);

            assert_eq!(firm.production_line[0].target, Some(4.0));
            let wood = &firm.property[&10];
            assert_eq!(wood.use_target, 8.0);
            assert_eq!(wood.stock_target, 16.0);
            assert_eq!(wood.purchase_target, 6.0);
            assert_eq!(wood.amv_bound, FirmAmvBound::Maximum(2.5));
            let plank = &firm.property[&20];
            assert_eq!(plank.use_target, 0.0);
            assert_eq!(plank.sell_target, 4.0);
            assert_eq!(plank.amv_bound, FirmAmvBound::Minimum(2.0));
            let coin = &firm.property[&5];
            assert_eq!(coin.purchase_target, 0.0);
            assert_eq!(coin.sell_target, 0.0);
            assert_eq!(coin.amv_bound, FirmAmvBound::None);
        }

        #[test]
        fn starting_a_line_from_zero_snaps_to_one_iteration() {
            let (factuals, history) = miller_world();
            let mut firm = miller_firm(0.0);
            mark_hit(&mut firm, 4.0, 0.0, 4.0);
            firm.production_line[0].last_iterations = 4.0;
            firm.plan(&factuals, &history);
            let got = firm.production_line[0].target.unwrap();
            assert!(got >= 1.0, "got {got}");
        }

        #[test]
        fn idle_line_stays_at_zero() {
            let (factuals, history) = miller_world();
            let mut firm = miller_firm(0.0);
            firm.plan(&factuals, &history);
            assert_eq!(firm.production_line[0].target, Some(0.0));
        }

        #[test]
        fn quiet_baseline_keeps_the_line_target() {
            let (factuals, history) = miller_world();
            let mut firm = miller_firm(4.0);
            let line = &mut firm.production_line[0];
            line.last_success_rate = 1.0;
            line.last_iterations = 4.0;
            line.last_amv_consumed = 10.0;
            line.last_amv_produced = 10.0;
            let plank = firm.property.get_mut(&20).unwrap();
            // sell success 0.6 sits in the shrink..grow band; leftover matches output_cover.
            plank.sold = 2.4;
            plank.produced = 4.0;
            plank.sell_target = 4.0;
            plank.quantity = 6.0;
            plank.amv_target = 5.0;
            firm.plan(&factuals, &history);
            assert_eq!(firm.production_line[0].target, Some(4.0));
            assert_eq!(firm.property[&20].sell_target, 4.0);
        }

        #[test]
        fn missing_inputs_do_not_shrink_the_line() {
            let (factuals, history) = miller_world();
            let mut firm = miller_firm(4.0);
            firm.production_line[0].last_success_rate = 0.0;
            firm.production_line[0].last_iterations = 0.0;
            firm.production_line[0].last_missing_goods = vec![10];
            firm.property.get_mut(&10).unwrap().quantity = 0.0;
            firm.plan(&factuals, &history);
            assert_eq!(firm.production_line[0].target, Some(4.0));
            assert_eq!(firm.property[&10].purchase_target, 16.0);
        }

        #[test]
        fn sell_above_output_raises_both_lines() {
            let (factuals, history) = miller_world();
            let mut firm = miller_firm(10.0);
            let mut second = empty_production_line(1);
            second.target = Some(10.0);
            second.last_success_rate = 1.0;
            second.last_iterations = 10.0;
            second.last_amv_consumed = 10.0;
            second.last_amv_produced = 11.0;
            firm.production_line.push(second);
            firm.production_line[0].last_success_rate = 1.0;
            firm.production_line[0].last_iterations = 10.0;
            firm.production_line[0].last_amv_consumed = 10.0;
            firm.production_line[0].last_amv_produced = 11.0;
            let plank = firm.property.get_mut(&20).unwrap();
            plank.sell_target = 40.0;
            plank.sold = 40.0;
            plank.produced = 20.0;
            plank.quantity = 40.0;
            plank.amv_target = 5.0;
            firm.plan(&factuals, &history);
            let a = firm.production_line[0].target.unwrap();
            let b = firm.production_line[1].target.unwrap();
            assert!(a > 10.0 && b > 10.0, "got {a} {b}");
        }

        #[test]
        fn unequal_profit_shifts_output_to_the_better_line() {
            let (factuals, history) = miller_world();
            let mut firm = miller_firm(10.0);
            let mut second = empty_production_line(1);
            second.target = Some(10.0);
            second.last_success_rate = 1.0;
            second.last_iterations = 10.0;
            second.last_amv_consumed = 10.0;
            second.last_amv_produced = 10.0;
            firm.production_line.push(second);
            firm.production_line[0].last_success_rate = 1.0;
            firm.production_line[0].last_iterations = 10.0;
            firm.production_line[0].last_amv_consumed = 10.0;
            firm.production_line[0].last_amv_produced = 11.9;
            let plank = firm.property.get_mut(&20).unwrap();
            plank.sell_target = 20.0;
            plank.sold = 12.0;
            plank.produced = 20.0;
            plank.quantity = 30.0;
            plank.amv_target = 5.0;
            firm.plan(&factuals, &history);
            let better = firm.production_line[0].target.unwrap();
            let worse = firm.production_line[1].target.unwrap();
            assert!(better > worse, "got {better} {worse}");
            assert!((better + worse - 20.0).abs() < 1e-6, "sum {}", better + worse);
        }

        #[test]
        fn undersell_with_a_high_quote_cuts_own_amv_not_to_market() {
            let (factuals, history) = miller_world();
            let mut firm = miller_firm(4.0);
            mark_hit(&mut firm, 0.0, 4.0, 8.0);
            firm.property.get_mut(&20).unwrap().quantity = 8.0;
            firm.property.get_mut(&20).unwrap().amv_target = 6.0;
            firm.plan(&factuals, &history);
            let amv = firm.property[&20].amv_target;
            assert!(amv < 6.0, "got {amv}");
            assert_ne!(amv, history.price(20));
            assert_eq!(firm.production_line[0].target, Some(4.0));
        }

        #[test]
        fn unlimited_target_stays_none_and_uses_last_iterations() {
            let (factuals, history) = miller_world();
            let mut firm = miller_firm(4.0);
            firm.production_line[0].target = None;
            firm.production_line[0].last_iterations = 3.0;
            firm.plan(&factuals, &history);
            assert_eq!(firm.production_line[0].target, None);
            assert_eq!(firm.property[&10].use_target, 6.0);
        }

        #[test]
        fn merchant_row_restocks_what_sold() {
            let (factuals, history) = miller_world();
            let mut firm = Firm::new(2, "trader".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(
                20,
                FirmPRow::new()
                    .with_quantity(4.0)
                    .with_purchase_target(10.0)
                    .with_sell_target(10.0)
                    .with_sold(6.0)
                    .with_amv_target(5.0),
            );
            firm.plan(&factuals, &history);
            let row = &firm.property[&20];
            assert_eq!(row.purchase_target, 6.0);
            assert_eq!(row.sell_target, 4.0);
            assert_eq!(row.use_target, 0.0);
            assert_eq!(row.amv_bound, FirmAmvBound::None);
            assert_eq!(row.margin, factuals.config.firm.default_margin);
        }

        #[test]
        fn untradeable_input_gets_use_but_no_purchase() {
            let (mut factuals, history) = miller_world();
            factuals.goods.get_mut(&10).unwrap().tags.insert(GoodTag::Untradeable);
            let mut firm = miller_firm(4.0);
            firm.plan(&factuals, &history);
            let wood = &firm.property[&10];
            assert_eq!(wood.use_target, 8.0);
            assert_eq!(wood.purchase_target, 0.0);
        }

        #[test]
        fn record_keeping_updates_rolling_average_then_plans() {
            let (factuals, history) = miller_world();
            let mut firm = miller_firm(4.0);
            firm.property.get_mut(&10).unwrap().rolling_average = 0.0;
            firm.record_keeping(&factuals, &history);
            assert_eq!(firm.property[&10].rolling_average, 2.5);
            assert_eq!(firm.property[&10].use_target, 8.0);
            assert_eq!(firm.records.profit_ratio, 1.0);
            assert_eq!(firm.records.sell_success, 1.0);
        }

        #[test]
        fn missed_purchase_raises_reserve_target() {
            let (factuals, history) = miller_world();
            let mut firm = miller_firm(4.0);
            firm.property.get_mut(&10).unwrap().purchase_target = 8.0;
            firm.property.get_mut(&10).unwrap().bought = 0.0;
            firm.property.get_mut(&10).unwrap().quantity = 0.0;
            firm.plan(&factuals, &history);
            // use 8 * reserve_cover 0.5 * (1 + 0.5 miss) = 6.0, lerp 1.0
            assert_eq!(firm.property[&10].reserve_target, 6.0);
        }

        #[test]
        fn unsold_output_does_not_raise_the_line() {
            let (factuals, history) = miller_world();
            let mut firm = miller_firm(4.0);
            mark_hit(&mut firm, 0.0, 4.0, 8.0);
            firm.property.get_mut(&20).unwrap().quantity = 8.0;
            firm.property.get_mut(&20).unwrap().amv_target = 5.0;
            firm.plan(&factuals, &history);
            assert_eq!(firm.production_line[0].target, Some(4.0));
            assert!(
                firm.property[&20].sell_target < 8.0,
                "got {}",
                firm.property[&20].sell_target
            );
        }

        #[test]
        fn strong_sales_above_cost_raise_sell_target() {
            let (factuals, history) = miller_world();
            let mut firm = miller_firm(4.0);
            mark_hit(&mut firm, 4.0, 4.0, 4.0);
            let plank = firm.property.get_mut(&20).unwrap();
            plank.quantity = 6.0;
            plank.amv_target = 5.0;
            plank.sold_amv = 40.0;
            plank.average_cost = 5.0;
            firm.plan(&factuals, &history);
            assert!(
                firm.property[&20].sell_target > 4.0,
                "got {}",
                firm.property[&20].sell_target
            );
        }

        #[test]
        fn record_keeping_snapshots_profit_and_sell_success() {
            let (factuals, history) = miller_world();
            let mut firm = miller_firm(4.0);
            mark_hit(&mut firm, 4.0, 4.0, 4.0);
            let plank = firm.property.get_mut(&20).unwrap();
            plank.sold_amv = 40.0;
            plank.average_cost = 5.0;
            firm.record_keeping(&factuals, &history);
            assert_eq!(firm.records.profit_ratio, 2.0);
            assert_eq!(firm.records.sell_success, 1.0);
        }
    }

    mod create_orders_should {
        use super::*;
        use crate::game::actor::Actor;
        use crate::game::config::market_priority;
        use crate::game::good::GoodTag;
        use crate::game::market::MarketHistory;
        use crate::game::marketorder::compose_sell_priority;

        fn make_history(entries: &[(usize, f64, f64)]) -> MarketHistory {
            let mut history = MarketHistory::new();
            for &(id, price, salability) in entries {
                history.prices.insert(id, price);
                history.salability.insert(id, salability);
            }
            history
        }

        fn make_factuals_goods(ids: &[usize]) -> Factuals {
            let mut factuals = Factuals::new();
            for &id in ids {
                factuals.goods.insert(id, make_good(id, "good", HashMap::new()));
            }
            factuals
        }

        fn empty_firm() -> Firm {
            Firm::new(7, "Shop".into(), 42, hexx::Hex::new(0, 0))
        }

        fn orders_for(orders: &[crate::game::marketorder::MarketOrder], good: usize) -> Vec<&crate::game::marketorder::MarketOrder> {
            orders.iter().filter(|order| order.target == good).collect()
        }

        #[test]
        fn miller_sells_output_and_buys_input_shortfall() {
            let mut firm = empty_firm();
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_quantity(4.0)
                    .with_purchase_target(8.0)
                    .with_use_target(6.0)
                    .with_stock_target(10.0),
            );
            firm.property.insert(
                20,
                FirmPRow::new()
                    .with_quantity(12.0)
                    .with_sell_target(12.0),
            );
            firm.production_line.push(empty_production_line(1));
            firm.production_line[0].inputs = vec![10];

            let factuals = make_factuals_goods(&[10, 20]);
            let history = make_history(&[(10, 1.0, 0.4), (20, 2.0, 0.4)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());

            assert_eq!(orders.len(), 2);
            assert!(orders[0].is_sell_order());
            assert_eq!(orders[0].target, 20);
            assert_eq!(orders[0].target_amount, -12.0);
            assert_eq!(orders[0].counter_offer, Some(10));
            assert_eq!(
                orders[0].priority,
                compose_sell_priority(market_priority::FIRM_PRODUCER, 12.0, 0.0)
            );
            assert!(orders[1].is_request_order());
            assert_eq!(orders[1].target, 10);
            assert_eq!(orders[1].target_amount, 6.0);
            assert_eq!(orders[1].priority, market_priority::FIRM_PRODUCER);
            assert_eq!(firm.property[&10].quantity, 4.0);
            assert_eq!(firm.property[&20].quantity, 12.0);
        }

        #[test]
        fn merchant_buys_beyond_stock_and_sells_current_sellable() {
            let mut firm = empty_firm();
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_quantity(20.0)
                    .with_reserve_target(5.0)
                    .with_purchase_target(8.0)
                    .with_sell_target(10.0)
                    .with_stock_target(20.0)
                    .with_amv_target(2.0)
                    .with_margin(0.1),
            );

            let factuals = make_factuals_goods(&[10]);
            let history = make_history(&[(10, 2.0, 0.5)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());

            assert_eq!(orders.len(), 2);
            assert!(orders[0].is_offer_order());
            assert_eq!(orders[0].target, 10);
            assert_eq!(orders[0].target_amount, -10.0);
            assert_eq!(
                orders[0].priority,
                compose_sell_priority(market_priority::FIRM_MERCHANT, 10.0, 0.0)
            );
            assert!(orders[1].is_request_order());
            assert_eq!(orders[1].target, 10);
            assert_eq!(orders[1].target_amount, 8.0);
            assert_eq!(orders[1].priority, market_priority::FIRM_MERCHANT);
        }

        #[test]
        fn producer_dumps_only_excess_above_stock() {
            let mut firm = empty_firm();
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_quantity(15.0)
                    .with_purchase_target(8.0)
                    .with_sell_target(20.0)
                    .with_use_target(5.0)
                    .with_stock_target(10.0),
            );

            let factuals = make_factuals_goods(&[10]);
            let history = make_history(&[(10, 1.0, 0.4)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());

            assert_eq!(orders.len(), 1);
            assert!(orders[0].is_offer_order());
            assert_eq!(orders[0].target_amount, -5.0);
            assert_eq!(orders[0].priority, compose_sell_priority(
                market_priority::FIRM_PRODUCER, 5.0, 0.0
            ));
        }

        #[test]
        fn mid_salability_split_is_half_sell_half_exchange() {
            let mut firm = empty_firm();
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_quantity(10.0)
                    .with_sell_target(10.0),
            );

            let factuals = make_factuals_goods(&[10]);
            let history = make_history(&[(10, 1.0, 0.8)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());

            assert_eq!(orders.len(), 1);
            assert!(orders[0].is_offer_order());
            assert_eq!(orders[0].target, 10);
            assert_eq!(orders[0].target_amount, -5.0);
        }

        #[test]
        fn exchange_floor_keeps_ten_percent_as_tender() {
            let mut firm = empty_firm();
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_quantity(10.0)
                    .with_sell_target(10.0),
            );

            let factuals = make_factuals_goods(&[10]);
            let history = make_history(&[(10, 1.0, 0.6)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());

            assert_eq!(orders.len(), 1);
            assert_eq!(orders[0].target_amount, -9.0);
        }

        #[test]
        fn full_salability_keeps_ten_percent_for_sale() {
            let mut firm = empty_firm();
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_quantity(10.0)
                    .with_sell_target(10.0),
            );
            firm.property.insert(
                20,
                FirmPRow::new().with_purchase_target(1.0),
            );

            let factuals = make_factuals_goods(&[10, 20]);
            let history = make_history(&[(10, 1.0, 1.0), (20, 1.0, 0.4)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());

            let outgoing = orders_for(&orders, 10);
            assert_eq!(outgoing.len(), 1);
            assert_eq!(outgoing[0].target_amount, -1.0);
            assert_eq!(orders.iter().find(|o| o.target == 20).unwrap().counter_offer, Some(10));
        }

        #[test]
        fn sell_target_caps_then_remainder_stays_exchange() {
            let mut firm = empty_firm();
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_quantity(10.0)
                    .with_sell_target(2.0),
            );
            firm.property.insert(
                20,
                FirmPRow::new().with_purchase_target(4.0),
            );

            let factuals = make_factuals_goods(&[10, 20]);
            let history = make_history(&[(10, 1.0, 0.75), (20, 1.0, 0.4)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());

            let outgoing = orders_for(&orders, 10);
            assert_eq!(outgoing.len(), 1);
            assert_eq!(outgoing[0].target_amount, -2.0);
            assert!(orders.iter().any(|o| o.target == 20 && o.counter_offer == Some(10)));
        }

        #[test]
        fn posts_sell_capped_by_max_sal_times_daily_output() {
            let mut firm = empty_firm();
            firm.property.insert(
                20,
                FirmPRow::new()
                    .with_quantity(150.0)
                    .with_sell_target(150.0),
            );
            let mut line = empty_production_line(1);
            line.target = Some(10.0);
            firm.production_line.push(line);
            let process = Process::new(1, "mill", 0)
                .with_output(ProcessOutput::new(20, 15.0, true));
            let mut factuals = make_factuals_goods(&[20]);
            factuals.processes.insert(1, process);
            let history = make_history(&[(20, 1.0, 0.3)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());
            let outgoing = orders_for(&orders, 20);
            assert_eq!(outgoing.len(), 1);
            assert_eq!(outgoing[0].target_amount, -45.0);
        }

        #[test]
        fn liquidates_unwanted_low_salability_as_offer() {
            let mut firm = empty_firm();
            firm.property.insert(
                10,
                FirmPRow::new().with_quantity(5.0),
            );

            let factuals = make_factuals_goods(&[10]);
            let history = make_history(&[(10, 1.0, 0.3)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());

            assert_eq!(orders.len(), 1);
            assert!(orders[0].is_offer_order());
            assert!(!orders[0].is_sell_order());
            assert_eq!(orders[0].target, 10);
            assert_eq!(orders[0].target_amount, -5.0);
        }

        #[test]
        fn liquidate_stays_an_offer_even_when_money_exists() {
            let mut firm = empty_firm();
            firm.property.insert(
                1,
                FirmPRow::new().with_quantity(10.0),
            );
            firm.property.insert(
                10,
                FirmPRow::new().with_quantity(5.0),
            );

            let factuals = make_factuals_goods(&[1, 10]);
            let history = make_history(&[(1, 1.0, 0.9), (10, 1.0, 0.3)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());

            let pottery = orders_for(&orders, 10);
            assert_eq!(pottery.len(), 1);
            assert!(pottery[0].is_offer_order());
            assert!(!pottery[0].is_sell_order());
        }

        #[test]
        fn unwanted_high_salability_is_exchange_not_liquidate() {
            let mut firm = empty_firm();
            firm.property.insert(
                1,
                FirmPRow::new().with_quantity(10.0),
            );
            firm.property.insert(
                20,
                FirmPRow::new().with_purchase_target(2.0),
            );

            let factuals = make_factuals_goods(&[1, 20]);
            let history = make_history(&[(1, 1.0, 0.9), (20, 1.0, 0.4)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());

            assert_eq!(orders_for(&orders, 1).len(), 0);
            assert_eq!(orders.len(), 1);
            assert!(orders[0].is_buy_order());
            assert_eq!(orders[0].target, 20);
            assert_eq!(orders[0].counter_offer, Some(1));
        }

        #[test]
        fn buys_production_inputs_before_merchant_restock() {
            let mut firm = empty_firm();
            // Coin-like tender funds both buys.
            firm.property.insert(
                5,
                FirmPRow::new().with_quantity(100.0),
            );
            firm.property.insert(
                20,
                FirmPRow::new()
                    .with_purchase_target(3.0),
            );
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_purchase_target(3.0)
                    .with_use_target(3.0)
                    .with_stock_target(3.0),
            );
            firm.production_line.push(empty_production_line(1));
            firm.production_line[0].inputs = vec![10];

            let factuals = make_factuals_goods(&[5, 10, 20]);
            let history = make_history(&[
                (5, 1.0, 0.9),
                (10, 1.0, 0.4),
                (20, 1.0, 0.4),
            ]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());

            let buys: Vec<_> = orders.iter().filter(|o| o.target_amount > 0.0).collect();
            assert_eq!(buys.len(), 2);
            assert_eq!(buys[0].target, 10);
            assert_eq!(buys[1].target, 20);
            assert_eq!(buys[0].priority, market_priority::FIRM_PRODUCER);
        }

        #[test]
        fn skips_untradeable_goods() {
            let mut firm = empty_firm();
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_quantity(5.0)
                    .with_purchase_target(2.0)
                    .with_sell_target(2.0),
            );
            let mut factuals = make_factuals_goods(&[10]);
            factuals.goods.get_mut(&10).unwrap().tags.insert(GoodTag::Untradeable);
            let history = make_history(&[(10, 1.0, 0.5)]);

            let orders = firm.create_orders(&history, &factuals, &HashSet::new());
            assert!(orders.is_empty());
        }

        #[test]
        fn emits_nothing_without_spendable_amv() {
            let mut firm = empty_firm();
            firm.property.insert(
                10,
                FirmPRow::new().with_purchase_target(4.0),
            );
            let factuals = make_factuals_goods(&[10]);
            let history = make_history(&[(10, 1.0, 0.4)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());
            assert!(orders.is_empty());
        }

        #[test]
        fn does_not_mutate_the_firm() {
            let mut firm = empty_firm();
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_quantity(8.0)
                    .with_sell_target(3.0)
                    .with_purchase_target(1.0),
            );
            let before = firm.clone();
            let factuals = make_factuals_goods(&[10]);
            let history = make_history(&[(10, 1.0, 0.4)]);
            let _ = firm.create_orders(&history, &factuals, &HashSet::new());
            assert_eq!(firm.property[&10].quantity, before.property[&10].quantity);
            assert_eq!(
                firm.property[&10].purchase_target,
                before.property[&10].purchase_target
            );
            assert_eq!(firm.property[&10].sell_target, before.property[&10].sell_target);
        }

        #[test]
        fn merchant_with_tender_sets_bid_ask_spread() {
            let mut firm = empty_firm();
            firm.property.insert(
                1,
                FirmPRow::new().with_quantity(10.0),
            );
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_quantity(20.0)
                    .with_purchase_target(5.0)
                    .with_sell_target(5.0)
                    .with_amv_target(2.0)
                    .with_margin(0.1),
            );

            let factuals = make_factuals_goods(&[1, 10]);
            let history = make_history(&[(1, 1.0, 0.9), (10, 2.0, 0.5)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());

            let sell = orders.iter().find(|o| o.is_sell_order()).expect("sell");
            let buy = orders.iter().find(|o| o.is_buy_order()).expect("buy");
            assert_eq!(sell.target, 10);
            assert_eq!(sell.target_amount, -5.0);
            assert_eq!(sell.amv_target, Some(2.2));
            assert_eq!(sell.counter_offer, Some(1));
            assert_eq!(buy.target, 10);
            assert_eq!(buy.target_amount, 5.0);
            assert_eq!(buy.amv_target, Some(1.8));
            assert_eq!(buy.counter_offer, Some(1));
            assert_eq!(buy.priority, market_priority::FIRM_MERCHANT);
        }

        #[test]
        fn sell_asks_for_market_money_not_on_hand_barter() {
            let mut firm = empty_firm();
            firm.property.insert(
                6,
                FirmPRow::new().with_quantity(4.0),
            );
            firm.property.insert(
                4,
                FirmPRow::new()
                    .with_quantity(8.0)
                    .with_sell_target(4.0)
                    .with_amv_target(8.0),
            );
            let factuals = make_factuals_goods(&[4, 5, 6]);
            let history = make_history(&[
                (4, 8.0, 0.7),
                (5, 0.21, 1.0),
                (6, 15.0, 1.0),
            ]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());
            let sell = orders.iter().find(|o| o.is_sell_order()).expect("sell");
            assert_eq!(sell.target, 4);
            assert_eq!(sell.counter_offer, Some(5));
        }

        #[test]
        fn sell_asks_for_needed_input_before_money() {
            let mut firm = empty_firm();
            firm.property.insert(
                2,
                FirmPRow::new()
                    .with_purchase_target(8.0)
                    .with_use_target(5.0)
                    .with_stock_target(10.0),
            );
            firm.property.insert(
                1,
                FirmPRow::new()
                    .with_quantity(12.0)
                    .with_sell_target(12.0)
                    .with_amv_target(1.2),
            );
            firm.property.insert(5, FirmPRow::new().with_quantity(40.0));
            let factuals = make_factuals_goods(&[1, 2, 5]);
            let history = make_history(&[
                (1, 1.2, 0.5),
                (2, 0.2, 0.35),
                (5, 0.21, 1.0),
            ]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());
            let sell = orders.iter().find(|o| o.is_sell_order()).expect("sell");
            assert_eq!(sell.target, 1);
            assert_eq!(sell.counter_offer, Some(2));
        }

        #[test]
        fn buy_posts_unclamped_bid_above_cap() {
            let mut firm = empty_firm();
            firm.property.insert(1, FirmPRow::new().with_quantity(50.0));
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_purchase_target(4.0)
                    .with_use_target(4.0)
                    .with_stock_target(4.0)
                    .with_amv_target(40.0)
                    .with_amv_bound(FirmAmvBound::Maximum(22.5)),
            );
            let factuals = make_factuals_goods(&[1, 10]);
            let history = make_history(&[(1, 1.0, 0.9), (10, 10.0, 0.4)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());
            let buy = orders.iter().find(|o| o.is_buy_order()).expect("buy");
            assert_eq!(buy.target, 10);
            assert_eq!(buy.amv_target, Some(40.0));
        }

        #[test]
        fn buy_posts_when_market_is_above_cap() {
            let mut firm = empty_firm();
            firm.property.insert(1, FirmPRow::new().with_quantity(50.0));
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_purchase_target(4.0)
                    .with_use_target(4.0)
                    .with_stock_target(4.0)
                    .with_amv_bound(FirmAmvBound::Maximum(22.5)),
            );
            let factuals = make_factuals_goods(&[1, 10]);
            let history = make_history(&[(1, 1.0, 0.9), (10, 30.0, 0.4)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());
            let buy = orders.iter().find(|o| o.target == 10 && o.target_amount > 0.0)
                .expect("buy");
            assert_eq!(buy.target_amount, 4.0);
            assert_eq!(buy.amv_target, Some(30.0));
        }

        #[test]
        fn sell_posts_unclamped_ask_below_floor() {
            let mut firm = empty_firm();
            firm.property.insert(1, FirmPRow::new().with_quantity(50.0));
            firm.property.insert(
                20,
                FirmPRow::new()
                    .with_quantity(12.0)
                    .with_sell_target(12.0)
                    .with_amv_target(20.0)
                    .with_amv_bound(FirmAmvBound::Minimum(25.0)),
            );
            let factuals = make_factuals_goods(&[1, 20]);
            let history = make_history(&[(1, 1.0, 0.9), (20, 20.0, 0.4)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());
            let sell = orders.iter().find(|o| o.is_sell_order()).expect("sell");
            assert_eq!(sell.target, 20);
            assert_eq!(sell.amv_target, Some(20.0));
        }

        #[test]
        fn origin_is_this_firm() {
            let mut firm = empty_firm();
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_quantity(4.0)
                    .with_sell_target(4.0),
            );
            let factuals = make_factuals_goods(&[10]);
            let history = make_history(&[(10, 1.0, 0.4)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());
            assert_eq!(orders[0].origin, Actor::Firm(7));
        }

        #[test]
        fn skips_non_positive_tender_price() {
            let mut firm = empty_firm();
            firm.property.insert(
                1,
                FirmPRow::new().with_quantity(10.0),
            );
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_quantity(20.0)
                    .with_purchase_target(5.0)
                    .with_sell_target(5.0)
                    .with_amv_target(2.0)
                    .with_margin(0.1),
            );

            let factuals = make_factuals_goods(&[1, 10]);
            let history = make_history(&[(1, -1.0, 0.9), (10, 2.0, 0.5)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());

            let sell = orders.iter().find(|o| o.target == 10 && o.target_amount < 0.0)
                .expect("outgoing");
            let buy = orders.iter().find(|o| o.target == 10 && o.target_amount > 0.0)
                .expect("incoming");
            assert!(sell.is_offer_order());
            assert!(!sell.is_sell_order());
            assert!(sell.counter_offer.is_none());
            assert!(buy.is_request_order());
            assert!(!buy.is_buy_order());
            assert!(buy.counter_offer.is_none());
        }

        #[test]
        fn skips_non_positive_tender_and_uses_next() {
            let mut firm = empty_firm();
            firm.property.insert(1, FirmPRow::new().with_quantity(10.0));
            firm.property.insert(2, FirmPRow::new().with_quantity(10.0));
            firm.property.insert(
                20,
                FirmPRow::new().with_purchase_target(2.0),
            );

            let factuals = make_factuals_goods(&[1, 2, 20]);
            let history = make_history(&[
                (1, -1.0, 0.95),
                (2, 1.0, 0.9),
                (20, 1.0, 0.4),
            ]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());

            assert_eq!(orders.len(), 1);
            assert!(orders[0].is_buy_order());
            assert_eq!(orders[0].target, 20);
            assert_eq!(orders[0].counter_offer, Some(2));
        }

        #[test]
        fn floors_fractional_sell_and_buy_to_whole_units() {
            let mut firm = empty_firm();
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_quantity(5.7)
                    .with_sell_target(10.0),
            );
            firm.property.insert(1, FirmPRow::new().with_quantity(20.0));
            firm.property.insert(
                20,
                FirmPRow::new().with_purchase_target(2.7),
            );

            let factuals = make_factuals_goods(&[1, 10, 20]);
            let history = make_history(&[
                (1, 1.0, 0.9),
                (10, 1.0, 0.4),
                (20, 1.0, 0.4),
            ]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());
            let sell = orders.iter().find(|o| o.target == 10).expect("sell");
            let buy = orders.iter().find(|o| o.target == 20).expect("buy");
            assert_eq!(sell.target_amount, -5.0);
            assert_eq!(buy.target_amount, 2.0);
        }

        #[test]
        fn ceils_named_counter_to_a_whole_payment() {
            let mut firm = empty_firm();
            firm.property.insert(1, FirmPRow::new().with_quantity(50.0));
            firm.property.insert(
                10,
                FirmPRow::new()
                    .with_purchase_target(1.0)
                    .with_amv_target(2.5),
            );

            let factuals = make_factuals_goods(&[1, 10]);
            let history = make_history(&[(1, 1.0, 0.9), (10, 2.5, 0.4)]);
            let orders = firm.create_orders(&history, &factuals, &HashSet::new());
            let buy = orders.iter().find(|o| o.is_buy_order()).expect("buy");
            assert_eq!(buy.target_amount, 1.0);
            assert_eq!(buy.counter_offer_amount, Some(-3.0));
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

    mod deal_should {
        use super::*;
        use crate::game::actor::Actor;
        use crate::game::config::market_priority;
        use crate::game::deal::{DealMaker, DealResponse, ProposedDeal};
        use crate::game::market::MarketHistory;
        use crate::game::marketorder::MarketOrder;

        fn make_history(entries: &[(usize, f64, f64)]) -> MarketHistory {
            let mut history = MarketHistory::new();
            for &(id, price, salability) in entries {
                history.prices.insert(id, price);
                history.salability.insert(id, salability);
            }
            history
        }

        fn make_factuals_goods(ids: &[usize]) -> Factuals {
            let mut factuals = Factuals::new();
            for &id in ids {
                factuals.goods.insert(id, make_good(id, "good", HashMap::new()));
            }
            factuals
        }

        fn empty_firm() -> Firm {
            Firm::new(7, "Shop".into(), 42, hexx::Hex::new(0, 0))
        }

        #[test]
        fn buy_tenders_free_stock_and_does_not_move_it() {
            let mut firm = empty_firm();
            firm.property.insert(1, FirmPRow::new().with_quantity(10.0));
            firm.property.insert(
                20,
                FirmPRow::new().with_purchase_target(4.0),
            );
            let factuals = make_factuals_goods(&[1, 20]);
            let history = make_history(&[(1, 1.0, 0.9), (20, 2.0, 0.4)]);
            let own = MarketOrder::request_order(
                Actor::Firm(7),
                20,
                4.0,
                market_priority::FIRM_PRODUCER,
            );
            let other = MarketOrder::offer_order(
                Actor::Firm(2),
                20,
                -4.0,
                market_priority::FIRM_PRODUCER,
            );

            let deal = firm.buy(&own, &other, &history, &factuals).expect("proposal");
            assert_eq!(deal.buyer, Actor::Firm(7));
            assert_eq!(deal.seller, Actor::Firm(2));
            assert!((deal.goods[&20] + 4.0).abs() < 1e-12);
            assert!((deal.goods[&1] - 8.0).abs() < 1e-12);
            assert_eq!(firm.property[&1].quantity, 10.0);
        }

        #[test]
        fn finalize_moves_stock_and_records_bought() {
            let mut firm = empty_firm();
            firm.property.insert(1, FirmPRow::new().with_quantity(10.0));
            firm.property.insert(
                20,
                FirmPRow::new().with_purchase_target(4.0),
            );
            let factuals = make_factuals_goods(&[1, 20]);
            let history = make_history(&[(1, 1.0, 0.9), (20, 2.0, 0.4)]);
            let own = MarketOrder::request_order(
                Actor::Firm(7),
                20,
                4.0,
                market_priority::FIRM_PRODUCER,
            );
            let other = MarketOrder::offer_order(
                Actor::Firm(2),
                20,
                -4.0,
                market_priority::FIRM_PRODUCER,
            );
            let deal = firm.buy(&own, &other, &history, &factuals).expect("proposal");
            firm.finalize(&deal, &history);
            assert!((firm.property[&1].quantity - 2.0).abs() < 1e-12);
            assert!((firm.property[&20].quantity - 4.0).abs() < 1e-12);
            assert!((firm.property[&20].bought - 4.0).abs() < 1e-12);
            assert!((firm.property[&20].bought_amv - 8.0).abs() < 1e-12);
        }

        #[test]
        fn evaluate_rejects_forty_percent_keep_without_need() {
            let firm = empty_firm();
            let factuals = make_factuals_goods(&[1, 20]);
            let history = make_history(&[(1, 1.0, 0.9), (20, 1.0, 0.4)]);
            let own = MarketOrder::offer_order(
                Actor::Firm(7),
                20,
                -4.0,
                market_priority::FIRM_PRODUCER,
            );
            let other = MarketOrder::request_order(
                Actor::Pop(1),
                20,
                4.0,
                market_priority::POP_START,
            );
            let deal = ProposedDeal::new(Actor::Pop(1), Actor::Firm(7))
                .with_good(20, -10.0)
                .with_good(1, 4.0);
            assert_eq!(
                firm.evaluate(&deal, &own, &other, &history, &factuals),
                DealResponse::Reject
            );
        }

        #[test]
        fn evaluate_uses_input_at_full_amv_despite_low_salability() {
            let mut firm = empty_firm();
            firm.property.insert(10, FirmPRow::new().with_use_target(4.0));
            let factuals = make_factuals_goods(&[10, 20]);
            let history = make_history(&[(10, 1.0, 0.3), (20, 1.0, 0.4)]);
            let own = MarketOrder::offer_order(
                Actor::Firm(7),
                20,
                -4.0,
                market_priority::FIRM_PRODUCER,
            );
            let other = MarketOrder::request_order(
                Actor::Pop(1),
                20,
                4.0,
                market_priority::POP_START,
            );
            let deal = ProposedDeal::new(Actor::Pop(1), Actor::Firm(7))
                .with_good(20, -4.0)
                .with_good(10, 4.0);
            assert_eq!(
                firm.evaluate(&deal, &own, &other, &history, &factuals),
                DealResponse::Accept
            );
        }

        #[test]
        fn evaluate_discounts_unused_low_salability_tender() {
            let firm = empty_firm();
            let factuals = make_factuals_goods(&[11, 20]);
            let history = make_history(&[(11, 1.0, 0.3), (20, 1.0, 0.4)]);
            let own = MarketOrder::offer_order(
                Actor::Firm(7),
                20,
                -4.0,
                market_priority::FIRM_PRODUCER,
            );
            let other = MarketOrder::request_order(
                Actor::Pop(1),
                20,
                4.0,
                market_priority::POP_START,
            );
            let deal = ProposedDeal::new(Actor::Pop(1), Actor::Firm(7))
                .with_good(20, -4.0)
                .with_good(11, 4.0);
            assert_eq!(
                firm.evaluate(&deal, &own, &other, &history, &factuals),
                DealResponse::Reject
            );
        }

        #[test]
        fn evaluate_takes_high_salability_money_at_full_amv() {
            let firm = empty_firm();
            let factuals = make_factuals_goods(&[12, 20]);
            let history = make_history(&[(12, 1.0, 1.0), (20, 1.0, 0.4)]);
            let own = MarketOrder::offer_order(
                Actor::Firm(7),
                20,
                -4.0,
                market_priority::FIRM_PRODUCER,
            );
            let other = MarketOrder::request_order(
                Actor::Pop(1),
                20,
                4.0,
                market_priority::POP_START,
            );
            let deal = ProposedDeal::new(Actor::Pop(1), Actor::Firm(7))
                .with_good(20, -4.0)
                .with_good(12, 4.0);
            assert_eq!(
                firm.evaluate(&deal, &own, &other, &history, &factuals),
                DealResponse::Accept
            );
        }

        #[test]
        fn evaluate_need_catch_accepts_forty_percent_keep_on_an_input() {
            let mut firm = empty_firm();
            firm.property.insert(
                20,
                FirmPRow::new().with_use_target(4.0).with_purchase_target(4.0),
            );
            let factuals = make_factuals_goods(&[1, 20]);
            let history = make_history(&[(1, 1.0, 0.9), (20, 1.0, 0.4)]);
            let own = MarketOrder::request_order(
                Actor::Firm(7),
                20,
                4.0,
                market_priority::FIRM_PRODUCER,
            );
            let other = MarketOrder::offer_order(
                Actor::Firm(2),
                20,
                -4.0,
                market_priority::FIRM_PRODUCER,
            );
            let deal = ProposedDeal::new(Actor::Firm(7), Actor::Firm(2))
                .with_good(20, -4.0)
                .with_good(1, 10.0);
            assert_eq!(
                firm.evaluate(&deal, &own, &other, &history, &factuals),
                DealResponse::Accept
            );
        }

        #[test]
        fn buy_skips_reserved_stock() {
            let mut firm = empty_firm();
            firm.property.insert(
                1,
                FirmPRow::new()
                    .with_quantity(10.0)
                    .with_reserve_target(10.0)
                    .with_reserve(10.0),
            );
            firm.property.insert(
                20,
                FirmPRow::new().with_purchase_target(4.0),
            );
            let factuals = make_factuals_goods(&[1, 20]);
            let history = make_history(&[(1, 1.0, 0.9), (20, 2.0, 0.4)]);
            let own = MarketOrder::request_order(
                Actor::Firm(7),
                20,
                4.0,
                market_priority::FIRM_PRODUCER,
            );
            let other = MarketOrder::offer_order(
                Actor::Firm(2),
                20,
                -4.0,
                market_priority::FIRM_PRODUCER,
            );
            assert!(firm.buy(&own, &other, &history, &factuals).is_none());
        }

        #[test]
        fn buy_does_not_tender_stock_on_a_sell_order() {
            let mut firm = empty_firm();
            // 15 bread, sell_target 12, salability at the exchange floor:
            // 12 sell / 3 exchange. Only the 3 exchange units are tenderable.
            firm.property.insert(
                1,
                FirmPRow::new()
                    .with_quantity(15.0)
                    .with_sell_target(12.0),
            );
            firm.property.insert(
                20,
                FirmPRow::new().with_purchase_target(4.0),
            );
            let factuals = make_factuals_goods(&[1, 20]);
            let history = make_history(&[(1, 1.0, 0.6), (20, 1.0, 0.4)]);
            let own = MarketOrder::request_order(
                Actor::Firm(7),
                20,
                4.0,
                market_priority::FIRM_PRODUCER,
            );
            let other = MarketOrder::offer_order(
                Actor::Firm(2),
                20,
                -4.0,
                market_priority::FIRM_PRODUCER,
            );

            let deal = firm.buy(&own, &other, &history, &factuals).expect("proposal");
            assert!((deal.goods[&20] + 3.0).abs() < 1e-12);
            assert!((deal.goods[&1] - 3.0).abs() < 1e-12);
        }

        #[test]
        fn buy_proposes_when_request_exceeds_row_buy_cap() {
            let mut firm = empty_firm();
            firm.property.insert(1, FirmPRow::new().with_quantity(20.0));
            firm.property.insert(
                20,
                FirmPRow::new()
                    .with_purchase_target(4.0)
                    .with_amv_bound(FirmAmvBound::Maximum(1.5)),
            );
            let factuals = make_factuals_goods(&[1, 20]);
            let history = make_history(&[(1, 1.0, 0.9), (20, 1.0, 0.4)]);
            let own = MarketOrder::request_order(
                Actor::Firm(7),
                20,
                4.0,
                market_priority::FIRM_PRODUCER,
            );
            let other = MarketOrder::sell_order(
                Actor::Firm(2),
                20,
                -4.0,
                2.0,
                1,
                8.0,
                market_priority::FIRM_PRODUCER,
            );
            let deal = firm.buy(&own, &other, &history, &factuals).expect("proposal");
            assert!((deal.goods[&20] + 4.0).abs() < 1e-12);
            assert!((deal.goods[&1] - 8.0).abs() < 1e-12);
        }
    }

    mod pay_wage_shares_should {
        use super::*;
        use crate::game::actor::Actor;
        use crate::game::config::GameConfig;
        use crate::game::household::Household;
        use crate::game::pop::{DemoRow, Pop, PopRecords};
        use crate::game::sentiment::Sentiment;
        use crate::game::workforce::Workforce;

        const COIN: usize = 5;

        fn make_pop(id: usize) -> Pop {
            Pop {
                id,
                job: 0,
                property: HashMap::new(),
                desires: vec![vec![]; 3],
                working_desires: vec![],
                demographics: DemoRow {
                    household: Household::with_count(10.0),
                    species: 0,
                    culture: 0,
                    class: 0,
                    religion: 0,
                },
                current_orders: vec![],
                stored_effects: vec![],
                sentiment: Sentiment::new(),
                records: PopRecords::default(),
            }
        }

        fn worker(id: usize) -> Workforce {
            let mut w = Workforce::empty();
            w.id = id;
            w
        }

        #[test]
        fn unowned_till_pays_workers_and_keeps_the_owner_share() {
            let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0));
            firm.property.insert(COIN, FirmPRow::new().with_quantity(10.0));
            firm.workforce.push(worker(2));
            let mut pops = HashMap::from([(2, make_pop(2))]);

            let payout = firm.pay_wage_shares(&mut pops, COIN, 1.0, &GameConfig::default());

            assert_eq!(payout.coinage, 10.0);
            assert_eq!(payout.owner_amount, 0.0);
            assert!(!payout.owner_credited);
            assert_eq!(payout.worker_amount, 3.0);
            assert_eq!(payout.workers, vec![(2, 3.0)]);
            assert_eq!(firm.property[&COIN].quantity, 7.0);
            assert_eq!(pops[&2].property[&COIN].quantity, 3.0);
            assert_eq!(pops[&2].records.income_amv, 3.0);
        }

        #[test]
        fn owner_pop_receives_the_owner_share() {
            let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0));
            firm.property.insert(COIN, FirmPRow::new().with_quantity(10.0));
            firm.owners.owner = Actor::Pop(3);
            let mut pops = HashMap::from([(3, make_pop(3))]);

            let payout = firm.pay_wage_shares(&mut pops, COIN, 1.0, &GameConfig::default());

            assert!(payout.owner_credited);
            assert_eq!(payout.owner_amount, 3.0);
            assert_eq!(payout.worker_amount, 0.0);
            assert_eq!(firm.property[&COIN].quantity, 7.0);
            assert_eq!(pops[&3].property[&COIN].quantity, 3.0);
        }

        #[test]
        fn living_owner_is_paid_before_workers() {
            let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0));
            firm.property.insert(COIN, FirmPRow::new().with_quantity(10.0));
            firm.owners.owner = Actor::Pop(3);
            firm.workforce.push(worker(2));
            let mut pops = HashMap::from([(2, make_pop(2)), (3, make_pop(3))]);

            let payout = firm.pay_wage_shares(&mut pops, COIN, 1.0, &GameConfig::default());

            assert!(payout.owner_credited);
            assert_eq!(payout.owner_amount, 3.0);
            assert_eq!(payout.worker_amount, 3.0);
            assert_eq!(payout.workers, vec![(2, 3.0)]);
            assert_eq!(firm.property[&COIN].quantity, 4.0);
            assert_eq!(pops[&3].property[&COIN].quantity, 3.0);
            assert_eq!(pops[&2].property[&COIN].quantity, 3.0);
        }

        #[test]
        fn one_coin_goes_to_workers_when_there_is_no_owner() {
            let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0));
            firm.property.insert(COIN, FirmPRow::new().with_quantity(1.0));
            firm.workforce.push(worker(2));
            let mut pops = HashMap::from([(2, make_pop(2))]);

            let payout = firm.pay_wage_shares(&mut pops, COIN, 1.0, &GameConfig::default());

            assert_eq!(payout.owner_amount, 0.0);
            assert_eq!(payout.worker_amount, 1.0);
            assert_eq!(payout.workers, vec![(2, 1.0)]);
            assert_eq!(firm.property[&COIN].quantity, 0.0);
            assert_eq!(pops[&2].property[&COIN].quantity, 1.0);
        }

        #[test]
        fn splits_the_worker_share_across_listed_pops() {
            let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0));
            firm.property.insert(COIN, FirmPRow::new().with_quantity(20.0));
            firm.workforce.push(worker(2));
            firm.workforce.push(worker(3));
            let mut pops = HashMap::from([(2, make_pop(2)), (3, make_pop(3))]);

            let payout = firm.pay_wage_shares(&mut pops, COIN, 1.0, &GameConfig::default());

            assert_eq!(payout.owner_amount, 0.0);
            assert_eq!(payout.worker_amount, 6.0);
            assert_eq!(payout.workers, vec![(2, 3.0), (3, 3.0)]);
            assert_eq!(pops[&2].property[&COIN].quantity, 3.0);
            assert_eq!(pops[&3].property[&COIN].quantity, 3.0);
            assert_eq!(firm.property[&COIN].quantity, 14.0);
        }

        #[test]
        fn missing_workers_leave_the_worker_share_in_the_till() {
            let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0));
            firm.property.insert(COIN, FirmPRow::new().with_quantity(10.0));
            firm.workforce.push(worker(9));
            let mut pops = HashMap::new();

            let payout = firm.pay_wage_shares(&mut pops, COIN, 1.0, &GameConfig::default());

            assert_eq!(payout.owner_amount, 0.0);
            assert_eq!(payout.worker_amount, 0.0);
            assert_eq!(firm.property[&COIN].quantity, 10.0);
        }

        #[test]
        fn input_free_producer_pays_out_the_whole_till() {
            let mut firm = Firm::new(1, "mine".into(), 1, hexx::Hex::new(0, 0));
            firm.production_line.push(super::empty_production_line(3));
            firm.property.insert(COIN, FirmPRow::new().with_quantity(10.0));
            firm.owners.owner = Actor::Pop(3);
            firm.workforce.push(worker(2));
            let mut pops = HashMap::from([(2, make_pop(2)), (3, make_pop(3))]);

            let payout = firm.pay_wage_shares(&mut pops, COIN, 1.0, &GameConfig::default());

            assert!(payout.owner_credited);
            assert_eq!(payout.owner_amount, 5.0);
            assert_eq!(payout.worker_amount, 5.0);
            assert_eq!(payout.workers, vec![(2, 5.0)]);
            assert_eq!(firm.property[&COIN].quantity, 0.0);
            assert_eq!(pops[&3].property[&COIN].quantity, 5.0);
            assert_eq!(pops[&2].property[&COIN].quantity, 5.0);
        }
    }
}
