use std::collections::{HashMap, HashSet};

use crate::game::config::labor_constants;
use crate::game::desire::DesireTargetType;
use crate::game::factuals::Factuals;
use crate::game::firm::{Firm, FirmPRow};
use crate::game::good::TIME;
use crate::game::market::MarketHistory;
use crate::game::pop::Pop;
use crate::game::process::InputType;
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
    /// or post market orders.
    ///
    /// 1. Remainder owner on a loss (yesterday profit AMV <= 0) covers the
    ///    AMV shortfall between needs (recipe inputs, wage basket, stock
    ///    fence) and on-hand goods. Goods come from the owner's unreserved
    ///    stock, whole units, skipping Time: missing inputs, missing wage
    ///    goods, production outputs, then exchange goods. Contributed inputs
    ///    and outputs are fenced so leftover remainder cannot take them back
    ///    the same morning; wage and exchange goods stay spendable.
    /// 2. For each living workforce pop, cap hours by on-hand Time and
    ///    `work_time_fraction`, pay the wage basket (scaling terms first, then
    ///    flat; whole units) from stock above the stock fence, and move Time
    ///    to the firm in proportion to AMV paid / AMV promised.
    /// 3. Stock fence is `max(stock_target, reserve_target)` and is never spent.
    ///    Wages may raid `growth_target`. Profit shares cannot.
    /// 4. After wages, pay worker profit shares of yesterday's
    ///    `sold_amv - sold_cost_amv` from goods above stock, growth, and
    ///    posted sell, highest salability first, skipping Time.
    /// 5. Then the owner: remainder takes leftover till (even if profit is 0)
    ///    the same way; otherwise a limited `profit_share` of yesterday's
    ///    profit AMV. Posted sell is `min(sell_target, max market salability
    ///    * daily output)` for goods this firm makes. Leftover till is extra
    ///    above that, paid high-salability first.
    ///
    /// Partial pay withholds Time linearly in AMV. Missing pops are skipped.
    pub fn settle(
        firm: &mut Firm,
        pops: &mut HashMap<usize, Pop>,
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> Self {
        let work_fraction = factuals.config.labor.work_time_fraction;
        let mut report = Self::empty();
        let profit_amv = firm.records.yesterday_profit_amv();
        let mut recap_amv = 0.0;
        let mut recap_paid_amv = 0.0;
        let mut recap: HashMap<usize, f64> = HashMap::new();
        if firm.owners.remainder && profit_amv <= 0.0 {
            if let Some(owner_id) = firm.owners.pop_id() {
                if let Some(pop) = pops.get_mut(&owner_id) {
                    let (want, paid, goods) =
                        firm.cover_remainder_shortfall(pop, factuals, history);
                    recap_amv = want;
                    recap_paid_amv = paid;
                    recap = goods;
                }
            }
        }
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
                let (paid_amv, paid) = firm.pay_profit_share_amv(pop, want, history, factuals);
                if let Some(row) = report.workers.iter_mut().find(|r| r.pop == pop_id) {
                    row.profit_share_amv = want;
                    row.profit_paid_amv = paid_amv;
                    for (good, qty) in paid {
                        *row.paid.entry(good).or_insert(0.0) += qty;
                    }
                }
            }
        }

        if let Some(owner_id) = firm.owners.pop_id() {
            if let Some(pop) = pops.get_mut(&owner_id) {
                let remainder = firm.owners.remainder;
                let want = if remainder {
                    firm.leftover_profit_amv(history, factuals)
                } else if firm.owners.profit_share > 0.0 && profit_amv > 0.0 {
                    profit_amv * firm.owners.profit_share
                } else {
                    0.0
                };
                if remainder || want > 0.0 || recap_paid_amv > 0.0 {
                    let (paid_amv, paid) = if remainder || want > 0.0 {
                        firm.pay_profit_share_amv(pop, want, history, factuals)
                    } else {
                        (0.0, HashMap::new())
                    };
                    report.owner = Some(LaborOwnerReport {
                        pop: owner_id,
                        remainder,
                        profit_share_amv: want,
                        paid_amv,
                        paid,
                        recap_amv,
                        recap_paid_amv,
                        recap,
                    });
                }
            }
        }

        report
    }
}

impl Firm {
    /// # Budget Labor
    ///
    /// Rewrites standing workforce `hours` and payment amounts. Does not hire,
    /// fire, or move pops. No-op when `labor.budget_interval` is 0 or `day`
    /// is not a multiple of the interval (`day` is 1-based completed days).
    ///
    /// 1. Snap hours to recipe Time for current line targets, plus today's
    ///    `transport_spent + 1` (and one extra `transaction_cost` if they still
    ///    need to buy much more). Plan already slow-walks targets; hours are
    ///    not cut to fit the till.
    /// 2. Fold fat flats into hourly when `flat / hours >= 1`.
    /// 3. Wages: angry/fearful pops get a 1-unit flat bonus in a good they
    ///    consume or a salable good. Calm + unprofitable trims flats.
    ///    Calm + profit in 1.0..=1.15 holds. Calm + more profit ensures a
    ///    1-unit flat of product they still want as kind. Hourly rates never
    ///    go below 1. Short till is settle's problem.
    pub fn budget_labor(
        &mut self,
        factuals: &Factuals,
        history: &MarketHistory,
        pops: &HashMap<usize, Pop>,
        day: u32,
    ) {
        let interval = factuals.config.labor.budget_interval;
        if interval == 0 || day % interval != 0 {
            return;
        }
        let idxs: Vec<usize> = self
            .workforce
            .iter()
            .enumerate()
            .filter(|(_, worker)| worker.id != 0)
            .map(|(i, _)| i)
            .collect();
        if idxs.is_empty() {
            return;
        }

        let extra_shop = if self.needs_much_more_shopping() {
            factuals.config.market.transaction_cost.max(0.0)
        } else {
            0.0
        };
        let haul = self.transport_spent.max(0.0) + 1.0 + extra_shop;
        let hours_want_total = self.plan_time_need(factuals) + haul;
        let hour_sum: f64 = idxs
            .iter()
            .map(|&i| self.workforce[i].hours.max(0.0))
            .sum();
        let n = idxs.len() as f64;
        for &i in &idxs {
            let share = if hour_sum > 0.0 {
                self.workforce[i].hours.max(0.0) / hour_sum
            } else {
                1.0 / n
            };
            let want = hours_want_total * share;
            let mut hours = round_units(want.max(0.0));
            if factuals.config.firm.keep_alive && hours < 1.0 {
                hours = 1.0;
            }
            self.workforce[i].hours = hours;
        }

        for &i in &idxs {
            self.workforce[i].fold_flat_into_hourly();
            let Some(pop) = pops.get(&self.workforce[i].id) else {
                self.workforce[i].round_hours_and_wages();
                continue;
            };
            self.negotiate_wages(i, pop, factuals, history);
            self.workforce[i].fold_flat_into_hourly();
            self.workforce[i].round_hours_and_wages();
        }
    }

    /// One worker's wage basket: pop pressure vs firm product/miserliness.
    fn negotiate_wages(
        &mut self,
        idx: usize,
        pop: &Pop,
        factuals: &Factuals,
        history: &MarketHistory,
    ) {
        let pressure = (pop.sentiment.anger() + pop.sentiment.fear()).clamp(0.0, 1.0);
        let profit = self.records.profit_ratio;
        if pressure >= labor_constants::WAGE_PRESSURE_BAR {
            if let Some(good) = self.pick_pop_wage_good(pop, factuals, history) {
                self.workforce[idx].add_flat_bonus(good);
            }
        } else if profit < 1.0 {
            self.workforce[idx].trim_flats();
        } else if profit <= labor_constants::WAGE_HOLD_MAX {
            // Barely profitable: do not push product or raises.
        } else if let Some(good) = self.pick_firm_product(factuals) {
            if pop_wants_kind(pop, good) {
                let has_product = self.workforce[idx]
                    .payment
                    .iter()
                    .any(|term| term.good == good);
                if !has_product {
                    self.workforce[idx].add_flat_bonus(good);
                }
            }
        }
    }

    /// Pop pick: in-kind they still want, else a salable firm output, else
    /// the most salable good on the firm (not Time). Satiated consume desires
    /// skip in-kind so pay can move toward coin/salable.
    fn pick_pop_wage_good(
        &self,
        pop: &Pop,
        factuals: &Factuals,
        history: &MarketHistory,
    ) -> Option<usize> {
        let outputs = self.firm_output_goods(factuals);
        for &good in &outputs {
            if pop_wants_kind(pop, good) {
                return Some(good);
            }
        }
        let sal_min = factuals.config.market.exchange_salability_min;
        for &good in &outputs {
            if history.salability(good) >= sal_min {
                return Some(good);
            }
        }
        let mut best: Option<(usize, f64)> = None;
        for &good in self.property.keys() {
            if good == TIME {
                continue;
            }
            let sal = history.salability(good);
            if best.map_or(true, |(_, old)| sal > old) {
                best = Some((good, sal));
            }
        }
        best.map(|(good, _)| good).or_else(|| outputs.first().copied())
    }

    /// Firm pick: first output of an active production line.
    fn pick_firm_product(&self, factuals: &Factuals) -> Option<usize> {
        self.firm_output_goods(factuals).into_iter().next()
    }

    /// Output goods of lines with a positive target.
    fn firm_output_goods(&self, factuals: &Factuals) -> Vec<usize> {
        let mut goods = Vec::new();
        for line in &self.production_line {
            if line.target.unwrap_or(0.0) <= 0.0 {
                continue;
            }
            let Some(process) = factuals.processes.get(&line.process) else {
                continue;
            };
            for output in &process.outputs {
                if output.good != TIME && !goods.contains(&output.good) {
                    goods.push(output.good);
                }
            }
        }
        goods
    }

    /// AMV of on-hand goods, skipping Time.
    fn held_goods_amv(&self, history: &MarketHistory) -> f64 {
        self.property
            .iter()
            .filter(|(good, _)| **good != TIME)
            .map(|(good, row)| row.quantity.max(0.0) * history.price(*good).max(0.0))
            .sum()
    }

    /// Recipe inputs (not Time, not optional, not factors), wage basket, and
    /// stock fence. Used to size a remainder owner's loss cover.
    fn remainder_need_qty(&self, factuals: &Factuals) -> HashMap<usize, f64> {
        let mut need: HashMap<usize, f64> = HashMap::new();
        for line in &self.production_line {
            let target = match line.target {
                Some(t) if t > 0.0 => t,
                _ => continue,
            };
            let Some(process) = factuals.processes.get(&line.process) else {
                continue;
            };
            for input in &process.inputs {
                if input.good == TIME
                    || input.is_optional()
                    || matches!(input.input_type, InputType::Factor)
                {
                    continue;
                }
                *need.entry(input.good).or_insert(0.0) += target * input.amount.max(0.0);
            }
        }
        for worker in &self.workforce {
            if worker.id == 0 {
                continue;
            }
            let hours = worker.hours.max(0.0);
            for term in worker.ordered_payment_terms() {
                if term.good == TIME {
                    continue;
                }
                *need.entry(term.good).or_insert(0.0) += term.promised_qty(hours);
            }
        }
        for (&good, row) in &self.property {
            if good == TIME {
                continue;
            }
            let fence = row.stock_fence();
            if fence > 0.0 {
                let entry = need.entry(good).or_insert(0.0);
                if fence > *entry {
                    *entry = fence;
                }
            }
        }
        need
    }

    /// Goods the remainder owner should transfer, first missing input, then
    /// missing wage good, then production output, then exchange (high
    /// salability first). Time is never taken.
    fn remainder_cover_goods(
        &self,
        need: &HashMap<usize, f64>,
        pop: &Pop,
        factuals: &Factuals,
        history: &MarketHistory,
    ) -> Vec<usize> {
        let mut seen = HashSet::new();
        let mut order = Vec::new();

        let have = |good: usize| {
            self.property
                .get(&good)
                .map(|row| row.quantity.max(0.0))
                .unwrap_or(0.0)
        };
        let missing = |good: usize| need.get(&good).copied().unwrap_or(0.0) > have(good);

        for line in &self.production_line {
            if line.target.unwrap_or(0.0) <= 0.0 {
                continue;
            }
            let Some(process) = factuals.processes.get(&line.process) else {
                continue;
            };
            for input in &process.inputs {
                if input.is_optional() || matches!(input.input_type, InputType::Factor) {
                    continue;
                }
                if missing(input.good) {
                    push_cover_good(&mut order, &mut seen, input.good);
                }
            }
        }
        for worker in &self.workforce {
            if worker.id == 0 {
                continue;
            }
            for term in worker.ordered_payment_terms() {
                if missing(term.good) {
                    push_cover_good(&mut order, &mut seen, term.good);
                }
            }
        }
        for good in self.firm_output_goods(factuals) {
            push_cover_good(&mut order, &mut seen, good);
        }

        let exchange_min = factuals.config.market.exchange_salability_min;
        let mut rest: Vec<usize> = pop
            .property
            .keys()
            .copied()
            .filter(|&good| good != TIME && !seen.contains(&good))
            .collect();
        rest.sort_by(|a, b| {
            let sa = history.salability(*a);
            let sb = history.salability(*b);
            let a_ex = sa >= exchange_min;
            let b_ex = sb >= exchange_min;
            b_ex
                .cmp(&a_ex)
                .then(sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal))
                .then(a.cmp(b))
        });
        for good in rest {
            push_cover_good(&mut order, &mut seen, good);
        }
        order
    }

    /// Remainder owner covers `need AMV - held AMV` from unreserved stock.
    /// Whole units. Returns `(want_amv, paid_amv, goods)`.
    fn cover_remainder_shortfall(
        &mut self,
        pop: &mut Pop,
        factuals: &Factuals,
        history: &MarketHistory,
    ) -> (f64, f64, HashMap<usize, f64>) {
        let need = self.remainder_need_qty(factuals);
        let mut need_amv = 0.0;
        for (&good, &qty) in &need {
            if good == TIME {
                continue;
            }
            need_amv += qty.max(0.0) * history.price(good).max(0.0);
        }
        let want = (need_amv - self.held_goods_amv(history)).max(0.0);
        if want <= 0.0 {
            return (0.0, 0.0, HashMap::new());
        }
        let order = self.remainder_cover_goods(&need, pop, factuals, history);
        let fence = self.remainder_cover_fence_goods(factuals, &need);
        let mut remaining = want;
        let mut paid_amv = 0.0;
        let mut paid = HashMap::new();
        for good in order {
            if remaining <= 0.0 {
                break;
            }
            let price = history.price(good);
            if price <= 0.0 {
                continue;
            }
            let available = pop
                .property
                .get(&good)
                .map(|row| row.saved())
                .unwrap_or(0.0);
            let give = whole_units((remaining / price).min(available));
            if give <= 0.0 {
                continue;
            }
            pop.debit_good(good, give);
            self.credit_good(good, give);
            if fence.contains(&good) {
                if let Some(row) = self.property.get_mut(&good) {
                    let floor = row.stock_fence() + give;
                    row.reserve_target = row.reserve_target.max(floor);
                    row.sync_reserve();
                }
            }
            *paid.entry(good).or_insert(0.0) += give;
            let got = give * price;
            paid_amv += got;
            remaining -= got;
        }
        (want, paid_amv, paid)
    }

    /// Recipe inputs and outputs that are not a missing wage good. Fenced so
    /// remainder leftover cannot take the same-morning cover back.
    fn remainder_cover_fence_goods(
        &self,
        factuals: &Factuals,
        need: &HashMap<usize, f64>,
    ) -> HashSet<usize> {
        let have = |good: usize| {
            self.property
                .get(&good)
                .map(|row| row.quantity.max(0.0))
                .unwrap_or(0.0)
        };
        let mut wage_missing = HashSet::new();
        for worker in &self.workforce {
            if worker.id == 0 {
                continue;
            }
            for term in worker.ordered_payment_terms() {
                if need.get(&term.good).copied().unwrap_or(0.0) > have(term.good) {
                    wage_missing.insert(term.good);
                }
            }
        }
        let mut fence = HashSet::new();
        for line in &self.production_line {
            if line.target.unwrap_or(0.0) <= 0.0 {
                continue;
            }
            let Some(process) = factuals.processes.get(&line.process) else {
                continue;
            };
            for input in &process.inputs {
                if input.good != TIME && !wage_missing.contains(&input.good) {
                    fence.insert(input.good);
                }
            }
            for output in &process.outputs {
                if output.good != TIME && !wage_missing.contains(&output.good) {
                    fence.insert(output.good);
                }
            }
        }
        fence
    }

    /// Destroyed Time on current production-line targets.
    fn plan_time_need(&self, factuals: &Factuals) -> f64 {
        let mut need = 0.0;
        for line in &self.production_line {
            let Some(target) = line.target else {
                continue;
            };
            if target <= 0.0 {
                continue;
            }
            let Some(process) = factuals.processes.get(&line.process) else {
                continue;
            };
            for input in &process.inputs {
                if input.good == TIME && matches!(input.input_type, InputType::Destroyed) {
                    need += target * input.amount.max(0.0);
                }
            }
        }
        need
    }

    /// True when remaining purchase targets are much larger than today's buys.
    fn needs_much_more_shopping(&self) -> bool {
        let mut want = 0.0;
        let mut got = 0.0;
        for row in self.property.values() {
            want += row.purchase_target.max(0.0);
            got += row.bought.max(0.0);
        }
        want > 0.0 && want > got * 2.0
    }
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

#[cfg(test)]
mod settle_labor_contracts_should {
    use super::*;
    use crate::game::actor::Actor;
    use crate::game::factuals::Factuals;
    use crate::game::firm::{FirmPRow, ProductionLine};
    use crate::game::good::TIME;
    use crate::game::household::Household;
    use crate::game::pop::{DemoRow, Pop, PopPRow, PopRecords};
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput};
    use crate::game::sentiment::Sentiment;

    const COIN: usize = 5;
    const BREAD: usize = 3;
    const WOOD: usize = 10;
    const PLANK: usize = 20;

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

    fn with_good(mut pop: Pop, good: usize, qty: f64) -> Pop {
        pop.property.insert(good, PopPRow::new(qty));
        pop
    }

    fn mill_factuals() -> Factuals {
        let mut factuals = Factuals::new();
        factuals.processes.insert(
            1,
            Process::new(1, "mill", 0)
                .with_input(ProcessInput::new(
                    WOOD,
                    2.0,
                    true,
                    InputType::Destroyed,
                    false,
                ))
                .with_output(ProcessOutput::new(PLANK, 1.0, true)),
        );
        factuals
    }

    fn mill_line(target: f64) -> ProductionLine {
        ProductionLine {
            process: 1,
            target: Some(target),
            inputs: vec![WOOD],
            historical_productivity: 0.0,
            last_success_rate: 0.0,
            last_iterations: 0.0,
            last_effects: vec![],
            last_missing_goods: vec![],
            last_amv_consumed: 0.0,
            last_amv_produced: 0.0,
        }
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

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &Factuals::new());

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

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &Factuals::new());

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

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &Factuals::new());

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

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &Factuals::new());

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

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &Factuals::new());

        assert_eq!(pops[&2].property[&COIN].quantity, 1.0);
        let owner = report.owner.expect("owner paid");
        assert_eq!(owner.paid[&COIN], 2.0);
        assert!(!owner.remainder);
        assert_eq!(firm.property[&COIN].quantity, 7.0);
    }

    #[test]
    fn remainder_takes_leftover_even_when_profit_is_zero() {
        let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(Actor::Pop(3))
            .with_owner_remainder();
        firm.property.insert(
            COIN,
            FirmPRow::new()
                .with_quantity(20.0)
                .with_stock_target(2.0)
                .with_growth_target(5.0),
        );
        let mut pops = HashMap::from([(3, make_pop(3))]);
        let history = history_prices(&[(COIN, 1.0)]);

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &Factuals::new());

        let owner = report.owner.expect("remainder owner");
        assert!(owner.remainder);
        assert_eq!(owner.paid[&COIN], 13.0);
        assert_eq!(firm.property[&COIN].quantity, 7.0);
    }

    #[test]
    fn remainder_does_not_take_the_sell_plan() {
        let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(Actor::Pop(3))
            .with_owner_remainder();
        firm.property.insert(
            WOOD,
            FirmPRow::new()
                .with_quantity(20.0)
                .with_sell_target(15.0),
        );
        firm.property.insert(COIN, FirmPRow::new().with_quantity(4.0));
        let mut pops = HashMap::from([(3, make_pop(3))]);
        let history = history_prices(&[(WOOD, 1.0), (COIN, 1.0)]);

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &Factuals::new());

        let owner = report.owner.expect("remainder owner");
        assert_eq!(owner.paid.get(&WOOD).copied().unwrap_or(0.0), 5.0);
        assert_eq!(owner.paid.get(&COIN).copied().unwrap_or(0.0), 4.0);
        assert_eq!(firm.property[&WOOD].quantity, 15.0);
        assert_eq!(firm.property[&COIN].quantity, 0.0);
    }

    #[test]
    fn remainder_caps_posted_sell_by_max_salability() {
        let mut firm = Firm::new(1, "mill".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(Actor::Pop(3))
            .with_owner_remainder();
        firm.production_line.push(mill_line(10.0));
        firm.property.insert(
            PLANK,
            FirmPRow::new()
                .with_quantity(10.0)
                .with_sell_target(10.0),
        );
        let mut pops = HashMap::from([(3, make_pop(3))]);
        let mut history = MarketHistory::new();
        history.prices.insert(PLANK, 1.0);
        history.salability.insert(PLANK, 0.3);

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &mill_factuals());

        let owner = report.owner.expect("remainder owner");
        assert_eq!(owner.paid.get(&PLANK).copied().unwrap_or(0.0), 7.0);
        assert_eq!(firm.property[&PLANK].quantity, 3.0);
    }

    #[test]
    fn limited_share_caps_below_leftover_till() {
        let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(Actor::Pop(3))
            .with_owner_profit_share(0.5);
        firm.property.insert(COIN, FirmPRow::new().with_quantity(20.0));
        firm.records.sold_amv = 10.0;
        firm.records.sold_cost_amv = 0.0;
        let mut pops = HashMap::from([(3, make_pop(3))]);
        let history = history_prices(&[(COIN, 1.0)]);

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &Factuals::new());

        let owner = report.owner.expect("share owner");
        assert!(!owner.remainder);
        assert_eq!(owner.paid[&COIN], 5.0);
        assert_eq!(firm.property[&COIN].quantity, 15.0);
    }

    #[test]
    fn remainder_follows_worker_dividend() {
        let worker = Workforce::new(2)
            .with_hours(1.0)
            .with_payment(PaymentTerm::new(COIN, 1.0))
            .with_profit_share(0.2);
        let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(Actor::Pop(3))
            .with_owner_remainder()
            .with_workforce(worker);
        firm.property.insert(COIN, FirmPRow::new().with_quantity(20.0));
        firm.records.sold_amv = 10.0;
        firm.records.sold_cost_amv = 0.0;
        let mut pops = HashMap::from([
            (2, with_time(make_pop(2), 48.0)),
            (3, make_pop(3)),
        ]);
        let history = history_prices(&[(COIN, 1.0)]);

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &Factuals::new());

        assert_eq!(pops[&2].property[&COIN].quantity, 3.0);
        let owner = report.owner.expect("remainder after dividend");
        assert!(owner.remainder);
        assert_eq!(owner.paid[&COIN], 17.0);
        assert_eq!(firm.property[&COIN].quantity, 0.0);
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

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &Factuals::new());

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

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &Factuals::new());

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

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &Factuals::new());

        assert_eq!(pops[&2].property[&COIN].quantity, 4.0);
        assert_eq!(pops[&2].property[&BREAD].quantity, 1.0);
        assert!((report.workers[0].paid_amv - 6.0).abs() < 1e-12);
        assert!((report.workers[0].time_given - 4.0).abs() < 1e-12);
    }

    #[test]
    fn remainder_owner_covers_missing_input_on_a_loss() {
        let mut firm = Firm::new(1, "mill".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(Actor::Pop(3))
            .with_owner_remainder();
        firm.production_line.push(mill_line(5.0));
        let owner = with_good(make_pop(3), WOOD, 10.0);
        let mut pops = HashMap::from([(3, owner)]);
        let history = history_prices(&[(WOOD, 1.0), (PLANK, 1.0)]);
        let factuals = mill_factuals();

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &factuals);

        let owner = report.owner.expect("remainder cover");
        assert!(owner.remainder);
        assert!((owner.recap_amv - 10.0).abs() < 1e-12);
        assert_eq!(owner.recap[&WOOD], 10.0);
        assert_eq!(firm.property[&WOOD].quantity, 10.0);
        assert_eq!(pops[&3].property[&WOOD].quantity, 0.0);
    }

    #[test]
    fn remainder_cover_prefers_missing_input_over_exchange() {
        let mut firm = Firm::new(1, "mill".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(Actor::Pop(3))
            .with_owner_remainder();
        firm.production_line.push(mill_line(5.0));
        firm.property.insert(
            WOOD,
            FirmPRow::new()
                .with_quantity(4.0)
                .with_stock_target(4.0),
        );
        let owner = with_good(with_good(make_pop(3), WOOD, 10.0), COIN, 20.0);
        let mut pops = HashMap::from([(3, owner)]);
        let history = history_prices(&[(WOOD, 1.0), (PLANK, 1.0), (COIN, 1.0)]);
        let factuals = mill_factuals();

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &factuals);

        let owner = report.owner.expect("cover");
        assert_eq!(owner.recap.get(&WOOD).copied().unwrap_or(0.0), 6.0);
        assert_eq!(owner.recap.get(&COIN).copied().unwrap_or(0.0), 0.0);
        assert_eq!(firm.property[&WOOD].quantity, 10.0);
        assert_eq!(pops[&3].property[&WOOD].quantity, 4.0);
        assert_eq!(pops[&3].property[&COIN].quantity, 20.0);
    }

    #[test]
    fn remainder_cover_uses_wage_goods_after_inputs() {
        let worker = Workforce::new(2)
            .with_hours(5.0)
            .with_payment(PaymentTerm::new(COIN, 1.0));
        let mut firm = Firm::new(1, "mill".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(Actor::Pop(3))
            .with_owner_remainder()
            .with_workforce(worker);
        firm.production_line.push(mill_line(5.0));
        firm.property.insert(
            WOOD,
            FirmPRow::new()
                .with_quantity(10.0)
                .with_stock_target(10.0),
        );
        let owner = with_good(with_good(make_pop(3), COIN, 8.0), PLANK, 8.0);
        let mut pops = HashMap::from([
            (2, with_time(make_pop(2), 48.0)),
            (3, owner),
        ]);
        let history = history_prices(&[(WOOD, 1.0), (PLANK, 1.0), (COIN, 1.0)]);
        let factuals = mill_factuals();

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &factuals);

        let owner = report.owner.expect("cover");
        assert_eq!(owner.recap.get(&COIN).copied().unwrap_or(0.0), 5.0);
        assert_eq!(owner.recap.get(&PLANK).copied().unwrap_or(0.0), 0.0);
        assert_eq!(pops[&2].property[&COIN].quantity, 5.0);
        assert!((report.workers[0].time_given - 5.0).abs() < 1e-12);
        assert_eq!(
            firm.property.get(&COIN).map(|row| row.quantity).unwrap_or(0.0),
            0.0
        );
        assert_eq!(pops[&3].property[&COIN].quantity, 3.0);
    }

    #[test]
    fn remainder_cover_uses_output_before_exchange() {
        let mut firm = Firm::new(1, "mill".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(Actor::Pop(3))
            .with_owner_remainder();
        firm.production_line.push(mill_line(5.0));
        let owner = with_good(with_good(make_pop(3), PLANK, 10.0), COIN, 10.0);
        let mut pops = HashMap::from([(3, owner)]);
        let history = history_prices(&[(WOOD, 1.0), (PLANK, 1.0), (COIN, 1.0)]);
        let factuals = mill_factuals();

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &factuals);

        let owner = report.owner.expect("cover");
        assert!((owner.recap_amv - 10.0).abs() < 1e-12);
        assert_eq!(owner.recap.get(&PLANK).copied().unwrap_or(0.0), 10.0);
        assert_eq!(owner.recap.get(&COIN).copied().unwrap_or(0.0), 0.0);
        assert_eq!(firm.property[&PLANK].quantity, 10.0);
        assert_eq!(pops[&3].property[&COIN].quantity, 10.0);
    }

    #[test]
    fn limited_owner_does_not_cover_a_shortfall() {
        let mut firm = Firm::new(1, "mill".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(Actor::Pop(3))
            .with_owner_profit_share(0.5);
        firm.production_line.push(mill_line(5.0));
        let owner = with_good(make_pop(3), WOOD, 10.0);
        let mut pops = HashMap::from([(3, owner)]);
        let history = history_prices(&[(WOOD, 1.0), (PLANK, 1.0)]);
        let factuals = mill_factuals();

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &factuals);

        assert!(report.owner.is_none());
        assert_eq!(pops[&3].property[&WOOD].quantity, 10.0);
        assert_eq!(
            firm.property.get(&WOOD).map(|row| row.quantity).unwrap_or(0.0),
            0.0
        );
    }

    #[test]
    fn profitable_remainder_does_not_cover_a_shortfall() {
        let mut firm = Firm::new(1, "mill".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(Actor::Pop(3))
            .with_owner_remainder();
        firm.production_line.push(mill_line(5.0));
        firm.records.sold_amv = 20.0;
        firm.records.sold_cost_amv = 5.0;
        let owner = with_good(make_pop(3), WOOD, 10.0);
        let mut pops = HashMap::from([(3, owner)]);
        let history = history_prices(&[(WOOD, 1.0), (PLANK, 1.0)]);
        let factuals = mill_factuals();

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &factuals);

        let owner = report.owner.expect("remainder");
        assert!(owner.remainder);
        assert_eq!(owner.recap_paid_amv, 0.0);
        assert!(owner.recap.is_empty());
        assert_eq!(pops[&3].property[&WOOD].quantity, 10.0);
    }

    #[test]
    fn remainder_cover_gives_what_the_owner_has() {
        let mut firm = Firm::new(1, "mill".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(Actor::Pop(3))
            .with_owner_remainder();
        firm.production_line.push(mill_line(5.0));
        let owner = with_good(make_pop(3), WOOD, 3.0);
        let mut pops = HashMap::from([(3, owner)]);
        let history = history_prices(&[(WOOD, 1.0), (PLANK, 1.0)]);
        let factuals = mill_factuals();

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &factuals);

        let owner = report.owner.expect("cover");
        assert!((owner.recap_amv - 10.0).abs() < 1e-12);
        assert_eq!(owner.recap[&WOOD], 3.0);
        assert!((owner.recap_paid_amv - 3.0).abs() < 1e-12);
        assert_eq!(firm.property[&WOOD].quantity, 3.0);
        assert_eq!(pops[&3].property[&WOOD].quantity, 0.0);
    }

    #[test]
    fn remainder_cover_skips_reserved_stock_and_time() {
        let mut firm = Firm::new(1, "mill".into(), 1, hexx::Hex::new(0, 0))
            .with_owner(Actor::Pop(3))
            .with_owner_remainder();
        firm.production_line.push(mill_line(5.0));
        let mut owner = with_good(with_time(make_pop(3), 20.0), WOOD, 4.0);
        owner.property.get_mut(&WOOD).unwrap().reserved = 4.0;
        let mut pops = HashMap::from([(3, owner)]);
        let history = history_prices(&[(WOOD, 1.0), (PLANK, 1.0), (TIME, 1.0)]);
        let factuals = mill_factuals();

        let report = LaborSettlement::settle(&mut firm, &mut pops, &history, &factuals);

        let owner = report.owner.expect("cover");
        assert!(owner.recap.is_empty());
        assert_eq!(pops[&3].property[&WOOD].quantity, 4.0);
        assert_eq!(pops[&3].property[&TIME].quantity, 20.0);
        assert_eq!(
            firm.property.get(&TIME).map(|row| row.quantity).unwrap_or(0.0),
            0.0
        );
    }
}

#[cfg(test)]
mod budget_labor_should {
    use super::*;
    use crate::game::config::labor_constants;
    use crate::game::factuals::Factuals;
    use crate::game::firm::{FirmPRow, ProductionLine};
    use crate::game::good::TIME;
    use crate::game::process::{InputType, Process, ProcessInput};

    const COIN: usize = 5;

    fn history_coin() -> MarketHistory {
        let mut history = MarketHistory::new();
        history.prices.insert(COIN, 1.0);
        history.salability.insert(COIN, 0.9);
        history
    }

    fn time_process() -> Process {
        Process::new(1, "farm grain".to_string(), 0).with_input(ProcessInput::new(
            TIME,
            3.0,
            true,
            InputType::Destroyed,
            false,
        ))
    }

    fn line(target: f64) -> ProductionLine {
        ProductionLine {
            process: 1,
            target: Some(target),
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

    const GRAIN: usize = 1;

    fn farm_with_hours(hours: f64) -> Firm {
        let worker = Workforce::new(2)
            .with_hours(hours)
            .with_payment(PaymentTerm::new(COIN, 1.0));
        let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0))
            .with_workforce(worker);
        firm.production_line.push(line(5.0));
        firm.property.insert(COIN, FirmPRow::new().with_quantity(100.0));
        firm
    }

    fn worker_pop(id: usize) -> Pop {
        use crate::game::household::Household;
        use crate::game::pop::{DemoRow, PopPRow, PopRecords};
        use crate::game::sentiment::Sentiment;
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

    fn pops_for(firm: &Firm) -> HashMap<usize, Pop> {
        firm.workforce
            .iter()
            .filter(|w| w.id != 0)
            .map(|w| (w.id, worker_pop(w.id)))
            .collect()
    }

    fn with_grain_need(mut pop: Pop, satisfaction: f64) -> Pop {
        use crate::game::desire::{Desire, DesireSource, DesireTarget, DesireTargetType};
        use crate::game::scalingfactor::ScalingFactor;
        pop.desires[0].push(Desire {
            source: DesireSource::Species(0, 0),
            priority: 0,
            target: vec![DesireTarget::new(GRAIN, DesireTargetType::Consume, 1.0)],
            amount: 2.0,
            satisfaction,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Household(1.0),
            decay: 0.0,
        });
        pop
    }

    fn grain_process() -> Process {
        time_process().with_output(crate::game::process::ProcessOutput::new(GRAIN, 6.0, true))
    }

    #[test]
    fn interval_zero_skips() {
        let mut firm = farm_with_hours(1.0);
        let mut factuals = Factuals::new().with_process(time_process());
        factuals.config.labor.budget_interval = 0;
        let pops = pops_for(&firm);
        firm.budget_labor(&factuals, &history_coin(), &pops, 1);
        assert!((firm.workforce[0].hours - 1.0).abs() < 1e-12);
    }

    #[test]
    fn off_interval_skips() {
        let mut firm = farm_with_hours(1.0);
        let mut factuals = Factuals::new().with_process(time_process());
        factuals.config.labor.budget_interval = 3;
        let pops = pops_for(&firm);
        firm.budget_labor(&factuals, &history_coin(), &pops, 1);
        assert!((firm.workforce[0].hours - 1.0).abs() < 1e-12);
        firm.budget_labor(&factuals, &history_coin(), &pops, 3);
        assert!(firm.workforce[0].hours > 1.0);
    }

    #[test]
    fn hours_snap_to_recipe_time() {
        let mut firm = farm_with_hours(1.0);
        let factuals = Factuals::new().with_process(time_process());
        let pops = pops_for(&firm);
        firm.budget_labor(&factuals, &history_coin(), &pops, 1);
        assert_eq!(firm.workforce[0].hours, 16.0);
        assert_eq!(firm.workforce[0].id, 2);
        assert_eq!(firm.workforce.len(), 1);
    }

    #[test]
    fn short_till_does_not_cut_hours() {
        let mut firm = farm_with_hours(10.0);
        firm.property.insert(COIN, FirmPRow::new().with_quantity(4.0));
        let factuals = Factuals::new().with_process(time_process());
        let pops = pops_for(&firm);
        firm.budget_labor(&factuals, &history_coin(), &pops, 1);
        let worker = &firm.workforce[0];
        assert_eq!(worker.hours, 16.0);
        assert!(
            worker.payment.iter().any(|t| !t.flat && t.amount >= labor_constants::WAGE_AMOUNT_MIN)
        );
    }

    #[test]
    fn empty_till_keeps_hours_and_rate() {
        let mut firm = farm_with_hours(10.0);
        firm.property.insert(COIN, FirmPRow::new().with_quantity(0.0));
        let factuals = Factuals::new().with_process(time_process());
        let pops = pops_for(&firm);
        firm.budget_labor(&factuals, &history_coin(), &pops, 1);
        assert_eq!(firm.workforce[0].hours, 16.0);
        assert_eq!(
            firm.workforce[0]
                .payment
                .iter()
                .find(|t| !t.flat && t.good == COIN)
                .map(|t| t.amount),
            Some(labor_constants::WAGE_AMOUNT_MIN)
        );
    }

    #[test]
    fn wage_rate_does_not_fall_to_zero() {
        let mut firm = farm_with_hours(10.0);
        firm.property.insert(COIN, FirmPRow::new().with_quantity(1.0));
        let factuals = Factuals::new().with_process(time_process());
        let pops = pops_for(&firm);
        firm.budget_labor(&factuals, &history_coin(), &pops, 1);
        let min_scale = firm.workforce[0]
            .payment
            .iter()
            .filter(|t| !t.flat)
            .map(|t| t.amount)
            .fold(f64::INFINITY, f64::min);
        assert!(min_scale >= labor_constants::WAGE_AMOUNT_MIN, "{min_scale}");
    }

    #[test]
    fn folds_fat_flat_into_hourly() {
        let mut worker = Workforce::new(2)
            .with_hours(10.0)
            .with_payment(PaymentTerm::new(COIN, 1.0))
            .with_payment(PaymentTerm::flat(COIN, 15.0));
        worker.fold_flat_into_hourly();
        let scaling = worker
            .payment
            .iter()
            .find(|t| !t.flat && t.good == COIN)
            .expect("hourly");
        assert_eq!(scaling.amount, 2.0);
        let flat = worker.payment.iter().find(|t| t.flat && t.good == COIN);
        assert_eq!(flat.map(|t| t.amount), Some(5.0));
    }

    #[test]
    fn angry_pop_gets_a_flat_bonus() {
        let mut firm = farm_with_hours(10.0);
        let factuals = Factuals::new().with_process(grain_process());
        let mut pops = pops_for(&firm);
        pops.get_mut(&2).expect("worker").sentiment =
            crate::game::sentiment::Sentiment::from_parts(0.0, 0.1, 0.5, 0.4, 0.0);
        firm.property.insert(GRAIN, FirmPRow::new().with_quantity(10.0));
        let mut history = history_coin();
        history.prices.insert(GRAIN, 1.0);
        history.salability.insert(GRAIN, 0.5);
        firm.budget_labor(&factuals, &history, &pops, 1);
        assert!(
            firm.workforce[0]
                .payment
                .iter()
                .any(|t| t.flat && t.amount >= 1.0),
            "{:?}",
            firm.workforce[0].payment
        );
    }

    #[test]
    fn calm_profitable_firm_adds_product_bonus() {
        let mut firm = farm_with_hours(10.0);
        firm.records.profit_ratio = 1.5;
        let factuals = Factuals::new().with_process(grain_process());
        let mut pops = pops_for(&firm);
        let worker = pops.remove(&2).expect("worker");
        pops.insert(2, with_grain_need(worker, 0.0));
        let mut history = history_coin();
        history.prices.insert(GRAIN, 1.0);
        history.salability.insert(GRAIN, 0.5);
        firm.budget_labor(&factuals, &history, &pops, 1);
        assert!(
            firm.workforce[0]
                .payment
                .iter()
                .any(|t| t.good == GRAIN && t.amount >= 1.0),
            "{:?}",
            firm.workforce[0].payment
        );
    }

    #[test]
    fn hourly_wage_amv_is_promised_over_hours() {
        let worker = Workforce::new(2)
            .with_hours(10.0)
            .with_payment(PaymentTerm::new(COIN, 1.0));
        let history = history_coin();
        assert!((worker.hourly_wage_amv(&history) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn barely_profitable_holds_wages() {
        let mut firm = farm_with_hours(10.0);
        firm.records.profit_ratio = 1.1;
        let factuals = Factuals::new().with_process(grain_process());
        let mut pops = pops_for(&firm);
        let worker = pops.remove(&2).expect("worker");
        pops.insert(2, with_grain_need(worker, 0.0));
        let mut history = history_coin();
        history.prices.insert(GRAIN, 1.0);
        history.salability.insert(GRAIN, 0.5);
        firm.budget_labor(&factuals, &history, &pops, 1);
        assert!(
            !firm.workforce[0].payment.iter().any(|t| t.good == GRAIN),
            "{:?}",
            firm.workforce[0].payment
        );
    }

    #[test]
    fn satiated_kind_skips_product_bonus() {
        let mut firm = farm_with_hours(10.0);
        firm.records.profit_ratio = 1.5;
        let factuals = Factuals::new().with_process(grain_process());
        let mut pops = pops_for(&firm);
        let worker = pops.remove(&2).expect("worker");
        pops.insert(2, with_grain_need(worker, 2.0));
        let mut history = history_coin();
        history.prices.insert(GRAIN, 1.0);
        history.salability.insert(GRAIN, 0.5);
        firm.budget_labor(&factuals, &history, &pops, 1);
        assert!(
            !firm.workforce[0].payment.iter().any(|t| t.good == GRAIN),
            "{:?}",
            firm.workforce[0].payment
        );
    }

    #[test]
    fn hours_include_todays_transport_plus_one() {
        let mut firm = farm_with_hours(1.0);
        firm.transport_spent = 4.0;
        let factuals = Factuals::new().with_process(time_process());
        let pops = pops_for(&firm);
        firm.budget_labor(&factuals, &history_coin(), &pops, 1);
        assert_eq!(firm.workforce[0].hours, 20.0);
    }

    #[test]
    fn unprofitable_trims_flats_not_hours() {
        let worker = Workforce::new(2)
            .with_hours(10.0)
            .with_payment(PaymentTerm::new(COIN, 1.0))
            .with_payment(PaymentTerm::flat(COIN, 3.0));
        let mut firm = Firm::new(1, "farm".into(), 1, hexx::Hex::new(0, 0))
            .with_workforce(worker);
        firm.production_line.push(line(5.0));
        firm.property.insert(COIN, FirmPRow::new().with_quantity(100.0));
        firm.records.profit_ratio = 0.5;
        let factuals = Factuals::new().with_process(time_process());
        let pops = pops_for(&firm);
        firm.budget_labor(&factuals, &history_coin(), &pops, 1);
        assert_eq!(firm.workforce[0].hours, 16.0);
        let flat = firm.workforce[0]
            .payment
            .iter()
            .find(|t| t.flat && t.good == COIN)
            .map(|t| t.amount);
        assert_eq!(flat, Some(2.0));
    }
}
