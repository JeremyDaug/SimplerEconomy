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

/// One good in a wage basket.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaymentTerm {
    pub good: usize,
    /// Units owed: per time unit when `flat` is false, else the whole lump.
    pub amount: f64,
    /// True: a lump. False: scales with hours.
    pub flat: bool,
}

/// How a workforce row is paid. Placeholder kinds.
#[derive(Debug, Clone)]
pub enum WorkforceContractType {
    /// Paid per time unit.
    Wage,
    /// Paid a percent of profit AMV. The value is that percent.
    Owner(f64),
}

impl Workforce {
    /// # Empty
    ///
    /// A wage row with id 0, no workers, no hours, and no pay.
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

    /// # New
    ///
    /// [`Self::empty`] with this pop id.
    pub fn new(id: usize) -> Self {
        let mut worker = Self::empty();
        worker.id = id;
        worker
    }

    /// # With Id
    ///
    /// Sets the pop id and returns this row.
    pub fn with_id(mut self, id: usize) -> Self {
        self.id = id;
        self
    }

    /// # With Contract Type
    ///
    /// Sets how this row is paid and returns it.
    pub fn with_contract_type(mut self, contract_type: WorkforceContractType) -> Self {
        self.contract_type = contract_type;
        self
    }

    /// # With Workers
    ///
    /// Sets the minimum and maximum worker counts. Both ends must be `>= 0.0`.
    pub fn with_workers(mut self, min: f64, max: f64) -> Self {
        debug_assert!(min >= 0.0, "workers min must be >= 0.0");
        debug_assert!(max >= 0.0, "workers max must be >= 0.0");
        self.workers = (min, max);
        self
    }

    /// # With Hours
    ///
    /// Time units claimed from the pop. Must be `>= 0.0`.
    pub fn with_hours(mut self, hours: f64) -> Self {
        debug_assert!(hours >= 0.0, "hours must be >= 0.0");
        self.hours = hours;
        self
    }

    /// # With Labor
    ///
    /// Records expected labor `amount` of `good` per time unit. Amount must be `>= 0.0`.
    pub fn with_labor(mut self, good: usize, amount: f64) -> Self {
        debug_assert!(amount >= 0.0, "labor amount must be >= 0.0");
        self.labor.insert(good, amount);
        self
    }

    /// # With Payment
    ///
    /// Appends one wage-basket term and returns this row.
    pub fn with_payment(mut self, term: PaymentTerm) -> Self {
        self.payment.push(term);
        self
    }

    /// # With Profit Share
    ///
    /// Sets the profit share reserved for this row. Must be in `0.0..=1.0`.
    pub fn with_profit_share(mut self, profit_share: f64) -> Self {
        debug_assert!(
            (0.0..=1.0).contains(&profit_share),
            "profit_share must be in 0.0..=1.0"
        );
        self.profit_share = profit_share;
        self
    }

    /// # Ordered Payment Terms
    ///
    /// Scaling terms first, then flat terms, then good id.
    pub fn ordered_payment_terms(&self) -> Vec<PaymentTerm> {
        let mut terms = self.payment.clone();
        terms.sort_by(|a, b| a.flat.cmp(&b.flat).then(a.good.cmp(&b.good)));
        terms
    }
}

impl PaymentTerm {
    /// # New
    ///
    /// A per-hour term. `amount` must be `>= 0.0`.
    pub fn new(good: usize, amount: f64) -> Self {
        debug_assert!(amount >= 0.0, "payment amount must be >= 0.0");
        Self {
            good,
            amount,
            flat: false,
        }
    }

    /// # Flat
    ///
    /// A lump term that does not scale with hours. `amount` must be `>= 0.0`.
    pub fn flat(good: usize, amount: f64) -> Self {
        debug_assert!(amount >= 0.0, "payment amount must be >= 0.0");
        Self {
            good,
            amount,
            flat: true,
        }
    }

    /// # Quantity For
    ///
    /// Raw units owed at `hours` (not rounded).
    ///
    /// A non-positive amount owes nothing. A flat term owes its amount.
    /// Any other term owes `amount * hours`, and hours below 0 count as 0.
    pub fn quantity_for(&self, hours: f64) -> f64 {
        if self.amount <= 0.0 {
            0.0
        } else if self.flat {
            self.amount
        } else {
            self.amount * hours.max(0.0)
        }
    }

    /// # Promised Qty
    ///
    /// Whole-unit quantity owed at `hours`.
    ///
    /// The raw amount is [`Self::quantity_for`]. A non-positive total is 0.
    /// A positive total rounds away from zero.
    pub fn promised_qty(&self, hours: f64) -> f64 {
        let raw = self.quantity_for(hours);
        if raw <= 0.0 { 0.0 } else { whole_units_up(raw) }
    }
}
