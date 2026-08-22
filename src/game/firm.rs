use std::collections::HashMap;

use hexx::Hex;

use crate::game::{
    actor::Actor,
    contract::Contract,
    factuals::Factuals,
    firmorganization::FirmOrganization,
    market::Market,
    pop::Pop,
    process::ProcessEffect,
    workforce::Workforce,
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

    /// End-of-day good decay for this firm (stock, used capital, byproducts).
    /// Only external input is factuals.
    pub fn decay_goods(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Firm decay goods")
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
    pub owner: Actor,
}
impl Owners {
    pub fn empty() -> Self {
        Owners {
            owner: Actor::Pop(0),
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


#[cfg(test)]
mod firm {
    use crate::game::factuals::Factuals;
    use crate::game::good::Good; // if you need Good defs
    use crate::game::market::{Market, MarketGood};
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput, ProcessEffect};
    use std::collections::{HashMap, HashSet};
    use crate::game::firm::{Firm, FirmPRow, ProductionLine, ProductionReport};

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
        FirmPRow {
            quantity,
            rolling_average: 0.0,
            target: 0.0,
            reserve: 0.0,
            average_cost: 0.0,
            used_capital: 0.0,
        }
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
            firm.property.insert(10, FirmPRow {
                quantity: 10.0,
                rolling_average: 0.0,
                target: 0.0,
                reserve: 0.0,
                average_cost: 0.0,
                used_capital: 0.0,
            });

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

            let report = firm.run_production(&factuals, &market);

            // Property should be updated
            assert_eq!(firm.property[&10].quantity, 0.0);
            assert_eq!(firm.property[&20].quantity, 5.0); // 5 iterations * 1.0

            // Report should show what was produced and consumed
            assert_eq!(report.produced.get(&20), Some(&5.0));
            assert_eq!(report.consumed.get(&10), Some(&10.0));
            assert!(report.effects.is_empty());

            // Line should have recorded success + AMV snapshots
            let line = &firm.production_line[0];
            assert_eq!(line.last_success_rate, 1.0);
            assert_eq!(line.last_iterations, 5.0);
            assert_eq!(line.last_amv_consumed, 50.0);
            assert_eq!(line.last_amv_produced, 60.0);
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

            let report = firm.run_production(&factuals, &market);

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

            assert_eq!(report.consumed.get(&30), Some(&6.0));
            assert_eq!(report.produced.get(&40), Some(&2.0));
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

            let report = firm.run_production(&factuals, &market);

            // Capital good should be recorded in used_capital, not in report.consumed
            assert_eq!(firm.property[&50].used_capital, 1.0);
            assert_eq!(firm.property[&50].quantity, 0.0);
            assert_eq!(firm.property[&10].quantity, 8.0);

            assert!(report.consumed.get(&50).is_none()); // capital should NOT appear in consumed
            assert_eq!(report.consumed.get(&10), Some(&2.0));
            assert_eq!(report.produced.get(&20), Some(&1.0));
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

            let report = firm.run_production(&factuals, &market);

            assert_eq!(report.effects.len(), 1);
            assert!(matches!(report.effects[0], ProcessEffect::Research(50.0)));

            // New good 99 should have been created in property
            assert!(firm.property.contains_key(&99));
            assert_eq!(firm.property[&99].quantity, 10.0);
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

            let report = firm.run_production(&factuals, &market);

            // Property assertions
            assert_eq!(firm.property[&100].quantity, 15.0);   // 20 - 5
            assert_eq!(firm.property[&110].quantity, 2.0);    // produced 5, consumed 3, 
            assert_eq!(firm.property[&200].used_capital, 8.0); // 5 + 3
            assert_eq!(firm.property[&200].quantity, 12.0);    // 20- 5 - 3
            // (adjust expected numbers based on exact per-iter costs you want)

            // Report aggregation across both lines
            assert_eq!(report.produced.get(&110), Some(&5.0)); // planks created
            assert_eq!(report.produced.get(&120), Some(&3.0)); // tables created
            assert_eq!(report.consumed.get(&100), Some(&5.0)); // wood
            assert_eq!(report.consumed.get(&110), Some(&3.0));  // planks consumed in line 2
            assert!(report.consumed.get(&200).is_none());       // capital never in consumed

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

            let report = firm.run_production(&factuals, &market);

            // Should run (required factor present) but without the optional throughput bonus
            assert!(firm.production_line[0].last_success_rate > 0.9);
            assert_eq!(firm.production_line[0].last_iterations, 20.0);
            assert_eq!(firm.production_line[0].last_missing_goods.len(), 1);
            assert!(firm.production_line[0].last_missing_goods.contains(&100));
            assert_eq!(firm.production_line[0].last_amv_consumed, 160.0);
            assert_eq!(firm.production_line[0].last_amv_produced, 400.0);
            assert_eq!(report.consumed.get(&100), Some(&20.0)); // 10 iterations * 2.0
            assert_eq!(report.consumed.get(&110), Some(&20.0)); // 10 iterations * 2.0
            assert_eq!(report.produced.get(&120), Some(&20.0)); // 10 iterations * 2.0

            // test with optional factor included
            firm.property.insert(301, empty_firm_row(1.0));
            firm.property.get_mut(&100).unwrap().quantity += 20.0;
            firm.property.get_mut(&110).unwrap().quantity += 20.0;
            firm.production_line[0].last_amv_consumed = 0.0;
            firm.production_line[0].last_amv_produced = 0.0;
            firm.production_line[0].last_iterations = 0.0;
            firm.production_line[0].last_success_rate = 0.0;

            let report = firm.run_production(&factuals, &market);

            // Should run (required factor present) but without the optional throughput bonus
            assert!(firm.production_line[0].last_success_rate > 0.9);
            assert_eq!(firm.production_line[0].last_iterations, 20.0);
            assert_eq!(firm.production_line[0].last_missing_goods.len(), 1);
            assert!(firm.production_line[0].last_missing_goods.contains(&100));
            assert_eq!(firm.production_line[0].last_amv_consumed, 220.0);
            assert_eq!(firm.production_line[0].last_amv_produced, 600.0);
            assert_eq!(report.consumed.get(&100), Some(&20.0)); // 10 iterations * 2.0
            assert_eq!(report.consumed.get(&110), Some(&30.0)); // 10 iterations * 2.0
            assert_eq!(report.produced.get(&120), Some(&30.0)); // 10 iterations * 2.0
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

            let report = firm.run_production(&factuals, &market);

            // With catalyst bonus we should get more than the base 5 iterations worth of output
            assert_eq!(firm.production_line[0].last_iterations, 10.0);
            assert_eq!(firm.production_line[0].last_amv_consumed, 50.0);
            assert_eq!(firm.production_line[0].last_amv_produced, 75.25);
            assert_eq!(report.consumed[&100], 10.0);
            assert_eq!(report.consumed[&400], 3.0);
            assert_eq!(report.produced[&110], 10.75);
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

            let report = firm.run_production(&factuals, &market);

            assert_eq!(report.produced.get(&110), Some(&8.0));  // main output
            assert_eq!(report.produced.get(&130), Some(&4.0));  // decay result (8 iters * 0.5)
            assert_eq!(report.consumed.get(&100), Some(&8.0)); 
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

            let report = firm.run_production(&factuals, &market);

            assert_eq!(report.produced.len(), 2);
            assert_eq!(report.consumed.len(), 3);
            assert_eq!(report.produced.get(&120), Some(&8.0));  // main output
            assert_eq!(report.produced.get(&130), Some(&13.0));  // decay result (8 iters * 0.5)
            assert_eq!(report.consumed.get(&100), Some(&8.0)); 
            assert_eq!(report.consumed.get(&110), Some(&13.0)); 
            assert_eq!(report.consumed.get(&500), Some(&5.0)); 
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

            let _report = firm.run_production(&factuals, &market);

            // Should fall back to the economic default of 1.0
            assert_eq!(
                firm.production_line[0].last_amv_consumed, 5.0,
                "Missing goods should default to AMV 1.0"
            );
        }
    }
}
