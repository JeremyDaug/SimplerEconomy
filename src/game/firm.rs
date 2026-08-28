use std::collections::HashMap;

use hexx::Hex;

use crate::game::{
    actor::Actor, config::{market_constants, market_priority}, contract::Contract, factuals::Factuals, firmorganization::FirmOrganization, good::GoodTag, market::{Market, MarketHistory}, marketorder::{compose_sell_priority, MarketOrder}, pop::Pop, process::ProcessEffect, util::lerp, workforce::Workforce,
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

    /// End-of-day bookkeeping for this firm (production stats, costs, …).
    /// Only external input is factuals.
    pub fn record_keeping(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Firm record keeping")
    }

    /// # Decay Goods
    ///
    /// End-of-day decay for this firm.
    ///
    /// 1. Return `used` capital to `quantity` and clear `used` so it can decay.
    /// 2. Decay on-hand `quantity` by each good's `decay_rate` (skip Exposure while owned).
    /// 3. Apply decay byproducts. Do not stamp them as `produced`.
    /// 4. [`FirmPRow::sync_reserve`] after quantity changes.
    ///
    /// `consumed` on the row is a day-flow counter (goods already left `quantity`
    /// during production, and Consumed-type byproducts were applied there). It is
    /// not destroyed again here. Clear it with [`Firm::clear_day_flows`].
    pub fn decay_goods(&mut self, factuals: &Factuals) {
        let mut gains: HashMap<usize, f64> = HashMap::new();

        for (&good_id, row) in self.property.iter_mut() {
            if row.used != 0.0 {
                row.quantity += row.used;
                row.used = 0.0;
            }

            let good = factuals.find_good(good_id);
            let exposure = good.tags.contains(&GoodTag::Exposure);
            if !exposure && good.decay_rate > 0.0 && row.quantity > 0.0 {
                let lost = row.quantity * good.decay_rate;
                row.quantity -= lost;
                debug_assert!(row.quantity >= 0.0, "Quantity should never be negative!");
                for (&byproduct, &ratio) in &good.decay_result {
                    if ratio != 0.0 && lost != 0.0 {
                        *gains.entry(byproduct).or_insert(0.0) += lost * ratio;
                    }
                }
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
    }

    /// # Clear Day Flows
    ///
    /// Zero today's exchange and production counters on every property row:
    /// `produced`, `consumed`, `bought`, `bought_amv`, `sold`, `sold_amv`.
    ///
    /// Leaves `used` alone (returned in [`Firm::decay_goods`]) and does not
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
        }
    }

    /// # Create Orders
    ///
    /// Turns current [`FirmPRow`] targets and on-hand stock into market orders.
    /// Read-only: does not edit the firm.
    ///
    /// 1. Classify each tradeable row's free on-hand pile as **sell**, **exchange**,
    ///    and/or **liquidate**. Production-fenced stock is not in that pile.
    /// 2. Emit sell/offer orders, then buy/request orders funded by exchange AMV
    ///    plus expected sell and liquidate AMV (optimistic: assumes outgoing fills).
    ///
    /// Exchange if salability >= [`market_constants::EXCHANGE_SALABILITY_MIN`].
    /// Dedicated sell if `sell_target` > 0. When both apply, salability lerps the
    /// free pile from 90% sell / 10% exchange at the exchange floor to 10% sell /
    /// 90% exchange at salability 1.0. Exchange units are rounded to nearest;
    /// sell is the remainder, then capped at `sell_target` (overflow stays
    /// exchange).
    ///
    /// Liquidate if the row has free stock and no purchase, sell, or use target,
    /// and it is not exchange-eligible. Those units are leftover barter and go
    /// out as offer orders, never priced sell orders.
    ///
    /// Dual buy+sell: producer inputs (`use_target` > 0) buy only the stock-target
    /// shortfall and sell only free excess. Merchants (no `use_target`) emit the
    /// full `purchase_target` even above stock target. Buy is incoming stock, not
    /// an on-hand role, so a row may still buy and sell the same good.
    ///
    /// Buys stop when spendable AMV is exhausted; the last buy may overdraw.
    /// AMV on orders is stamped for later settlement. Matching does not use it yet.
    /// Buy order priority is the merchant band if any row is merchant-like
    /// (purchase and sell, no use), otherwise the producer band. Sells use
    /// [`compose_sell_priority`].
    pub fn create_orders(&self, history: &MarketHistory, factuals: &Factuals) -> Vec<MarketOrder> {
        let mut line_rank: HashMap<usize, usize> = HashMap::new();
        for (idx, line) in self.production_line.iter().enumerate() {
            for &good_id in &line.inputs {
                line_rank.entry(good_id).or_insert(idx);
            }
        }

        let mut plans: Vec<RowPlan> = Vec::new();
        let mut merchant_like = false;

        for (&good, row) in &self.property {
            if !factuals.find_good(good).is_buyable() {
                continue;
            }

            if row.purchase_target > 0.0 && row.sell_target > 0.0 && row.use_target == 0.0 {
                merchant_like = true;
            }

            let salability = history.salability(good);
            let mid = row.mid_amv(history.price(good));
            let split = classify_on_hand(row, salability);
            let buy_qty = row.purchase_qty();

            debug_assert!(split.sell >= 0.0, "sell_qty must be >= 0.0");
            debug_assert!(split.exchange >= 0.0, "exchange_qty must be >= 0.0");
            debug_assert!(split.liquidate >= 0.0, "liquidate_qty must be >= 0.0");
            debug_assert!(buy_qty >= 0.0, "buy_qty must be >= 0.0");
            debug_assert!(
                split.liquidate == 0.0 || (split.sell == 0.0 && split.exchange == 0.0),
                "liquidate stock cannot also be sell or exchange"
            );

            if buy_qty == 0.0
                && split.sell == 0.0
                && split.exchange == 0.0
                && split.liquidate == 0.0
            {
                continue;
            }

            plans.push(RowPlan {
                good,
                buy_qty,
                sell_qty: split.sell,
                exchange_qty: split.exchange,
                liquidate_qty: split.liquidate,
                use_target: row.use_target,
                bid: row.bid_amv(mid),
                ask: row.ask_amv(mid),
                salability,
                line_rank: line_rank.get(&good).copied().unwrap_or(usize::MAX),
            });
        }

        let buy_band = if merchant_like {
            market_priority::FIRM_MERCHANT
        } else {
            market_priority::FIRM_PRODUCER
        };

        let mut exchange_goods: Vec<(usize, f64, f64)> = plans
            .iter()
            .filter(|plan| plan.exchange_qty > 0.0)
            .map(|plan| (plan.good, plan.salability, history.price(plan.good)))
            .collect();
        exchange_goods.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });

        let mut spendable = 0.0;
        for plan in &plans {
            let price = history.price(plan.good);
            if price > 0.0 {
                spendable += plan.exchange_qty * price;
                spendable += plan.liquidate_qty * price;
            }
            if plan.ask > 0.0 {
                spendable += plan.sell_qty * plan.ask;
            }
        }

        let mut orders: Vec<MarketOrder> = Vec::new();
        let mut outgoing: Vec<&RowPlan> = plans
            .iter()
            .filter(|plan| plan.sell_qty > 0.0 || plan.liquidate_qty > 0.0)
            .collect();
        outgoing.sort_by_key(|plan| plan.good);
        for plan in outgoing {
            let (qty, liquidate) = if plan.liquidate_qty > 0.0 {
                (plan.liquidate_qty, true)
            } else {
                (plan.sell_qty, false)
            };
            let weight = compose_sell_priority(buy_band, qty, 0.0);
            if liquidate {
                orders.push(MarketOrder::offer_order(
                    Actor::Firm(self.id),
                    plan.good,
                    -qty,
                    weight,
                ));
            } else if let Some((pay_good, pay_price)) =
                counter_good(&exchange_goods, plan.good)
            {
                orders.push(MarketOrder::sell_order(
                    Actor::Firm(self.id),
                    plan.good,
                    -qty,
                    plan.ask,
                    pay_good,
                    qty * plan.ask / pay_price,
                    weight,
                ));
            } else {
                orders.push(MarketOrder::offer_order(
                    Actor::Firm(self.id),
                    plan.good,
                    -qty,
                    weight,
                ));
            }
        }

        let mut buys: Vec<&RowPlan> = plans.iter().filter(|plan| plan.buy_qty > 0.0).collect();
        buys.sort_by(|a, b| {
            let a_prod = if a.use_target > 0.0 { 0 } else { 1 };
            let b_prod = if b.use_target > 0.0 { 0 } else { 1 };
            a_prod
                .cmp(&b_prod)
                .then(a.line_rank.cmp(&b.line_rank))
                .then(a.good.cmp(&b.good))
        });

        let mut remaining = spendable;
        for plan in buys {
            if remaining <= 0.0 {
                break;
            }
            let cost = plan.buy_qty * plan.bid;
            if let Some((pay_good, pay_price)) = counter_good(&exchange_goods, plan.good) {
                orders.push(MarketOrder::buy_order(
                    Actor::Firm(self.id),
                    plan.good,
                    plan.buy_qty,
                    plan.bid,
                    pay_good,
                    -(plan.buy_qty * plan.bid / pay_price),
                    buy_band,
                ));
            } else {
                orders.push(MarketOrder::request_order(
                    Actor::Firm(self.id),
                    plan.good,
                    plan.buy_qty,
                    buy_band,
                ));
            }
            remaining -= cost;
        }

        orders
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
    /// - Applies all good changes (consumed inputs, produced outputs, decay results)
    ///   directly to `property` quantities. New output goods are auto-created.
    /// - Stamps day-flows on each [`FirmPRow`]: `produced` for positive changes
    ///   (outputs + decay results), `consumed` for destroyed/consumed inputs.
    ///   Those two input types are not distinguished on the row; decay products of
    ///   Consumed inputs show up as `produced` on their result goods.
    /// - Used capital goods are removed from `quantity` **and** recorded into the
    ///   `used` field on the corresponding `FirmPRow` (to be returned at
    ///   the end of the day). Capital is never added to `consumed`.
    ///   Later: fold capital cost / maintenance / amortization into output
    ///   `average_cost`. Capital should wear; it is not indestructible. Not
    ///   needed for v0 cost blending.
    /// - Factors are left untouched (not consumed, used, or locked).
    /// - After quantity changes, [`FirmPRow::sync_reserve`] matches `reserve` to
    ///   `min(quantity, reserve_target)`.
    /// - Output `average_cost` blends this run's input AMV (allocated by each
    ///   output's share of `last_amv_produced`) into existing inventory cost basis.
    /// - Records success rate, iterations, effects, missing goods, and AMV 
    ///   of the goods involved on each `ProductionLine`.
    /// 
    /// Returns `ProcessEffect`s (research, culture, growth...) for the caller to
    /// apply elsewhere. Good flows live on the property rows; market
    /// production/consumption totals should sum those rows.
    /// 
    /// Only reads from `self.property` for available stock. The `market` parameter is
    /// used solely to snapshot current AMV values for record-keeping.
    /// 
    /// ## Panic
    /// 
    /// Panics if good or process is not found in factuals.
    pub fn run_production(&mut self, factuals: &Factuals, market: &Market) -> Vec<ProcessEffect> {
        let mut effects = Vec::new();

        for line in &mut self.production_line {
            // if process is not found, panic
            let Some(process) = factuals.processes.get(&line.process) else {
                panic!("Process not found!");
            };

            // Snapshot of available goods from this firm's property only
            let available: HashMap<usize, f64> = self
                .property
                .iter()
                .map(|(&gid, row)| (gid, row.quantity))
                .collect();

            let result = process.do_process(&available, line.target, factuals);

            // This-run AMV snapshots; leftover values would poison cost blending.
            line.last_amv_consumed = 0.0;
            line.last_amv_produced = 0.0;

            // Apply net changes to property (outputs + consumed inputs + decay)
            // and stamp produced / consumed day-flows on each row.
            for (&good_id, &delta) in &result.changes {
                let amv = if let Some(good) = market.goods.get(&good_id) {
                    good.amv
                } else { 1.0 };

                if delta > 0.0 {
                    // Produced (outputs + decay results of Consumed inputs)
                    let row = self.property.entry(good_id).or_insert_with(FirmPRow::new);
                    row.quantity += delta;
                    row.produced += delta;
                    row.sync_reserve();
                    debug_assert!(row.quantity >= 0.0, "Quantity should never be negative!");
                    line.last_amv_produced += amv * delta;
                } else if delta < 0.0 {
                    // Destroyed or Consumed inputs; both stamp `consumed`.
                    let consumed_qty = -delta;
                    let Some(row) = self.property.get_mut(&good_id) else {
                        unreachable!("A sanity checkpoint, we should never consume goods we don't have.");
                    };
                    row.quantity += delta;
                    row.consumed += consumed_qty;
                    row.sync_reserve();
                    debug_assert!(row.quantity >= 0.0, "Quantity should never be negative!");
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

            // Remove used capital from quantity and record it in the row for later return
            for (&good_id, &used) in &result.used_inputs {
                if let Some(row) = self.property.get_mut(&good_id) {
                    row.quantity -= used;
                    debug_assert!(row.quantity >= 0.0, "Quantity should never be negative.");
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
}

impl Owners {
    pub fn empty() -> Self {
        Owners {
            owner: Actor::Pop(0),
            priority_override: None
        }
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
    /// Removed from `quantity` during `run_production`; returned later.
    pub used: f64,
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

    /// Blend `added` units at `unit_cost` into inventory cost basis.
    /// `quantity` must already include `added`.
    pub fn blend_average_cost(&mut self, added: f64, unit_cost: f64) {
        debug_assert!(self.quantity >= 0.0, "quantity must be >= 0.0");
        if self.quantity > 0.0 {
            let previous = (self.quantity - added).max(0.0);
            self.average_cost =
                (previous * self.average_cost + added * unit_cost) / self.quantity;
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

/// Per-row shopping plan built by [`Firm::create_orders`].
struct RowPlan {
    good: usize,
    buy_qty: f64,
    sell_qty: f64,
    exchange_qty: f64,
    liquidate_qty: f64,
    use_target: f64,
    bid: f64,
    ask: f64,
    salability: f64,
    line_rank: usize,
}

/// Split of free on-hand stock for [`classify_on_hand`].
struct OnHandSplit {
    sell: f64,
    exchange: f64,
    liquidate: f64,
}

impl OnHandSplit {
    fn empty() -> Self {
        Self {
            sell: 0.0,
            exchange: 0.0,
            liquidate: 0.0,
        }
    }
}

/// Split free on-hand stock into sell, exchange, and/or liquidate.
/// Production-fenced units are already excluded by [`FirmPRow::free_for_market`].
fn classify_on_hand(row: &FirmPRow, salability: f64) -> OnHandSplit {
    let free = row.free_for_market();
    if free <= 0.0 {
        return OnHandSplit::empty();
    }

    let trading = row.purchase_target > 0.0
        || row.sell_target > 0.0
        || row.use_target > 0.0;
    let can_exchange = salability >= market_constants::EXCHANGE_SALABILITY_MIN;

    if !trading {
        if can_exchange {
            return OnHandSplit {
                sell: 0.0,
                exchange: free,
                liquidate: 0.0,
            };
        }
        return OnHandSplit {
            sell: 0.0,
            exchange: 0.0,
            liquidate: free,
        };
    }

    let can_sell = row.sell_target > 0.0;
    if can_sell && can_exchange {
        let span = 1.0 - market_constants::EXCHANGE_SALABILITY_MIN;
        let t = if span > 0.0 {
            ((salability - market_constants::EXCHANGE_SALABILITY_MIN) / span).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let edge = market_constants::SELL_EXCHANGE_EDGE;
        let exchange_frac = lerp(edge, 1.0 - edge, t);
        let mut exchange_qty = round_units(free * exchange_frac).clamp(0.0, free);
        let mut sell_qty = free - exchange_qty;
        if sell_qty > row.sell_target {
            exchange_qty += sell_qty - row.sell_target;
            sell_qty = row.sell_target;
        }
        OnHandSplit {
            sell: sell_qty,
            exchange: exchange_qty,
            liquidate: 0.0,
        }
    } else if can_sell {
        OnHandSplit {
            sell: row.sell_target.min(free),
            exchange: 0.0,
            liquidate: 0.0,
        }
    } else if can_exchange {
        OnHandSplit {
            sell: 0.0,
            exchange: free,
            liquidate: 0.0,
        }
    } else {
        OnHandSplit::empty()
    }
}

/// Round a non-negative unit count half-up to a whole number.
fn round_units(amount: f64) -> f64 {
    debug_assert!(amount >= 0.0, "amount must be >= 0.0");
    (amount + 0.5).floor()
}

/// First exchange tender that is not `exclude`, as (good id, unit price).
/// Skips non-positive AMV so counter amounts keep the buy/sell sign.
fn counter_good(exchange_goods: &[(usize, f64, f64)], exclude: usize) -> Option<(usize, f64)> {
    exchange_goods.iter().find_map(|&(good, _, price)| {
        if good != exclude && price > 0.0 {
            debug_assert!(price.is_finite(), "tender AMV must be finite");
            Some((good, price))
        } else {
            None
        }
    })
}

#[cfg(test)]
mod firm {
    use crate::game::factuals::Factuals;
    use crate::game::good::Good; // if you need Good defs
    use crate::game::market::{Market, MarketGood};
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput, ProcessEffect};
    use std::collections::{HashMap, HashSet};
    use crate::game::firm::{Firm, FirmPRow, ProductionLine};

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
            assert_eq!(firm.property[&20].quantity, 5.0); // 5 iterations * 1.0
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

            assert_eq!(firm.property[&20].quantity, 4.0);
            assert_eq!(firm.property[&20].reserve, 4.0);
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

            assert_eq!(firm.property[&20].quantity, 4.0);
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
            assert_eq!(firm.property[&40].quantity, 2.0);

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
            assert_eq!(firm.property[&99].quantity, 10.0);
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
            assert_eq!(firm.property[&110].quantity, 2.0);    // produced 5, consumed 3, 
            assert_eq!(firm.property[&200].used, 8.0); // 5 + 3
            assert_eq!(firm.property[&200].consumed, 0.0); // capital never in consumed
            assert_eq!(firm.property[&200].quantity, 12.0);    // 20- 5 - 3
            // (adjust expected numbers based on exact per-iter costs you want)

            // Row day-flows aggregated across both lines
            assert_eq!(firm.property[&110].produced, 5.0); // planks created
            assert_eq!(firm.property[&110].consumed, 3.0); // planks consumed in line 2
            assert_eq!(firm.property[&120].produced, 3.0); // tables created
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
            assert_eq!(firm.property[&120].quantity, 8.0);
            assert_eq!(firm.property[&130].quantity, 13.0);
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
            let orders = firm.create_orders(&history, &factuals);

            assert_eq!(orders.len(), 2);
            assert!(orders[0].is_offer_order());
            assert_eq!(orders[0].target, 20);
            assert_eq!(orders[0].target_amount, -12.0);
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
            let orders = firm.create_orders(&history, &factuals);

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
            let orders = firm.create_orders(&history, &factuals);

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
            let orders = firm.create_orders(&history, &factuals);

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
            let orders = firm.create_orders(&history, &factuals);

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
            let orders = firm.create_orders(&history, &factuals);

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
            let orders = firm.create_orders(&history, &factuals);

            let outgoing = orders_for(&orders, 10);
            assert_eq!(outgoing.len(), 1);
            assert_eq!(outgoing[0].target_amount, -2.0);
            assert!(orders.iter().any(|o| o.target == 20 && o.counter_offer == Some(10)));
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
            let orders = firm.create_orders(&history, &factuals);

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
            let orders = firm.create_orders(&history, &factuals);

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
            let orders = firm.create_orders(&history, &factuals);

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
            let orders = firm.create_orders(&history, &factuals);

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

            let orders = firm.create_orders(&history, &factuals);
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
            let orders = firm.create_orders(&history, &factuals);
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
            let _ = firm.create_orders(&history, &factuals);
            assert_eq!(firm.property[&10].quantity, before.property[&10].quantity);
            assert_eq!(
                firm.property[&10].purchase_target,
                before.property[&10].purchase_target
            );
            assert_eq!(firm.property[&10].sell_target, before.property[&10].sell_target);
        }

        #[test]
        fn merchant_with_tender_stamps_bid_ask_spread() {
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
            let orders = firm.create_orders(&history, &factuals);

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
            let orders = firm.create_orders(&history, &factuals);
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
            let orders = firm.create_orders(&history, &factuals);

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
            let orders = firm.create_orders(&history, &factuals);

            assert_eq!(orders.len(), 1);
            assert!(orders[0].is_buy_order());
            assert_eq!(orders[0].target, 20);
            assert_eq!(orders[0].counter_offer, Some(2));
        }
    }
}
