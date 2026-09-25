use std::collections::HashMap;

use crate::game::util::whole_units_up;

/// One pop working for a firm: hours claimed and the pay basket.
///
/// Morning settlement (paying the basket, moving Time, rewriting AMV) is not here.
#[derive(Debug, Clone)]
pub struct Workforce {
    /// Pop id. `0` is none.
    pub id: usize,
    pub contract_type: WorkforceContractType,
    /// `(minimum workers, maximum workers)`.
    pub workers: (f64, f64),
    /// Time units claimed from the pop.
    pub hours: f64,
    /// Expected labor units per time unit, keyed by skill or good id.
    pub labor: HashMap<usize, f64>,
    /// Wage basket.
    pub payment: Vec<PaymentTerm>,
    /// Share of profit reserved for this row. `0..=1`. Not paid out here.
    pub profit_share: f64,
    /// Last exchange with the pop, when a settle exists again.
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

    pub fn new(id: usize) -> Self {
        let mut worker = Self::empty();
        worker.id = id;
        worker
    }

    pub fn with_id(mut self, id: usize) -> Self {
        self.id = id;
        self
    }

    pub fn with_contract_type(mut self, contract_type: WorkforceContractType) -> Self {
        self.contract_type = contract_type;
        self
    }

    /// Both ends must be `>= 0.0`.
    pub fn with_workers(mut self, min: f64, max: f64) -> Self {
        debug_assert!(min >= 0.0, "workers min must be >= 0.0");
        debug_assert!(max >= 0.0, "workers max must be >= 0.0");
        self.workers = (min, max);
        self
    }

    /// Time units claimed from the pop. Must be `>= 0.0`.
    pub fn with_hours(mut self, hours: f64) -> Self {
        debug_assert!(hours >= 0.0, "hours must be >= 0.0");
        self.hours = hours;
        self
    }

    pub fn with_labor(mut self, good: usize, amount: f64) -> Self {
        debug_assert!(amount >= 0.0, "labor amount must be >= 0.0");
        self.labor.insert(good, amount);
        self
    }

    pub fn with_payment(mut self, term: PaymentTerm) -> Self {
        self.payment.push(term);
        self
    }

    pub fn with_profit_share(mut self, profit_share: f64) -> Self {
        debug_assert!(
            (0.0..=1.0).contains(&profit_share),
            "profit_share must be in 0.0..=1.0"
        );
        self.profit_share = profit_share;
        self
    }

    /// Scaling terms first, then flat terms, then good id.
    pub fn ordered_payment_terms(&self) -> Vec<PaymentTerm> {
        let mut terms = self.payment.clone();
        terms.sort_by(|a, b| a.flat.cmp(&b.flat).then(a.good.cmp(&b.good)));
        terms
    }
}

/// One good in a wage basket.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaymentTerm {
    pub good: usize,
    /// Units owed: per time unit when `flat` is false, else the whole lump.
    pub amount: f64,
    /// True: a lump. False: scales with hours.
    pub flat: bool,
}

impl PaymentTerm {
    pub fn new(good: usize, amount: f64) -> Self {
        debug_assert!(amount >= 0.0, "payment amount must be >= 0.0");
        Self {
            good,
            amount,
            flat: false,
        }
    }

    pub fn flat(good: usize, amount: f64) -> Self {
        debug_assert!(amount >= 0.0, "payment amount must be >= 0.0");
        Self {
            good,
            amount,
            flat: true,
        }
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

    /// Whole-unit quantity owed at `hours`.
    pub fn promised_qty(&self, hours: f64) -> f64 {
        let raw = self.quantity_for(hours);
        if raw <= 0.0 {
            0.0
        } else {
            whole_units_up(raw)
        }
    }
}

/// How a workforce row is paid. Placeholder kinds.
#[derive(Debug, Clone)]
pub enum WorkforceContractType {
    /// Paid per time unit.
    Wage,
    /// Paid a percent of profit AMV. The value is that percent.
    Owner(f64),
}

#[cfg(test)]
mod workforce_should {
    use super::*;

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
