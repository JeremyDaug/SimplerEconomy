use std::collections::{HashMap, HashSet};

use crate::game::config::labor_constants;
use crate::game::desire::DesireTargetType;
use crate::game::good::TIME;
use crate::game::market::MarketHistory;
use crate::game::pop::Pop;
use crate::game::util::{round_units, whole_units, whole_units_up};

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
    /// Last settle exchange, pop inventory change. Negative left the pop
    /// (Time given, recap). Positive entered the pop (wages, remainder,
    /// profit share). Overwritten each settle.
    pub last_exchange: HashMap<usize, f64>,
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
            last_exchange: HashMap::new(),
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

    /// AMV of the wage basket at `hours` (whole-unit quantities).
    pub fn promised_amv(&self, hours: f64, history: &MarketHistory) -> f64 {
        self.payment
            .iter()
            .map(|term| term.promised_qty(hours) * history.price(term.good))
            .sum()
    }

    /// Wage AMV per claimed Time unit (0 if hours are 0).
    pub fn hourly_wage_amv(&self, history: &MarketHistory) -> f64 {
        let hours = self.hours.max(0.0);
        if hours <= 0.0 {
            0.0
        } else {
            self.promised_amv(hours, history) / hours
        }
    }

    /// Scales every payment term amount by `factor`.
    /// Factor must be `>= 0.0`.
    pub fn scale_payments(&mut self, factor: f64) {
        debug_assert!(factor >= 0.0, "payment scale factor must be >= 0.0");
        let factor = factor.max(0.0);
        for term in &mut self.payment {
            term.amount = (term.amount * factor).max(0.0);
        }
    }

    /// Rounds hours and payment amounts to nearest whole units (half-up).
    /// Scaling wage amounts stay at least [`labor_constants::WAGE_AMOUNT_MIN`].
    fn round_hours_and_wages(&mut self) {
        self.hours = round_units(self.hours.max(0.0));
        for term in &mut self.payment {
            term.amount = round_units(term.amount.max(0.0));
            if !term.flat && term.amount > 0.0 {
                term.amount = term.amount.max(labor_constants::WAGE_AMOUNT_MIN);
            }
        }
    }

    /// Turns a fat flat into hourly when `flat / hours` is at least 1 whole unit.
    /// Leftover stays flat (the fractional sneak).
    fn fold_flat_into_hourly(&mut self) {
        if self.hours < 1.0 {
            return;
        }
        let hours = self.hours;
        let mut i = 0;
        while i < self.payment.len() {
            if !self.payment[i].flat {
                i += 1;
                continue;
            }
            let good = self.payment[i].good;
            let amount = self.payment[i].amount;
            let hourly = whole_units(amount / hours);
            if hourly < labor_constants::WAGE_AMOUNT_MIN {
                i += 1;
                continue;
            }
            let take = hourly * hours;
            self.payment[i].amount = (amount - take).max(0.0);
            if let Some(scaling) = self
                .payment
                .iter_mut()
                .find(|term| !term.flat && term.good == good)
            {
                scaling.amount = (scaling.amount + hourly).max(labor_constants::WAGE_AMOUNT_MIN);
            } else {
                self.payment.push(PaymentTerm::new(
                    good,
                    hourly.max(labor_constants::WAGE_AMOUNT_MIN),
                ));
            }
            if self.payment[i].amount < 1.0 {
                self.payment.remove(i);
            } else {
                i += 1;
            }
        }
    }

    /// Drops 1 unit from each flat term (bonuses first when the firm is shrinking).
    fn trim_flats(&mut self) {
        self.payment.retain_mut(|term| {
            if !term.flat {
                return true;
            }
            term.amount = (term.amount - 1.0).max(0.0);
            term.amount >= 1.0
        });
    }

    /// Adds 1 whole unit as a flat bonus for `good` (does not jack the hourly).
    fn add_flat_bonus(&mut self, good: usize) {
        if good == TIME {
            return;
        }
        if let Some(flat) = self
            .payment
            .iter_mut()
            .find(|term| term.flat && term.good == good)
        {
            flat.amount += 1.0;
            return;
        }
        self.payment.push(PaymentTerm::flat(good, 1.0));
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

fn push_cover_good(order: &mut Vec<usize>, seen: &mut HashSet<usize>, good: usize) {
    if good != TIME && seen.insert(good) {
        order.push(good);
    }
}

/// True when a consume desire for this good is still unsatisfied.
fn pop_wants_kind(pop: &Pop, good: usize) -> bool {
    pop.desires.iter().flatten().any(|desire| {
        let targets = desire.target.iter().any(|target| {
            target.good == good && matches!(target.desire_type, DesireTargetType::Consume)
        });
        targets && desire.satisfaction < desire.amount
    })
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
    /// True when this payout was the residual leftover claim.
    pub remainder: bool,
    pub profit_share_amv: f64,
    pub paid_amv: f64,
    pub paid: HashMap<usize, f64>,
    /// AMV shortfall the remainder owner was asked to cover.
    pub recap_amv: f64,
    /// AMV of goods the owner actually transferred.
    pub recap_paid_amv: f64,
    /// Goods moved owner -> firm to cover the shortfall.
    pub recap: HashMap<usize, f64>,
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
