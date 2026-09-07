use std::collections::{HashMap, HashSet};

use crate::game::config::GameConfig;
use crate::game::firm::{Firm, FirmPRow};

use crate::game::market::MarketHistory;
use crate::game::pop::Pop;
use crate::game::util::{whole_units, whole_units_up};

/// # Workforce
/// 
/// Storage for the workforce of a Firm, which pops at which wage and for how long each 
/// market day. 
/// 
/// Firms should encourage workforce pops to unify when possible to simplify things.
/// 
/// Firm does not actually care about size of a pop, but the pop will respond each day
/// with how much over/under work it is performing and giving the firm first dibs to
/// absorb the extra labor time.
/// 
/// If a firm doesn't directly manage the time balance of employees, instead letting 
/// them grow or shrink as wages and work hours demand. A tad, unrealistic perhaps,
/// but good enough until more complex labor contracts and rules can be added.
/// 
/// This will probably change when different kinds of wages and controls come in.
/// For now, this is a pure 'hourly wage' system, not a salary or contract.
/// 
/// ## Notes
/// 
/// For future purposes, Time Wage is time delimited, buying a specific amount of time
/// and letting workers manage their own size and population. Salary is worker limited,
/// defining how many people the workplace will hire, and dealing with hours second.
/// Salary gives more control to the firm over the population, but in return for more 
/// consistent wages per pop. Salaried has a soft cap on work hours.
/// 
/// Slavery operates as a special case of contract, giving a specific basket of goods in
/// return for work, but with still no control over time worked or workers included.
///
/// Employment is this roster, not a market order and not [`crate::game::contract::Contract`].
/// Morning settlement pays the basket, then moves committed Time to the firm.
/// One pop, one employer. A firm may list several worker pops.
#[derive(Debug, Clone)]
pub struct Workforce {
    /// The Id of the pop this connects to.
    pub id: usize,
    /// What kind of contract the workforce is under.
    pub contract_type: WorkforceContractType,
    /// The number of workers. Lower number is the minimum number of workers,
    /// upper is the maximum. 
    /// 
    /// The upper is the cap on the pop's size. Excess will be pushed out.
    /// 
    /// Pops below the minimum will cause recaluclation as the firm seeks to hire up.
    /// Minimum can also be used to calculate the 'overwork' cap of the firm.
    pub workers: (f64, f64),
    /// The hours (multiplier) applied to labor and possibly payment as well, if the
    /// worker is in the right contract type.
    /// Hours are time units claimed from the pop (not clock hours).
    pub hours: f64,
    /// The 'work unit' from the pop on. This is effectively the 'hourly work' done.
    /// If wage labor, this is multiplied by size, for the number of hours purchased 
    /// from the workers. For salary, this is the measure 
    pub labor: HashMap<usize, f64>,
    /// The payment for their work. This is either 'salaried' meaning it's everything,
    /// or 'waged' meaning it's per multiple of the expected labor.
    /// Scaling terms are per time unit; flat terms are a lump for the shift.
    pub payment: Vec<PaymentTerm>,
    /// Share of yesterday's profit AMV paid after wages and growth retain. 0..=1.
    pub profit_share: f64,
}

impl Workforce {
    pub fn empty() -> Self {
        Self {
            id: 0,
            contract_type: WorkforceContractType::Wage,
            workers: (0.0, 0.0),
            hours: 0.0,
            labor: HashMap::new(),
            payment: vec![],
            profit_share: 0.0,
        }
    }

    /// Roster row for this pop, wage contract, no hours or pay.
    pub fn new(id: usize) -> Self {
        let mut worker = Self::empty();
        worker.id = id;
        worker
    }

    /// Sets the pop id. `0` is none / skipped at settle.
    pub fn with_id(mut self, id: usize) -> Self {
        self.id = id;
        self
    }

    /// Sets the contract kind.
    pub fn with_contract_type(mut self, contract_type: WorkforceContractType) -> Self {
        self.contract_type = contract_type;
        self
    }

    /// Sets (min, max) worker counts.
    /// Both ends must be `>= 0.0`.
    pub fn with_workers(mut self, min: f64, max: f64) -> Self {
        debug_assert!(min >= 0.0, "workers min must be >= 0.0");
        debug_assert!(max >= 0.0, "workers max must be >= 0.0");
        self.workers = (min, max);
        self
    }

    /// Sets time units claimed from the pop.
    /// Must be `>= 0.0`.
    pub fn with_hours(mut self, hours: f64) -> Self {
        debug_assert!(hours >= 0.0, "hours must be >= 0.0");
        self.hours = hours;
        self
    }

    /// Sets expected skill/labor units per time unit for this good.
    /// Amount must be `>= 0.0`.
    pub fn with_labor(mut self, good: usize, amount: f64) -> Self {
        debug_assert!(amount >= 0.0, "labor amount must be >= 0.0");
        self.labor.insert(good, amount);
        self
    }

    /// Pushes one payment term (scaling or flat).
    pub fn with_payment(mut self, term: PaymentTerm) -> Self {
        self.payment.push(term);
        self
    }

    /// Sets the profit-share fraction of yesterday's profit AMV.
    /// Must be in 0..=1.
    pub fn with_profit_share(mut self, profit_share: f64) -> Self {
        debug_assert!(
            (0.0..=1.0).contains(&profit_share),
            "profit_share must be in 0.0..=1.0"
        );
        self.profit_share = profit_share;
        self
    }
}

/// # Payment Term
///
/// One good in a wage basket. Goods first; AMV is only a helper at settle.
///
/// Scaling terms (`flat == false`) are per time unit and are paid first (make-up).
/// Flat terms are a lump for the shift and are paid last.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaymentTerm {
    pub good: usize,
    /// Units owed: per time unit when `flat` is false, else the whole lump.
    pub amount: f64,
    /// True: lump (a loaf). False: scales with hours.
    pub flat: bool,
}

impl PaymentTerm {
    /// Scaling (per time unit) term for this good.
    /// Amount must be `>= 0.0`.
    pub fn new(good: usize, amount: f64) -> Self {
        debug_assert!(amount >= 0.0, "payment amount must be >= 0.0");
        Self {
            good,
            amount,
            flat: false,
        }
    }

    /// Flat lump term for this good.
    /// Amount must be `>= 0.0`.
    pub fn flat(good: usize, amount: f64) -> Self {
        debug_assert!(amount >= 0.0, "payment amount must be >= 0.0");
        Self {
            good,
            amount,
            flat: true,
        }
    }

    /// Marks this term flat or scaling.
    pub fn with_flat(mut self, flat: bool) -> Self {
        self.flat = flat;
        self
    }

    /// Raw units owed at `hours` (not rounded).
    pub fn quantity_for(&self, hours: f64) -> f64 {
        if self.amount <= 0.0 {
            0.0
        } else if self.flat {
            self.amount
        } else {
            self.amount * hours.max(0.0)
        }
    }

    /// Whole-unit quantity owed at `hours` time units.
    pub fn promised_qty(&self, hours: f64) -> f64 {
        let raw = self.quantity_for(hours);
        if raw <= 0.0 {
            0.0
        } else {
            whole_units_up(raw)
        }
    }
}

impl Workforce {
    /// Caps claimed hours by on-hand Time and the work-time fraction cap.
    /// `work_fraction` is a cap (later: culture / class / religion / law).
    pub fn time_claim(&self, on_hand_time: f64, work_fraction: f64) -> f64 {
        if self.hours <= 0.0 || on_hand_time <= 0.0 {
            return 0.0;
        }
        let offered = work_fraction.clamp(0.0, 1.0) * on_hand_time;
        self.hours.min(offered).min(on_hand_time).max(0.0)
    }

    /// Payment terms in settle order: scaling (make-up) first, then flat, then good id.
    pub fn ordered_payment_terms(&self) -> Vec<PaymentTerm> {
        let mut terms = self.payment.clone();
        terms.sort_by(|a, b| a.flat.cmp(&b.flat).then(a.good.cmp(&b.good)));
        terms
    }

    /// Pays this row's wage basket from wage-spendable stock. Whole units.
    /// Returns `(promised_amv, paid_amv, paid goods)`.
    pub fn pay_wages(
        &self,
        firm: &mut Firm,
        pop: &mut Pop,
        hours: f64,
        history: &MarketHistory,
    ) -> (f64, f64, HashMap<usize, f64>) {
        let terms = self.ordered_payment_terms();
        let mut promised_amv = 0.0;
        for term in &terms {
            promised_amv += term.promised_qty(hours) * history.price(term.good);
        }
        let mut paid_amv = 0.0;
        let mut paid: HashMap<usize, f64> = HashMap::new();
        for term in &terms {
            let want = term.promised_qty(hours);
            if want <= 0.0 {
                continue;
            }
            let spendable = firm
                .property
                .get(&term.good)
                .map(FirmPRow::wage_spendable)
                .unwrap_or(0.0);
            let give = whole_units(want.min(spendable));
            if give <= 0.0 {
                continue;
            }
            firm.debit_good(term.good, give);
            let unit = history.price(term.good);
            pop.credit_good(term.good, give, unit);
            *paid.entry(term.good).or_insert(0.0) += give;
            paid_amv += give * unit;
        }
        (promised_amv, paid_amv, paid)
    }
}

/// # Workforce Contract Type
/// 
/// Defines how workers are paid.
/// 
/// Currently mostly placeholder.
#[derive(Debug, Clone)]
pub enum WorkforceContractType {
    /// Hourly wage, Pop is paid for unit
    Wage,
    /// Paid in profits, the value attached being the percent of AMV profits they take
    /// daily.
    Owner(f64),
}

/// Result of [`LaborSettlement::settle`].
#[derive(Debug, Clone)]
pub struct LaborSettlement {
    pub owner: Option<LaborOwnerReport>,
    pub workers: Vec<LaborWorkerReport>,
}

impl LaborSettlement {
    fn empty() -> Self {
        Self {
            owner: None,
            workers: vec![],
        }
    }

    /// # Settle
    ///
    /// Morning employment settlement for a firm's roster. Does not hire, fire,
    /// or post market orders. Not wired into the tester CLI or PlayState yet.
    ///
    /// 1. For each living workforce pop, cap hours by on-hand Time and
    ///    `work_time_fraction`, pay the wage basket (scaling terms first, then
    ///    flat; whole units) from stock above the stock fence, and move Time
    ///    to the firm in proportion to AMV paid / AMV promised.
    /// 2. Stock fence is `max(stock_target, reserve_target)` and is never spent.
    ///    Wages may raid `growth_target`. Profit shares cannot.
    /// 3. After wages, pay worker then owner profit shares of yesterday's
    ///    `sold_amv - sold_cost_amv` from goods above stock and growth, highest
    ///    salability first, skipping Time.
    ///
    /// Partial pay withholds Time linearly in AMV. Missing pops are skipped.
    pub fn settle(
        firm: &mut Firm,
        pops: &mut HashMap<usize, Pop>,
        history: &MarketHistory,
        config: &GameConfig,
    ) -> Self {
        let work_fraction = config.labor.work_time_fraction;
        let mut report = Self::empty();
        let mut worker_ids: Vec<usize> = firm
            .workforce
            .iter()
            .map(|w| w.id)
            .filter(|id| *id != 0)
            .collect();
        worker_ids.sort_unstable();
        worker_ids.dedup();

        for pop_id in worker_ids {
            let Some(idx) = firm.workforce.iter().position(|w| w.id == pop_id) else {
                continue;
            };
            if !pops.contains_key(&pop_id) {
                continue;
            }
            let worker = firm.workforce[idx].clone();
            let pop = pops.get_mut(&pop_id).expect("pop checked");
            let claim = worker.time_claim(pop.on_hand_time(), work_fraction);
            let mut row_report = LaborWorkerReport::new(pop_id, claim);

            if claim > 0.0 {
                let (promised_amv, paid_amv, paid) =
                    worker.pay_wages(firm, pop, claim, history);
                row_report.promised_amv = promised_amv;
                row_report.paid_amv = paid_amv;
                row_report.paid = paid;
                let fill = if promised_amv <= 0.0 {
                    1.0
                } else {
                    (paid_amv / promised_amv).clamp(0.0, 1.0)
                };
                let given = pop.take_time(claim * fill);
                row_report.time_given = given;
                firm.credit_time(given);
            }

            report.workers.push(row_report);
        }

        let profit_amv = firm.records.yesterday_profit_amv();
        if profit_amv > 0.0 {
            let shares: Vec<(usize, f64)> = firm
                .workforce
                .iter()
                .filter(|w| w.id != 0 && w.profit_share > 0.0 && pops.contains_key(&w.id))
                .map(|w| (w.id, w.profit_share))
                .collect();
            let mut seen = HashSet::new();
            for (pop_id, share) in shares {
                if !seen.insert(pop_id) {
                    continue;
                }
                let want = profit_amv * share;
                let pop = pops.get_mut(&pop_id).expect("pop checked");
                let (paid_amv, paid) = firm.pay_profit_share_amv(pop, want, history);
                if let Some(row) = report.workers.iter_mut().find(|r| r.pop == pop_id) {
                    row.profit_share_amv = want;
                    row.profit_paid_amv = paid_amv;
                    for (good, qty) in paid {
                        *row.paid.entry(good).or_insert(0.0) += qty;
                    }
                }
            }

            if let Some(owner_id) = firm.owners.pop_id() {
                if firm.owners.profit_share > 0.0 {
                    if let Some(pop) = pops.get_mut(&owner_id) {
                        let want = profit_amv * firm.owners.profit_share;
                        let (paid_amv, paid) = firm.pay_profit_share_amv(pop, want, history);
                        report.owner = Some(LaborOwnerReport {
                            pop: owner_id,
                            profit_share_amv: want,
                            paid_amv,
                            paid,
                        });
                    }
                }
            }
        }

        report
    }
}

/// One workforce pop's wage and Time transfer for the morning settle.
#[derive(Debug, Clone)]
pub struct LaborWorkerReport {
    pub pop: usize,
    pub time_claimed: f64,
    pub time_given: f64,
    pub promised_amv: f64,
    pub paid_amv: f64,
    pub profit_share_amv: f64,
    pub profit_paid_amv: f64,
    pub paid: HashMap<usize, f64>,
}

impl LaborWorkerReport {
    fn new(pop: usize, time_claimed: f64) -> Self {
        Self {
            pop,
            time_claimed,
            time_given: 0.0,
            promised_amv: 0.0,
            paid_amv: 0.0,
            profit_share_amv: 0.0,
            profit_paid_amv: 0.0,
            paid: HashMap::new(),
        }
    }
}

/// Owner profit-share payout from leftover till.
#[derive(Debug, Clone)]
pub struct LaborOwnerReport {
    pub pop: usize,
    pub profit_share_amv: f64,
    pub paid_amv: f64,
    pub paid: HashMap<usize, f64>,
}

#[cfg(test)]
mod workforce_should {
    use super::*;

    #[test]
    fn time_claim_caps_hours_by_work_fraction_and_on_hand() {
        let worker = Workforce::new(2).with_hours(40.0);
        assert!((worker.time_claim(48.0, 0.5) - 24.0).abs() < 1e-12);
        assert!((worker.time_claim(48.0, 1.0) - 40.0).abs() < 1e-12);
        assert!((worker.time_claim(10.0, 1.0) - 10.0).abs() < 1e-12);
        assert_eq!(worker.time_claim(48.0, 0.0), 0.0);
        assert_eq!(Workforce::new(2).time_claim(48.0, 0.5), 0.0);
    }

    #[test]
    fn quantity_for_scales_unless_flat() {
        let coin = PaymentTerm::new(5, 2.0);
        let loaf = PaymentTerm::flat(3, 1.0);
        assert!((coin.quantity_for(4.0) - 8.0).abs() < 1e-12);
        assert!((loaf.quantity_for(4.0) - 1.0).abs() < 1e-12);
        assert_eq!(coin.promised_qty(4.1), 9.0);
        assert_eq!(loaf.promised_qty(4.1), 1.0);
    }

    #[test]
    fn ordered_payment_puts_scaling_before_flat() {
        let worker = Workforce::new(2)
            .with_payment(PaymentTerm::flat(3, 1.0))
            .with_payment(PaymentTerm::new(5, 1.0))
            .with_payment(PaymentTerm::new(3, 0.5));
        let ordered = worker.ordered_payment_terms();
        assert_eq!(ordered[0].good, 3);
        assert!(!ordered[0].flat);
        assert_eq!(ordered[1].good, 5);
        assert!(!ordered[1].flat);
        assert_eq!(ordered[2].good, 3);
        assert!(ordered[2].flat);
    }
}

#[cfg(test)]
mod settle_labor_contracts_should {
    use super::*;
    use crate::game::actor::Actor;
    use crate::game::config::GameConfig;
    use crate::game::firm::FirmPRow;
    use crate::game::good::TIME;
    use crate::game::household::Household;
    use crate::game::pop::{DemoRow, Pop, PopPRow, PopRecords};
    use crate::game::sentiment::Sentiment;

    const COIN: usize = 5;
    const BREAD: usize = 3;

    fn make_pop(id: usize) -> Pop {
        Pop {
            id,
            job: 0,
            property: HashMap::new(),
            desires: vec![vec![]; 3],
            working_desires: vec![],
            demographics: DemoRow {
                household: Household::with_count(1.0),
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

    fn with_time(mut pop: Pop, qty: f64) -> Pop {
        pop.property.insert(TIME, PopPRow::new(qty));
        pop
    }

    fn history_prices(pairs: &[(usize, f64)]) -> MarketHistory {
        let mut history = MarketHistory::new();
        for &(good, amv) in pairs {
            history.prices.insert(good, amv);
            history.salability.insert(good, 0.9);
        }
        history
    }

    #[test]
    fn pays_the_basket_and_moves_time_to_the_firm() {
        let worker = Workforce::new(2)
            .with_hours(10.0)
            .with_payment(PaymentTerm::new(COIN, 1.0));
        let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0))
            .with_workforce(worker);
        firm.property.insert(COIN, FirmPRow::new().with_quantity(20.0));
        let mut pops = HashMap::from([(2, with_time(make_pop(2), 48.0))]);
        let history = history_prices(&[(COIN, 1.0)]);

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &GameConfig::default());

        assert_eq!(report.workers.len(), 1);
        assert!((report.workers[0].time_claimed - 10.0).abs() < 1e-12);
        assert!((report.workers[0].time_given - 10.0).abs() < 1e-12);
        assert!((report.workers[0].paid_amv - 10.0).abs() < 1e-12);
        assert_eq!(pops[&2].property[&COIN].quantity, 10.0);
        assert!((pops[&2].property[&TIME].quantity - 38.0).abs() < 1e-12);
        assert_eq!(firm.property[&COIN].quantity, 10.0);
        assert!((firm.property[&TIME].quantity - 10.0).abs() < 1e-12);
        assert!((firm.property[&TIME].reserve - 10.0).abs() < 1e-12);
    }

    #[test]
    fn work_fraction_caps_hours_below_the_contract() {
        let worker = Workforce::new(2)
            .with_hours(48.0)
            .with_payment(PaymentTerm::new(COIN, 1.0));
        let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0))
            .with_workforce(worker);
        firm.property.insert(COIN, FirmPRow::new().with_quantity(100.0));
        let mut pops = HashMap::from([(2, with_time(make_pop(2), 48.0))]);
        let history = history_prices(&[(COIN, 1.0)]);

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &GameConfig::default());

        assert!((report.workers[0].time_claimed - 24.0).abs() < 1e-12);
        assert!((report.workers[0].time_given - 24.0).abs() < 1e-12);
        assert_eq!(pops[&2].property[&COIN].quantity, 24.0);
        assert!((pops[&2].property[&TIME].quantity - 24.0).abs() < 1e-12);
    }

    #[test]
    fn stock_fence_blocks_wages_and_withholds_time_by_amv() {
        let worker = Workforce::new(2)
            .with_hours(10.0)
            .with_payment(PaymentTerm::new(COIN, 1.0));
        let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0))
            .with_workforce(worker);
        firm.property.insert(
            COIN,
            FirmPRow::new()
                .with_quantity(10.0)
                .with_stock_target(8.0),
        );
        let mut pops = HashMap::from([(2, with_time(make_pop(2), 48.0))]);
        let history = history_prices(&[(COIN, 1.0)]);

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &GameConfig::default());

        assert_eq!(report.workers[0].paid[&COIN], 2.0);
        assert!((report.workers[0].paid_amv - 2.0).abs() < 1e-12);
        assert!((report.workers[0].time_given - 2.0).abs() < 1e-12);
        assert_eq!(firm.property[&COIN].quantity, 8.0);
    }

    #[test]
    fn wages_raid_growth_and_profit_share_does_not() {
        let worker = Workforce::new(2)
            .with_hours(3.0)
            .with_payment(PaymentTerm::new(COIN, 1.0));
        let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(Actor::Pop(3))
            .with_owner_profit_share(1.0)
            .with_workforce(worker);
        firm.property.insert(
            COIN,
            FirmPRow::new()
                .with_quantity(10.0)
                .with_stock_target(2.0)
                .with_growth_target(5.0),
        );
        firm.records.sold_amv = 100.0;
        firm.records.sold_cost_amv = 0.0;
        let mut pops = HashMap::from([
            (2, with_time(make_pop(2), 48.0)),
            (3, make_pop(3)),
        ]);
        let history = history_prices(&[(COIN, 1.0)]);

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &GameConfig::default());

        assert_eq!(pops[&2].property[&COIN].quantity, 3.0);
        let owner = report.owner.expect("owner paid");
        assert_eq!(owner.paid.get(&COIN).copied().unwrap_or(0.0), 0.0);
        assert_eq!(firm.property[&COIN].quantity, 7.0);
    }

    #[test]
    fn leftover_after_wages_and_growth_goes_to_the_owner() {
        let worker = Workforce::new(2)
            .with_hours(1.0)
            .with_payment(PaymentTerm::new(COIN, 1.0));
        let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(Actor::Pop(3))
            .with_owner_profit_share(1.0)
            .with_workforce(worker);
        firm.property.insert(
            COIN,
            FirmPRow::new()
                .with_quantity(10.0)
                .with_stock_target(2.0)
                .with_growth_target(5.0),
        );
        firm.records.sold_amv = 100.0;
        firm.records.sold_cost_amv = 0.0;
        let mut pops = HashMap::from([
            (2, with_time(make_pop(2), 48.0)),
            (3, make_pop(3)),
        ]);
        let history = history_prices(&[(COIN, 1.0)]);

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &GameConfig::default());

        assert_eq!(pops[&2].property[&COIN].quantity, 1.0);
        let owner = report.owner.expect("owner paid");
        assert_eq!(owner.paid[&COIN], 2.0);
        assert_eq!(firm.property[&COIN].quantity, 7.0);
    }

    #[test]
    fn scaling_terms_are_paid_before_flat_terms() {
        let worker = Workforce::new(2)
            .with_hours(5.0)
            .with_payment(PaymentTerm::flat(COIN, 5.0))
            .with_payment(PaymentTerm::new(COIN, 1.0));
        let mut firm = Firm::new(1, "bakery".into(), 1, hexx::Hex::new(0, 0))
            .with_workforce(worker);
        firm.property.insert(COIN, FirmPRow::new().with_quantity(6.0));
        let mut pops = HashMap::from([(2, with_time(make_pop(2), 48.0))]);
        let history = history_prices(&[(COIN, 1.0)]);

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &GameConfig::default());

        assert_eq!(report.workers[0].paid[&COIN], 6.0);
        assert!((report.workers[0].promised_amv - 10.0).abs() < 1e-12);
        assert!((report.workers[0].time_given - 3.0).abs() < 1e-12);
    }

    #[test]
    fn missing_pop_is_skipped() {
        let worker = Workforce::new(9)
            .with_hours(10.0)
            .with_payment(PaymentTerm::new(COIN, 1.0));
        let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0))
            .with_workforce(worker);
        firm.property.insert(COIN, FirmPRow::new().with_quantity(10.0));
        let mut pops = HashMap::new();
        let history = history_prices(&[(COIN, 1.0)]);

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &GameConfig::default());

        assert!(report.workers.is_empty());
        assert_eq!(firm.property[&COIN].quantity, 10.0);
    }

    #[test]
    fn bread_and_coin_basket_pays_both_when_stocked() {
        let worker = Workforce::new(2)
            .with_hours(4.0)
            .with_payment(PaymentTerm::flat(BREAD, 1.0))
            .with_payment(PaymentTerm::new(COIN, 1.0));
        let mut firm = Firm::new(1, "bakery".into(), 1, hexx::Hex::new(0, 0))
            .with_workforce(worker);
        firm.property.insert(COIN, FirmPRow::new().with_quantity(10.0));
        firm.property.insert(BREAD, FirmPRow::new().with_quantity(5.0));
        let mut pops = HashMap::from([(2, with_time(make_pop(2), 48.0))]);
        let history = history_prices(&[(COIN, 1.0), (BREAD, 2.0)]);

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &GameConfig::default());

        assert_eq!(pops[&2].property[&COIN].quantity, 4.0);
        assert_eq!(pops[&2].property[&BREAD].quantity, 1.0);
        assert!((report.workers[0].paid_amv - 6.0).abs() < 1e-12);
        assert!((report.workers[0].time_given - 4.0).abs() < 1e-12);
    }
}
