use std::{collections::HashMap};

use hexx::Hex;

use crate::game::{contract::Contract, factuals::Factuals, firmorganization::FirmOrganization, market::Market, process::ProcessEffect, workforce::Workforce};

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
    /// - Used capital goods are removed from `quantity` **and** recorded into the
    ///   `used_capital` field on the corresponding `FirmPRow` (to be returned at 
    ///   the end of the day).
    /// - Records success rate, iterations, effects, missing goods, and AMV 
    ///   of the goods involved on each `ProductionLine`.
    /// 
    /// Returns a `ProductionReport` containing:
    /// - All `ProcessEffect`s produced (research, culture, growth...)
    /// - Consolidated `produced` and `consumed` quantities across every process run.
    /// 
    /// Only reads from `self.property` for available stock. The `market` parameter is
    /// used solely to snapshot current AMV values for record-keeping.
    /// 
    /// ## Panic
    /// 
    /// Panics if good or process is not found in factuals.
    pub fn run_production(&mut self, factuals: &Factuals, market: &Market) -> ProductionReport {
        let mut report = ProductionReport::default();

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

            // Apply net changes to property (outputs + consumed inputs + decay)
            for (&good_id, &delta) in &result.changes {
                if let Some(row) = self.property.get_mut(&good_id) {
                    // if already in property, add delta
                    row.quantity += delta;
                    debug_assert!(row.quantity >= 0.0, "Quantity should never be negative!");
                } else if delta > 0.0 {
                    // New good produced — create row with sensible defaults
                    self.property.insert(
                        good_id,
                        FirmPRow {
                            quantity: delta,
                            rolling_average: 0.0,
                            target: 0.0,
                            reserve: 0.0,
                            average_cost: 0.0,
                            used_capital: 0.0,
                        },
                    );
                } else if delta < 0.0 {
                    unreachable!("A sanity checkpoint, we should never consume goods we don't have.");
                }
            }

            // Remove used capital from quantity and record it in the row for later return
            for (&good_id, &used) in &result.used_inputs {
                if let Some(row) = self.property.get_mut(&good_id) {
                    row.quantity -= used;
                    debug_assert!(row.quantity >= 0.0, "Quantity should never be negative.");
                    row.used_capital += used;
                }
            }

            // --- Record AMV snapshots and build consolidated produced/consumed ---
            for (&good_id, &delta) in &result.changes {
                let amv = if let Some(good) = market.goods.get(&good_id) {
                    good.amv
                } else { 1.0 };

                if delta > 0.0 {
                    // Produced (outputs + decay results)
                    *report.produced.entry(good_id).or_insert(0.0) += delta;
                    line.last_amv_produced += amv * delta;
                } else if delta < 0.0 {
                    // Consumed (Destroyed or Consumed input types)
                    let consumed_qty = -delta;
                    *report.consumed.entry(good_id).or_insert(0.0) += consumed_qty;
                    line.last_amv_consumed -= amv * delta;
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
            report.effects.extend(result.effects);
        }

        report
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
    /// The ID of the pop who owns the firm.
    pub pop: usize,
}
impl Owners {
    pub fn empty() -> Self {
        Owners {
            pop: 0,
        }
    }
}

/// Consolidated record of everything a production run created and consumed.
/// Returned by `Firm::run_production` so a single object can be used by both
/// the Market (to increment `MarketGood.production` / `consumption`) and by
/// the Firm for its own record-keeping and later AMV/productivity analysis.
#[derive(Debug, Clone, Default)]
pub struct ProductionReport {
    pub effects: Vec<ProcessEffect>,
    /// Total quantity of each good created by production (outputs + decay results).
    pub produced: HashMap<usize, f64>,
    /// Total quantity of each good destroyed/consumed as non-capital inputs.
    pub consumed: HashMap<usize, f64>,
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
#[derive(Debug, Clone, Default)]
pub struct FirmPRow {
    /// The amount currently owned.
    pub quantity: f64,
    /// The average ownership of the good over the last 30 days.
    pub rolling_average: f64,
    /// The target amount the firm wants to have after shopping.
    /// This should be before production processes.
    pub target: f64,
    /// If trading the good, this is the amount that the firm will never willingly 
    /// part with.
    pub reserve: f64,

    /// The average cost to get these good so far. Updated after each purchase and
    /// productive process.
    /// Used for value production efficiency calculations.
    pub average_cost: f64,

    /// Amount of this good currently tied up as capital in active production runs.
    /// Removed from `quantity` during `run_production`; returned later.
    pub used_capital: f64,
}