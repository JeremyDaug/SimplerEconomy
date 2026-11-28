use std::collections::{HashMap, HashSet};

use rand::RngCore;

use crate::game::config::{FIRM_FINISH_GRACE, FIRM_MIN_MARGIN, FIRM_MIN_TARGET, FIRM_SELL_SHARE};
use crate::game::factuals::Factuals;
use crate::game::good::{GoodTag, TIME};
use crate::game::market::Market;
use crate::game::pop::{
    can_fund_one, market_price, optional_keeps_value, shop_firm, Pop, SellOffer, ShopBook,
};
use crate::game::process::{InputType, Process, ProcessResult};
use crate::game::actor::Actor;

/// The day's money, in abstract value.
///
/// [`Market::pay_owners`] clears this and writes the wage and dividend that
/// were paid. A sale adds [`Self::sold_value`] at the price then.
/// [`Firm::produce`] adds the input cost and the output value.
/// [`Firm::record_keeping`] leaves it.
#[derive(Debug, Clone, Copy, Default)]
pub struct FirmBudget {
    /// Required inputs the runs destroyed or consumed.
    ///
    /// Capital and time are not in this cost.
    pub input_cost: f64,
    /// Goods the runs put out.
    pub output_value: f64,
    /// Goods sold, at the price when they sold.
    pub sold_value: f64,
    /// Tender paid as the owner wage.
    pub wage: f64,
    /// Tender paid as the owner dividend.
    pub dividend: f64,
}

/// A firm is one workshop: production and stock.
///
/// Labor is a process input bought on the market. This firm does not keep
/// a workforce, and it does not record who owns it.
#[derive(Debug, Clone)]
pub struct Firm {
    pub id: usize,
    pub name: String,
    pub property: HashMap<usize, FirmPRow>,
    pub production_line: Vec<ProductionLine>,
    /// Today's costs and receipts. [`Market::pay_owners`] starts it over.
    pub budget: FirmBudget,
    /// Set when this firm cannot fund one run of its first line.
    ///
    /// [`Self::planning`] writes it. A dead firm does not plan, shop,
    /// produce, or pay its owners.
    pub dead: bool,
}

/// One process the firm can run, plus the day's quota.
#[derive(Debug, Clone)]
pub struct ProductionLine {
    pub process: usize,
    /// Iterations sought. `None` means as many as inputs allow.
    pub target: Option<f64>,
    /// Optional inputs the line is allowed to draw.
    pub inputs: Vec<usize>,
    /// Iterations this line was asked to run today.
    ///
    /// [`Firm::produce`] writes this before the run.
    pub attempted: f64,
    /// Iterations this line finished today.
    ///
    /// [`Firm::produce`] writes this from the process result.
    pub completed: f64,
}

/// Stock of one good held by a firm.
#[derive(Debug, Clone, Copy, Default)]
pub struct FirmPRow {
    pub quantity: f64,
    /// Units earmarked and not for sale.
    pub reserve: f64,
    /// Capital tied up in a run. Returned to `quantity` at decay.
    pub used: f64,
    /// Fresh units in `quantity`. [`Firm::decay_goods`] leaves this portion in place.
    ///
    /// [`Firm::record_keeping`] clears it after that night.
    pub fresh: f64,
    /// Units lost to rot tonight.
    ///
    /// [`Firm::decay_goods`] writes this. [`Firm::record_keeping`] clears it.
    pub lost: f64,
    /// Destroyed in production today. Already left `quantity`.
    pub consumed: f64,
    /// Units a haul destroyed today. Already left `quantity`.
    ///
    /// [`Firm::record_keeping`] clears it.
    pub spent: f64,
    /// Units sold today.
    ///
    /// A completed sale adds to this. [`Firm::record_keeping`] leaves it.
    /// The next market day clears it before the new sales.
    pub sold: f64,
    /// Units produced today, including decay byproducts when a run records them.
    pub produced: f64,
    /// Units produced on the last day.
    ///
    /// [`Firm::record_keeping`] copies `produced` here before clearing it.
    pub made: f64,
}

impl Firm {
    /// # New
    ///
    /// A firm with this id and name.
    ///
    /// An empty budget, and no property or production lines. It is not
    /// dead.
    pub fn new(id: usize, name: String) -> Self {
        Self {
            id,
            name,
            property: HashMap::new(),
            production_line: vec![],
            budget: FirmBudget::default(),
            dead: false,
        }
    }

    /// # Is Dead
    ///
    /// Whether this firm has been marked dead.
    ///
    /// Returns [`Self::dead`].
    pub fn is_dead(&self) -> bool {
        self.dead
    }

    /// # Produced Goods
    ///
    /// Goods these lines output.
    ///
    /// `factuals` supplies the process. A missing process is skipped.
    /// Returns the set of output good ids.
    pub fn produced_goods(&self, factuals: &Factuals) -> HashSet<usize> {
        let mut goods = HashSet::new();
        for line in &self.production_line {
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            for output in &process.outputs {
                goods.insert(output.good);
            }
        }
        goods
    }

    /// # Decay Goods
    ///
    /// End-of-day decay for this firm's stock.
    ///
    /// `factuals` supplies each good. `used` returns to `quantity`, then the
    /// aging part of `quantity` decays by the good's rate. [`FirmPRow::fresh`]
    /// stays. [`GoodTag::Exposure`] skips decay while the good is owned.
    /// Byproducts are credited into `quantity`. [`FirmPRow::lost`] is the
    /// units that rotted.
    pub fn decay_goods(&mut self, factuals: &Factuals) {
        let mut gains: HashMap<usize, f64> = HashMap::new();
        for (&good_id, row) in self.property.iter_mut() {
            debug_assert!(row.used.is_finite() && row.used >= 0.0);
            if row.used > 0.0 {
                row.quantity += row.used;
                row.used = 0.0;
            }

            debug_assert!(row.quantity.is_finite() && row.quantity >= 0.0);
            row.keep_fresh_inside();
            debug_assert!(row.fresh.is_finite() && row.fresh >= 0.0);
            debug_assert!(row.fresh <= row.quantity);
            let good = factuals.find_good(good_id);
            let exposure = good.tags.contains(&GoodTag::Exposure);
            // Fresh stock stays whole. Only the older pile rots.
            let fresh = row.fresh;
            let aging = row.quantity - fresh;
            debug_assert!(good.decay_rate.is_finite() && good.decay_rate >= 0.0);
            let mut lost = 0.0;
            if !exposure && good.decay_rate > 0.0 && aging > 0.0 {
                lost = aging * good.decay_rate;
                row.quantity -= lost;
                for (&byproduct, &ratio) in &good.decay_result {
                    debug_assert!(ratio.is_finite() && ratio >= 0.0);
                    if ratio > 0.0 && lost > 0.0 {
                        *gains.entry(byproduct).or_insert(0.0) += lost * ratio;
                    }
                }
            }
            row.lost = lost;
        }

        for (good_id, amount) in gains {
            if amount == 0.0 {
                continue;
            }
            self.property.entry(good_id).or_default().quantity += amount;
        }
    }

    /// # Holding
    ///
    /// Units of `good` this firm holds.
    ///
    /// The row's [`FirmPRow::holding`]. A good the firm has never held is 0.
    /// Returns that amount.
    pub fn holding(&self, good: usize) -> f64 {
        self.property.get(&good).map(FirmPRow::holding).unwrap_or(0.0)
    }

    /// # Reset Day
    ///
    /// Clears yesterday's fresh marker and yesterday's production record.
    ///
    /// `fresh` and `produced` on every property row are set to 0. Quantity
    /// stays, so those units can rot on the coming night.
    pub fn reset_day(&mut self) {
        for row in self.property.values_mut() {
            row.fresh = 0.0;
            row.produced = 0.0;
        }
    }

    /// # Clear Property Records
    ///
    /// Zeros today's consumed counter. Leaves stock, `fresh`, and `produced`.
    pub fn clear_property_records(&mut self) {
        for row in self.property.values_mut() {
            row.consumed = 0.0;
        }
    }

    /// # Release Good
    ///
    /// Removes up to `units` of `good` from on-hand stock.
    ///
    /// Old units go before fresh. `reserve` falls by the units that left,
    /// and not below 0. A missing good releases nothing. Returns the units
    /// removed.
    pub fn release_good(&mut self, good: usize, units: f64) -> f64 {
        debug_assert!(units.is_finite() && units >= 0.0);
        if units <= 0.0 {
            return 0.0;
        }
        let Some(row) = self.property.get_mut(&good) else {
            return 0.0;
        };
        let take = units.min(row.quantity).max(0.0);
        if take == 0.0 {
            return 0.0;
        }
        row.spend_aged_first(take);
        row.quantity -= take;
        row.reserve = (row.reserve - take).max(0.0);
        take
    }

    /// # Take Good
    ///
    /// Removes this good's row and returns the on-hand quantity.
    ///
    /// `fresh` is a portion of that quantity, so it is not added again.
    /// A missing good returns 0.
    pub fn take_good(&mut self, good: usize) -> f64 {
        self.property
            .remove(&good)
            .map(|row| row.quantity)
            .unwrap_or(0.0)
    }

    /// # Clear Sold
    ///
    /// Sets every property row's `sold` back to 0.
    ///
    /// The market day calls this before the new offers are posted.
    pub(crate) fn clear_sold(&mut self) {
        for row in self.property.values_mut() {
            row.sold = 0.0;
        }
    }

    /// # Post Sale Offers
    ///
    /// Reserves this firm's inputs and lists the outputs it will sell.
    ///
    /// `factuals` supplies the processes and `firm.stock_offer_share`.
    /// Earlier reserves do not carry in. Required inputs are reserved
    /// first, then listed optionals that keep the iteration's margin.
    /// Prices in that check are 1. An output with anything left unreserved
    /// becomes one offer: the units made on the last day, up to what is
    /// unreserved, plus `stock_offer_share` of the unreserved units left
    /// over. The rest is held stock and stays off the book, as do other
    /// goods. A dead firm lists nothing. Returns those offers, in good-id
    /// order.
    pub fn post_sale_offers(&mut self, factuals: &Factuals) -> Vec<SellOffer> {
        if self.is_dead() {
            return Vec::new();
        }
        for row in self.property.values_mut() {
            row.reserve = 0.0;
        }
        self.reserve_inputs(factuals);
        let outputs = self.produced_goods(factuals);
        let share = factuals.config.firm.stock_offer_share;
        let mut offers = Vec::new();
        for (&good, row) in &self.property {
            if !outputs.contains(&good) {
                continue;
            }
            let available = row.available();
            let made = row.made.min(available);
            let amount = made + share * (available - made);
            if amount > 0.0 {
                offers.push(SellOffer {
                    seller: Actor::Firm(self.id),
                    good,
                    amount,
                });
            }
        }
        offers.sort_by_key(|offer| offer.good);
        offers
    }

    /// # Market Day
    ///
    /// Buys this firm's inputs from the open offers.
    ///
    /// `market` prices the goods. `factuals` supplies the processes.
    /// `offers` are the sell offers already posted, including this firm's.
    /// `pops` and the other firm slices are the sellers. This firm is not
    /// in either slice. `rng` draws the seller. Required inputs are bought
    /// before listed optionals. An optional is bought only when it keeps
    /// the iteration's margin. Time is not bought. An open line buys the
    /// iterations its coins can pay for, on top of the inputs already in
    /// stock. A line with a target buys the gap up to that target. A target
    /// of 0 buys nothing. This firm does not buy its own offer. A dead firm
    /// buys nothing. Returns the bids and the completed exchanges.
    pub(crate) fn market_day(
        &mut self,
        market: &Market,
        factuals: &Factuals,
        offers: &mut Vec<SellOffer>,
        pops: &mut [Pop],
        firms_before: &mut [Firm],
        firms_after: &mut [Firm],
        rng: &mut dyn RngCore,
    ) -> ShopBook {
        if self.is_dead() {
            return ShopBook::default();
        }
        shop_firm(
            self,
            firms_before,
            firms_after,
            pops,
            market,
            factuals,
            offers,
            rng,
        )
    }

    /// # Produce
    ///
    /// Runs this firm's production lines.
    ///
    /// `factuals` supplies the processes. `market` prices the run for the
    /// budget. No market prices every good at 1. The lines run in order.
    /// A missing process is skipped. `Some(0)` does not run. `None` runs
    /// as many iterations as the required inputs on hand allow. Each line
    /// records the iterations it was asked for and the iterations it
    /// finished. Process effects are not stored. A dead firm records a run
    /// of 0 on every line.
    pub fn produce(&mut self, factuals: &Factuals, market: Option<&Market>) {
        if self.is_dead() {
            // Yesterday's ask would still be on the line.
            for line in &mut self.production_line {
                line.attempted = 0.0;
                line.completed = 0.0;
            }
            return;
        }
        let lines: Vec<(usize, Option<f64>, Vec<usize>)> = self
            .production_line
            .iter()
            .map(|line| (line.process, line.target, line.inputs.clone()))
            .collect();
        for (index, (process_id, target, optional)) in lines.into_iter().enumerate() {
            let Some(process) = factuals.get_process(process_id) else {
                self.record_run(index, 0.0, 0.0);
                continue;
            };
            // The ask is measured before the run spends inputs.
            let attempted = self.planned_iterations(process, target);
            let run_target = match target {
                Some(n) => {
                    debug_assert!(n.is_finite() && n >= 0.0);
                    if n == 0.0 {
                        self.record_run(index, 0.0, 0.0);
                        continue;
                    }
                    Some(n)
                }
                None => None,
            };
            let inputs = self.line_inputs(process, &optional);
            let result = process.do_process(&inputs, run_target, factuals);
            self.apply_result(&result);
            self.note_run(&result, market);
            self.record_run(index, attempted, result.iterations);
        }
    }

    /// # Record Keeping
    ///
    /// Clears this firm's day counters.
    ///
    /// `factuals` supplies the world and is not read. `produced` is copied
    /// to `made`. Then `produced`, `consumed`, `spent`, `lost`, `reserve`,
    /// and `fresh` return to 0. `sold` stays. Quantity stays. The budget
    /// stays.
    pub fn record_keeping(&mut self, factuals: &Factuals) {
        let _ = factuals;
        for row in self.property.values_mut() {
            row.made = row.produced;
            row.produced = 0.0;
            row.consumed = 0.0;
            row.spent = 0.0;
            row.lost = 0.0;
            row.reserve = 0.0;
            row.fresh = 0.0;
        }
    }

    /// # Reserve Inputs
    ///
    /// Reserves production inputs up to each line's plan.
    ///
    /// `factuals` supplies the processes. A missing process is skipped.
    /// `Some(0)` reserves nothing. `Some(n)` reserves `n` iterations.
    /// `None` reserves as many iterations as the required inputs on hand
    /// allow. Every line's required inputs are reserved first. A factor is
    /// reserved once, at its listed amount. A listed optional is reserved
    /// after that, and only when it keeps the iteration's margin.
    fn reserve_inputs(&mut self, factuals: &Factuals) {
        let lines: Vec<(usize, Option<f64>, Vec<usize>)> = self
            .production_line
            .iter()
            .map(|line| (line.process, line.target, line.inputs.clone()))
            .collect();
        for (process_id, target, _) in &lines {
            let Some(process) = factuals.get_process(*process_id) else {
                continue;
            };
            let iterations = self.planned_iterations(process, *target);
            if iterations <= 0.0 {
                continue;
            }
            for input in &process.inputs {
                if input.is_optional() {
                    continue;
                }
                let need = if matches!(input.input_type, InputType::Factor) {
                    input.amount
                } else {
                    iterations * input.amount
                };
                self.reserve_up_to(input.good, need);
            }
        }
        for (process_id, target, listed) in &lines {
            let Some(process) = factuals.get_process(*process_id) else {
                continue;
            };
            let iterations = self.planned_iterations(process, *target);
            if iterations <= 0.0 {
                continue;
            }
            for good in listed {
                let Some(input) = process
                    .inputs
                    .iter()
                    .find(|input| input.good == *good && input.is_optional())
                else {
                    continue;
                };
                if !optional_keeps_value(process, input, None) {
                    continue;
                }
                let need = if matches!(input.input_type, InputType::Factor) {
                    input.amount
                } else {
                    iterations * input.amount
                };
                self.reserve_up_to(input.good, need);
            }
        }
    }

    /// # Planned Iterations
    ///
    /// How many times `process` runs for `target`.
    ///
    /// `Some(n)` is that count. `Some(0)` runs nothing. `None` is the most
    /// iterations the required inputs on hand can cover, or 0 when nothing
    /// limits the run. The morning reserve is not subtracted.
    fn planned_iterations(&self, process: &Process, target: Option<f64>) -> f64 {
        match target {
            Some(n) => {
                debug_assert!(n.is_finite() && n >= 0.0);
                n
            }
            None => self.iterations_on_hand(process),
        }
    }

    /// # Iterations On Hand
    ///
    /// The most iterations `process` can run from property on hand.
    ///
    /// Only required inputs that are not factors limit the run. Returns 0
    /// when no such input exists.
    fn iterations_on_hand(&self, process: &Process) -> f64 {
        let mut cap = None;
        for input in &process.inputs {
            if input.is_optional() || matches!(input.input_type, InputType::Factor) {
                continue;
            }
            debug_assert!(input.amount.is_finite() && input.amount > 0.0);
            let runs = self.on_hand(input.good) / input.amount;
            cap = Some(cap.map(|held: f64| held.min(runs)).unwrap_or(runs));
        }
        cap.unwrap_or(0.0)
    }

    /// # On Hand
    ///
    /// Quantity of `good`. A missing row is 0.
    fn on_hand(&self, good: usize) -> f64 {
        self.property.get(&good).map(|row| row.quantity).unwrap_or(0.0)
    }

    /// # Reserve Up To
    ///
    /// Reserves `need` of `good`, stopping at the unreserved quantity.
    ///
    /// A need of 0 reserves nothing. Returns the units reserved.
    fn reserve_up_to(&mut self, good: usize, need: f64) -> f64 {
        debug_assert!(need.is_finite() && need >= 0.0);
        if need == 0.0 {
            return 0.0;
        }
        let Some(row) = self.property.get_mut(&good) else {
            return 0.0;
        };
        let open = row.available();
        let take = need.min(open);
        if take > 0.0 {
            row.reserve += take;
        }
        take
    }

    /// # Record Run
    ///
    /// Stores one line's ask and its finish.
    ///
    /// `index` selects the line. `attempted` is the iterations it was asked
    /// for. `completed` is the iterations it finished.
    fn record_run(&mut self, index: usize, attempted: f64, completed: f64) {
        debug_assert!(attempted.is_finite() && attempted >= 0.0);
        debug_assert!(completed.is_finite() && completed >= 0.0);
        self.production_line[index].attempted = attempted;
        self.production_line[index].completed = completed;
    }

    /// # Line Inputs
    ///
    /// On-hand quantity for the inputs one line may draw.
    ///
    /// A required input is included when its `quantity` is positive. An
    /// optional input is included only when `optional` lists it and that
    /// quantity is positive. Any other good is left out.
    fn line_inputs(&self, process: &Process, optional: &[usize]) -> HashMap<usize, f64> {
        let mut inputs = HashMap::new();
        for input in &process.inputs {
            let listed = !input.is_optional() || optional.contains(&input.good);
            if !listed {
                continue;
            }
            let quantity = self.on_hand(input.good);
            if quantity > 0.0 {
                inputs.insert(input.good, quantity);
            }
        }
        inputs
    }

    /// # Note Run
    ///
    /// Adds `result` to this firm's budget.
    ///
    /// `market` prices the goods. No market prices every good at 1.
    /// [`FirmBudget::input_cost`] gains each spent input.
    /// [`FirmBudget::output_value`] gains the goods put out. Time is skipped.
    fn note_run(&mut self, result: &ProcessResult, market: Option<&Market>) {
        for (&good, &spent) in &result.spent {
            if good == TIME || spent <= 0.0 {
                continue;
            }
            self.budget.input_cost += spent * market_price(market, good);
        }
        for (&good, &change) in &result.changes {
            if good == TIME {
                continue;
            }
            let spent = result.spent.get(&good).copied().unwrap_or(0.0);
            let made = change + spent;
            if made > 0.0 {
                self.budget.output_value += made * market_price(market, good);
            }
        }
    }

    /// # Apply Result
    ///
    /// Writes one process result onto this firm's property.
    ///
    /// Units destroyed are removed from `quantity` and added to `consumed`.
    /// Capital leaves `quantity` and is added to `used`. Aged units go
    /// before fresh. `reserve` falls by the units that left. Units put out
    /// are added to `quantity`, `produced`, and `fresh`. Effects are not
    /// stored.
    fn apply_result(&mut self, result: &ProcessResult) {
        let mut goods = Vec::new();
        for good in result
            .changes
            .keys()
            .chain(result.spent.keys())
            .chain(result.used_inputs.keys())
        {
            if !goods.contains(good) {
                goods.push(*good);
            }
        }
        for good in goods {
            let change = result.changes.get(&good).copied().unwrap_or(0.0);
            let spent = result.spent.get(&good).copied().unwrap_or(0.0);
            let used = result.used_inputs.get(&good).copied().unwrap_or(0.0);
            debug_assert!(change.is_finite() && spent.is_finite() && used.is_finite());
            debug_assert!(spent >= 0.0 && used >= 0.0);
            // `changes` nets output against destruction. Putting the output
            // back on its own means only the gross destruction is removed.
            let made = change + spent;
            debug_assert!(made >= 0.0);
            let removed = spent + used;
            if removed == 0.0 && made <= 0.0 {
                continue;
            }
            {
                let row = self.property.entry(good).or_default();
                row.spend_aged_first(removed);
                if spent > 0.0 {
                    row.quantity -= spent;
                    row.consumed += spent;
                }
                if used > 0.0 {
                    row.quantity -= used;
                    row.used += used;
                }
                if removed > 0.0 {
                    row.reserve = (row.reserve - removed).max(0.0);
                }
            }
            if made > 0.0 {
                self.add_made(good, made);
            }
        }
    }

    /// # Add Made
    ///
    /// Adds `amount` of `good` that this firm just produced.
    ///
    /// `quantity`, `produced`, and `fresh` each gain `amount`. A zero
    /// amount changes nothing.
    fn add_made(&mut self, good: usize, amount: f64) {
        debug_assert!(amount.is_finite() && amount >= 0.0);
        if amount == 0.0 {
            return;
        }
        let row = self.property.entry(good).or_default();
        row.quantity += amount;
        row.produced += amount;
        row.fresh += amount;
    }

    /// # Planning
    ///
    /// Sets the next target on this firm's first production line.
    ///
    /// `factuals` supplies the process. `market` prices a run the firm
    /// still has to buy. No market cannot buy a missing input. Only the
    /// first line is planned. A later line keeps its target. A missing
    /// process is left as it is. A dead firm stays dead. A firm that cannot
    /// fund one run of that line becomes dead and keeps its target.
    ///
    /// An open line plans from the iterations it was asked to run. The line
    /// rises by one when the day was profitable, the run finished, and the
    /// firm sold. It holds when one or two of those are true. It falls by
    /// one when none are, and it stops at [`FIRM_MIN_TARGET`]. Profitable
    /// means [`FirmBudget::output_value`] cleared [`FIRM_MIN_MARGIN`] on
    /// [`FirmBudget::input_cost`].
    ///
    /// Then any line, the first included, whose process puts out a good
    /// that would compete with subsistence on `market`, see
    /// [`Market::competes_with_subsistence`], is set to `Some(0)`.
    pub fn planning(&mut self, factuals: &Factuals, market: Option<&Market>) {
        if self.is_dead() {
            return;
        }
        let Some(process_id) = self.production_line.first().map(|line| line.process) else {
            return;
        };
        let Some(process) = factuals.get_process(process_id) else {
            return;
        };
        if !can_fund_one(self, process, market, factuals) {
            self.dead = true;
            return;
        }
        let marks = self.day_marks();
        let current = match self.production_line[0].target {
            Some(n) => {
                debug_assert!(n.is_finite() && n >= 0.0);
                n
            }
            None => self.production_line[0].attempted,
        };
        debug_assert!(current.is_finite() && current >= 0.0);
        let next = if marks == 3 {
            current + 1.0
        } else if marks == 0 {
            (current - 1.0).max(FIRM_MIN_TARGET)
        } else {
            current
        };
        self.production_line[0].target = Some(next);
        let Some(market) = market else {
            return;
        };
        for line in &mut self.production_line {
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            let competes = process
                .outputs
                .iter()
                .any(|output| market.competes_with_subsistence(factuals, output.good));
            if competes {
                line.target = Some(0.0);
            }
        }
    }

    /// # Day Marks
    ///
    /// How many of this day's three planning marks the first line earned.
    ///
    /// Profitable is a positive [`FirmBudget::output_value`] at least
    /// [`FIRM_MIN_MARGIN`] above [`FirmBudget::input_cost`]. Finished is
    /// [`ProductionLine::completed`] covering the ask past
    /// [`FIRM_FINISH_GRACE`] when that ask is positive. Selling is
    /// [`FirmBudget::sold_value`] covering the higher of the input cost and
    /// [`FIRM_SELL_SHARE`] of the output value. A day with neither bar earns
    /// no selling mark. A firm with no line earns none. Returns that count,
    /// from 0 to 3.
    fn day_marks(&self) -> u8 {
        let Some(line) = self.production_line.first() else {
            return 0;
        };
        let mut marks = 0;
        let cost = self.budget.input_cost;
        let output = self.budget.output_value;
        if output > 0.0 && output + 1e-9 >= cost * (1.0 + FIRM_MIN_MARGIN) {
            marks += 1;
        }
        if line.attempted > 0.0
            && line.completed + 1e-9 >= line.attempted * (1.0 - FIRM_FINISH_GRACE)
        {
            marks += 1;
        }
        let bar = cost.max(output * FIRM_SELL_SHARE);
        if bar > 1e-9 && self.budget.sold_value + 1e-9 >= bar {
            marks += 1;
        }
        marks
    }
}

impl FirmPRow {
    /// # New
    ///
    /// A zero ledger. Same value as [`Self::default`].
    pub fn new() -> Self {
        Self::default()
    }

    /// # Holding
    ///
    /// Units held in this row: `quantity` plus capital tied up in `used`.
    /// Returns that sum.
    pub fn holding(&self) -> f64 {
        self.quantity + self.used
    }

    /// # Available
    ///
    /// On-hand units not earmarked.
    ///
    /// `quantity` and `reserve` are finite, and `quantity` is at least
    /// `reserve`. A deficit under `1e-6` is treated as nothing left.
    pub fn available(&self) -> f64 {
        debug_assert!(self.quantity.is_finite() && self.reserve.is_finite());
        let left = self.quantity - self.reserve;
        if left < 0.0 {
            debug_assert!(
                left > -1e-6,
                "quantity {} reserve {}",
                self.quantity,
                self.reserve
            );
            return 0.0;
        }
        left
    }

    /// # Spend Aged First
    ///
    /// Removes `units` from the pile, spending old stock before fresh.
    ///
    /// A zero amount changes nothing. `fresh` is a portion of `quantity`,
    /// and `units` is no greater than `quantity`.
    pub fn spend_aged_first(&mut self, units: f64) {
        debug_assert!(units.is_finite() && units >= 0.0);
        debug_assert!(self.quantity.is_finite() && self.quantity >= 0.0);
        debug_assert!(self.fresh.is_finite());
        self.keep_fresh_inside();
        debug_assert!(self.fresh >= 0.0);
        debug_assert!(self.fresh <= self.quantity);
        debug_assert!(units <= self.quantity);
        if units == 0.0 {
            return;
        }
        let aged = self.quantity - self.fresh;
        let from_fresh = (units - aged).max(0.0);
        self.fresh -= from_fresh;
        self.keep_fresh_inside();
    }

    /// # Keep Fresh Inside
    ///
    /// Pulls `fresh` back into `0..=quantity` when it sits a scrap outside.
    ///
    /// A spend can leave `fresh` just under 0, or just over `quantity` once
    /// the units have left the pile. A gap of `1e-6` or more is left in
    /// place. A smaller gap is stored on the bound it crossed.
    pub fn keep_fresh_inside(&mut self) {
        debug_assert!(self.quantity.is_finite() && self.quantity >= 0.0);
        debug_assert!(self.fresh.is_finite());
        if self.fresh < 0.0 {
            debug_assert!(
                self.fresh > -1e-6,
                "fresh {} quantity {}",
                self.fresh,
                self.quantity
            );
            self.fresh = 0.0;
        } else if self.fresh > self.quantity {
            debug_assert!(
                self.fresh - self.quantity < 1e-6,
                "fresh {} quantity {}",
                self.fresh,
                self.quantity
            );
            self.fresh = self.quantity;
        }
    }

    /// # With Quantity
    ///
    /// Sets quantity. Must be `>= 0.0`.
    pub fn with_quantity(mut self, quantity: f64) -> Self {
        debug_assert!(quantity >= 0.0, "quantity must be >= 0.0");
        self.quantity = quantity;
        self
    }

    /// # With Reserve
    ///
    /// Sets the reserve. Must be `>= 0.0`.
    pub fn with_reserve(mut self, reserve: f64) -> Self {
        debug_assert!(reserve >= 0.0, "reserve must be >= 0.0");
        self.reserve = reserve;
        self
    }

    /// # Fresh Share
    ///
    /// Fraction of `quantity` that is fresh.
    ///
    /// `fresh` is a portion of `quantity`. Returns `0` when `quantity` is `0`.
    pub fn fresh_share(&self) -> f64 {
        debug_assert!(self.quantity.is_finite() && self.quantity >= 0.0);
        debug_assert!(self.fresh.is_finite() && self.fresh >= 0.0);
        debug_assert!(self.fresh <= self.quantity);
        if self.quantity == 0.0 {
            return 0.0;
        }
        self.fresh / self.quantity
    }
}

#[cfg(test)]
mod day_should {
    use std::collections::{HashMap, HashSet};

    use super::{Firm, FirmPRow, ProductionLine};
    use crate::game::market::Market;
    use crate::game::factuals::Factuals;
    use crate::game::good::{Good, GoodTag};
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput};
    
    const INPUT: usize = 2;
    const OUTPUT: usize = 3;
    const COIN: usize = 1;

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }

    fn good(id: usize, decay_rate: f64) -> Good {
        Good {
            id,
            name: format!("good {id}"),
            class: None,
            decay_rate,
            decay_result: HashMap::new(),
            mass: 1.0,
            volume: 1.0,
            tags: HashSet::new(),
            categories: vec![],
        }
    }

    fn mill(output: f64) -> Process {
        Process::new(1, "mill", 0)
            .with_input(ProcessInput::new(INPUT, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(OUTPUT, output, true))
    }

    fn firm_with(target: Option<f64>, input: f64) -> (Firm, Factuals) {
        let mut factuals = Factuals::new();
        factuals.goods.insert(INPUT, good(INPUT, 0.0));
        factuals.goods.insert(OUTPUT, good(OUTPUT, 0.0));
        factuals.processes.insert(1, mill(3.0));
        let mut firm = Firm::new(1, "mill".into());
        if input > 0.0 {
            firm.property.insert(INPUT, FirmPRow::new().with_quantity(input));
        }
        firm.production_line.push(ProductionLine {
            process: 1,
            target,
            inputs: vec![],
            attempted: 0.0,
            completed: 0.0,
        });
        (firm, factuals)
    }

    #[test]
    fn a_target_records_the_ask_and_the_finish() {
        let (mut firm, factuals) = firm_with(Some(2.0), 5.0);
        firm.produce(&factuals, None);
        let line = &firm.production_line[0];
        near(line.attempted, 2.0);
        near(line.completed, 2.0);
        near(firm.property[&INPUT].quantity, 3.0);
        near(firm.property[&INPUT].consumed, 2.0);
        near(firm.property[&OUTPUT].quantity, 6.0);
        near(firm.property[&OUTPUT].produced, 6.0);
        near(firm.property[&OUTPUT].fresh, 6.0);
    }

    #[test]
    fn a_short_stock_still_records_the_ask() {
        let (mut firm, factuals) = firm_with(Some(4.0), 1.0);
        firm.produce(&factuals, None);
        near(firm.production_line[0].attempted, 4.0);
        near(firm.production_line[0].completed, 1.0);
        near(firm.property[&INPUT].consumed, 1.0);
        near(firm.property[&OUTPUT].produced, 3.0);
    }

    #[test]
    fn an_open_line_runs_the_stock() {
        let (mut firm, factuals) = firm_with(None, 3.5);
        firm.produce(&factuals, None);
        near(firm.production_line[0].attempted, 3.5);
        near(firm.production_line[0].completed, 3.5);
        near(firm.property[&OUTPUT].quantity, 10.5);
    }

    #[test]
    fn a_zero_target_and_a_missing_process_record_nothing() {
        let (mut firm, factuals) = firm_with(Some(0.0), 5.0);
        firm.production_line.push(ProductionLine {
            process: 99,
            target: Some(3.0),
            inputs: vec![],
            attempted: 4.0,
            completed: 4.0,
        });
        firm.produce(&factuals, None);
        near(firm.production_line[0].attempted, 0.0);
        near(firm.production_line[0].completed, 0.0);
        near(firm.property[&INPUT].quantity, 5.0);
        near(firm.property[&INPUT].consumed, 0.0);
        near(firm.production_line[1].attempted, 0.0);
        near(firm.production_line[1].completed, 0.0);
        assert!(firm.property.get(&OUTPUT).is_none());
    }

    #[test]
    fn capital_leaves_the_pile_until_decay() {
        let mut factuals = Factuals::new();
        factuals.goods.insert(INPUT, good(INPUT, 0.0));
        factuals.goods.insert(OUTPUT, good(OUTPUT, 0.0));
        const TOOL: usize = 9;
        factuals.goods.insert(TOOL, good(TOOL, 0.0));
        factuals.processes.insert(
            1,
            mill(3.0).with_input(ProcessInput::new(TOOL, 1.0, true, InputType::Capital, false)),
        );
        let mut firm = Firm::new(1, "mill".into());
        firm.property.insert(INPUT, FirmPRow::new().with_quantity(5.0));
        firm.property.insert(TOOL, FirmPRow::new().with_quantity(1.0));
        firm.production_line.push(ProductionLine {
            process: 1,
            target: None,
            inputs: vec![],
            attempted: 0.0,
            completed: 0.0,
        });
        firm.produce(&factuals, None);
        near(firm.production_line[0].completed, 1.0);
        near(firm.property[&TOOL].quantity, 0.0);
        near(firm.property[&TOOL].used, 1.0);
        firm.decay_goods(&factuals);
        near(firm.property[&TOOL].quantity, 1.0);
        near(firm.property[&TOOL].used, 0.0);
        near(firm.property[&TOOL].lost, 0.0);
    }

    #[test]
    fn aging_stock_rots_and_fresh_stays() {
        let mut factuals = Factuals::new();
        factuals.goods.insert(1, good(1, 0.25));
        let mut firm = Firm::new(1, "mill".into());
        firm.property.insert(
            1,
            FirmPRow {
                quantity: 10.0,
                fresh: 6.0,
                consumed: 3.0,
                reserve: 2.0,
                ..FirmPRow::default()
            },
        );
        firm.decay_goods(&factuals);
        let row = &firm.property[&1];
        near(row.quantity, 9.0);
        near(row.fresh, 6.0);
        near(row.lost, 1.0);
        near(row.consumed, 3.0);
        near(row.reserve, 2.0);
    }

    #[test]
    fn an_owned_exposure_good_stays_whole() {
        let mut kind = good(1, 1.0);
        kind.tags.insert(GoodTag::Exposure);
        let mut factuals = Factuals::new();
        factuals.goods.insert(1, kind);
        let mut firm = Firm::new(1, "mill".into());
        firm.property.insert(1, FirmPRow::new().with_quantity(10.0));
        firm.decay_goods(&factuals);
        near(firm.property[&1].quantity, 10.0);
        near(firm.property[&1].lost, 0.0);
    }

    #[test]
    fn rot_credits_the_decay_result_without_marking_it_fresh() {
        let mut kind = good(1, 1.0);
        kind.decay_result.insert(2, 0.5);
        let mut factuals = Factuals::new();
        factuals.goods.insert(1, kind);
        let mut firm = Firm::new(1, "mill".into());
        firm.property.insert(1, FirmPRow::new().with_quantity(4.0));
        firm.decay_goods(&factuals);
        near(firm.property[&1].quantity, 0.0);
        near(firm.property[&1].lost, 4.0);
        near(firm.property[&2].quantity, 2.0);
        near(firm.property[&2].fresh, 0.0);
        near(firm.property[&2].produced, 0.0);
    }

    #[test]
    fn offers_reserve_inputs_and_list_only_output() {
        let mut factuals = Factuals::new();
        factuals.goods.insert(INPUT, good(INPUT, 0.0));
        factuals.goods.insert(OUTPUT, good(OUTPUT, 0.0));
        factuals.goods.insert(COIN, good(COIN, 0.0));
        factuals.processes.insert(1, mill(3.0));
        let mut firm = Firm::new(1, "mill".into());
        firm.property.insert(INPUT, FirmPRow::new().with_quantity(4.0));
        firm.property.insert(OUTPUT, FirmPRow::new().with_quantity(5.0));
        firm.property.insert(COIN, FirmPRow::new().with_quantity(2.0));
        firm.production_line.push(ProductionLine {
            process: 1,
            target: Some(2.0),
            inputs: vec![],
            attempted: 0.0,
            completed: 0.0,
        });
        let offers = firm.post_sale_offers(&factuals);
        assert_eq!(offers.len(), 1);
        assert_eq!(offers[0].seller, crate::game::actor::Actor::Firm(1));
        assert_eq!(offers[0].good, OUTPUT);
        // Nothing was made yesterday, so a tenth of the 5 held is offered.
        near(offers[0].amount, 0.5);
        near(firm.property[&INPUT].reserve, 2.0);
        near(firm.property[&OUTPUT].reserve, 0.0);
        near(firm.property[&COIN].reserve, 0.0);

        firm.dead = true;
        assert!(firm.post_sale_offers(&factuals).is_empty());
    }

    #[test]
    fn offers_the_last_output_and_a_share_of_held_stock() {
        // Three made yesterday are offered, and a tenth of the other two.
        let mut factuals = Factuals::new();
        factuals.goods.insert(INPUT, good(INPUT, 0.0));
        factuals.goods.insert(OUTPUT, good(OUTPUT, 0.0));
        factuals.processes.insert(1, mill(3.0));
        let mut firm = Firm::new(1, "mill".into());
        firm.property.insert(
            OUTPUT,
            FirmPRow {
                quantity: 5.0,
                made: 3.0,
                ..FirmPRow::default()
            },
        );
        firm.production_line.push(ProductionLine {
            process: 1,
            target: Some(0.0),
            inputs: vec![],
            attempted: 0.0,
            completed: 0.0,
        });
        let offers = firm.post_sale_offers(&factuals);
        assert_eq!(offers.len(), 1);
        near(offers[0].amount, 3.2);

        // More made than is on hand offers only what is on hand.
        firm.property.get_mut(&OUTPUT).unwrap().made = 9.0;
        near(firm.post_sale_offers(&factuals)[0].amount, 5.0);
    }

    #[test]
    fn record_keeping_keeps_the_days_output_as_made() {
        let mut firm = Firm::new(1, "mill".into());
        firm.property.insert(
            OUTPUT,
            FirmPRow {
                quantity: 5.0,
                produced: 3.0,
                made: 1.0,
                ..FirmPRow::default()
            },
        );
        firm.record_keeping(&Factuals::new());
        near(firm.property[&OUTPUT].made, 3.0);
        near(firm.property[&OUTPUT].produced, 0.0);
    }

    #[test]
    fn a_dead_firm_records_no_run() {
        let (mut firm, factuals) = firm_with(Some(2.0), 5.0);
        firm.production_line[0].attempted = 2.0;
        firm.production_line[0].completed = 2.0;
        firm.dead = true;
        firm.produce(&factuals, None);
        near(firm.production_line[0].attempted, 0.0);
        near(firm.production_line[0].completed, 0.0);
        near(firm.property[&INPUT].quantity, 5.0);
    }

    #[test]
    fn record_keeping_clears_the_day_and_leaves_stock_and_sold() {
        let mut firm = Firm::new(1, "mill".into());
        firm.property.insert(
            1,
            FirmPRow {
                quantity: 4.0,
                reserve: 1.0,
                fresh: 2.0,
                lost: 0.5,
                consumed: 1.0,
                spent: 0.25,
                sold: 3.0,
                produced: 6.0,
                ..FirmPRow::default()
            },
        );
        firm.budget.input_cost = 4.0;
        firm.budget.sold_value = 9.0;
        firm.record_keeping(&Factuals::new());
        let row = &firm.property[&1];
        near(row.quantity, 4.0);
        near(row.sold, 3.0);
        near(row.reserve, 0.0);
        near(row.fresh, 0.0);
        near(row.lost, 0.0);
        near(row.consumed, 0.0);
        near(row.spent, 0.0);
        near(row.produced, 0.0);
        near(firm.budget.input_cost, 4.0);
        near(firm.budget.sold_value, 9.0);
    }

    #[test]
    fn a_run_prices_its_inputs_and_its_output() {
        let (mut firm, factuals) = firm_with(Some(2.0), 5.0);
        let mut market = Market::new(1);
        market.set_amv(INPUT, 4.0);
        market.set_amv(OUTPUT, 3.0);
        firm.produce(&factuals, Some(&market));
        near(firm.budget.input_cost, 8.0);
        near(firm.budget.output_value, 18.0);
    }
}

#[cfg(test)]
mod plan_should {
    
    use super::{Firm, FirmPRow, ProductionLine};
    use crate::game::factuals::Factuals;
    use crate::game::market::Market;
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput};

    const INPUT: usize = 2;
    const OUTPUT: usize = 3;

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }

    fn line(target: Option<f64>) -> ProductionLine {
        ProductionLine {
            process: 1,
            target,
            inputs: vec![],
            attempted: 0.0,
            completed: 0.0,
        }
    }

    fn workshop(output: f64, input_amount: f64, target: Option<f64>) -> (Firm, Factuals) {
        let mut factuals = Factuals::new();
        factuals.processes.insert(
            1,
            Process::new(1, "mill", 0)
                .with_input(ProcessInput::new(INPUT, input_amount, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(OUTPUT, output, true)),
        );
        let mut firm = Firm::new(1, "mill".into());
        firm.property
            .insert(INPUT, FirmPRow::new().with_quantity(input_amount));
        firm.production_line.push(line(target));
        (firm, factuals)
    }

    fn ran(firm: &mut Firm, attempted: f64, completed: f64, input: f64, output: f64, sold: f64) {
        let line = &mut firm.production_line[0];
        line.attempted = attempted;
        line.completed = completed;
        firm.budget.input_cost = input;
        firm.budget.output_value = output;
        firm.budget.sold_value = sold;
    }

    #[test]
    fn three_marks_add_one_and_two_marks_hold() {
        let (mut firm, factuals) = workshop(2.0, 1.0, Some(4.0));
        ran(&mut firm, 4.0, 4.0, 10.0, 10.5, 10.0);
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 5.0);
        assert!(!firm.is_dead());

        let (mut firm, factuals) = workshop(2.0, 1.0, Some(4.0));
        ran(&mut firm, 4.0, 4.0, 10.0, 10.5, 0.0);
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 4.0);

        let (mut firm, factuals) = workshop(2.0, 1.0, Some(4.0));
        ran(&mut firm, 4.0, 0.5, 10.0, 10.5, 10.0);
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 4.0);

        let (mut firm, factuals) = workshop(2.0, 1.0, Some(4.0));
        ran(&mut firm, 4.0, 4.0, 10.0, 10.4, 10.0);
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 4.0);
    }

    #[test]
    fn no_marks_step_down_and_stop_at_one() {
        let (mut firm, factuals) = workshop(2.0, 1.0, Some(4.0));
        ran(&mut firm, 4.0, 0.0, 0.0, 0.0, 0.0);
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 3.0);

        let (mut firm, factuals) = workshop(2.0, 1.0, Some(1.0));
        ran(&mut firm, 1.0, 0.0, 0.0, 0.0, 0.0);
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 1.0);

        let (mut firm, factuals) = workshop(2.0, 1.0, Some(0.0));
        ran(&mut firm, 0.0, 0.0, 0.0, 0.0, 0.0);
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 1.0);
    }

    #[test]
    fn a_near_finish_counts_and_a_short_one_does_not() {
        let (mut firm, factuals) = workshop(2.0, 1.0, Some(4.0));
        ran(&mut firm, 4.0, 3.6, 10.0, 10.5, 10.0);
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 5.0);

        let (mut firm, factuals) = workshop(2.0, 1.0, Some(4.0));
        ran(&mut firm, 4.0, 3.5, 10.0, 10.5, 10.0);
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 4.0);
    }

    #[test]
    fn a_sale_has_to_clear_the_higher_bar() {
        let (mut firm, factuals) = workshop(2.0, 1.0, Some(4.0));
        ran(&mut firm, 4.0, 4.0, 4.0, 10.0, 8.0);
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 5.0);

        let (mut firm, factuals) = workshop(2.0, 1.0, Some(4.0));
        ran(&mut firm, 4.0, 4.0, 4.0, 10.0, 7.0);
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 4.0);

        let (mut firm, factuals) = workshop(2.0, 1.0, Some(4.0));
        ran(&mut firm, 4.0, 4.0, 12.0, 13.0, 12.0);
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 5.0);

        let (mut firm, factuals) = workshop(2.0, 1.0, Some(4.0));
        ran(&mut firm, 4.0, 4.0, 12.0, 13.0, 11.0);
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 4.0);

        let (mut firm, factuals) = workshop(2.0, 1.0, Some(4.0));
        ran(&mut firm, 4.0, 0.0, 0.0, 0.0, 5.0);
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 3.0);
    }

    #[test]
    fn an_open_line_plans_from_the_ask() {
        let (mut firm, factuals) = workshop(2.0, 1.0, None);
        ran(&mut firm, 1.0, 1.0, 1.0, 2.0, 0.0);
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 1.0);

        let (mut firm, factuals) = workshop(2.0, 1.0, None);
        ran(&mut firm, 2.0, 2.0, 1.0, 2.0, 3.0);
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 3.0);
    }

    #[test]
    fn one_mark_holds() {
        let (mut firm, factuals) = workshop(2.0, 1.0, Some(4.0));
        ran(&mut firm, 4.0, 0.0, 10.0, 10.0, 10.0);
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 4.0);
    }

    #[test]
    fn a_worn_tool_still_funds_the_run_and_a_missing_one_does_not() {
        let mut factuals = Factuals::new();
        factuals.processes.insert(
            1,
            Process::new(1, "mill", 0)
                .with_input(ProcessInput::new(0, 5.0, true, InputType::Destroyed, false))
                .with_input(ProcessInput::new(INPUT, 1.0, true, InputType::Destroyed, false))
                .with_input(ProcessInput::new(9, 1.0, true, InputType::Capital, false))
                .with_output(ProcessOutput::new(OUTPUT, 2.0, true)),
        );
        let mut firm = Firm::new(1, "mill".into());
        firm.property
            .insert(INPUT, FirmPRow::new().with_quantity(1.0));
        firm.property
            .insert(9, FirmPRow::new().with_quantity(0.5));
        firm.production_line.push(line(Some(3.0)));
        firm.planning(&factuals, None);
        assert!(!firm.is_dead());
        near(firm.production_line[0].target.unwrap(), 2.0);

        let mut broke = Firm::new(2, "broke".into());
        broke.property
            .insert(INPUT, FirmPRow::new().with_quantity(1.0));
        broke.production_line.push(line(Some(3.0)));
        broke.planning(&factuals, None);
        assert!(broke.is_dead());
        near(broke.production_line[0].target.unwrap(), 3.0);
    }

    #[test]
    fn idle_a_line_that_competes_with_subsistence_until_the_good_runs_short() {
        let mut market = Market::new(1);
        market.currencies.push(1);
        market.goods.insert(OUTPUT, crate::game::market::MarketGood {
            subsisted: 3.0,
            eaten: 4.0,
            ..crate::game::market::MarketGood::new()
        });
        let (mut firm, factuals) = workshop(2.0, 1.0, Some(2.0));
        firm.property.insert(1, FirmPRow::new().with_quantity(10.0));
        ran(&mut firm, 2.0, 2.0, 1.0, 2.0, 2.0);
        firm.planning(&factuals, Some(&market));
        assert!(!firm.is_dead());
        near(firm.production_line[0].target.unwrap(), 0.0);

        // Short five days in a row: the firm plans as usual again.
        market.goods.get_mut(&OUTPUT).unwrap().short_days = 5;
        let (mut firm, factuals) = workshop(2.0, 1.0, Some(2.0));
        firm.property.insert(1, FirmPRow::new().with_quantity(10.0));
        ran(&mut firm, 2.0, 2.0, 1.0, 2.0, 2.0);
        firm.planning(&factuals, Some(&market));
        near(firm.production_line[0].target.unwrap(), 3.0);
    }

    #[test]
    fn coins_fund_one_run_and_a_short_purse_does_not() {
        let mut market = Market::new(1);
        market.currencies.push(1);
        let (mut firm, factuals) = workshop(2.0, 1.0, Some(2.0));
        firm.property.remove(&INPUT);
        firm.property
            .insert(1, FirmPRow::new().with_quantity(1.0));
        ran(&mut firm, 2.0, 2.0, 1.0, 2.0, 2.0);
        firm.planning(&factuals, Some(&market));
        assert!(!firm.is_dead());
        near(firm.production_line[0].target.unwrap(), 3.0);

        let (mut firm, factuals) = workshop(2.0, 1.0, Some(2.0));
        firm.property.remove(&INPUT);
        firm.property
            .insert(1, FirmPRow::new().with_quantity(0.4));
        firm.planning(&factuals, Some(&market));
        assert!(firm.is_dead());
        near(firm.production_line[0].target.unwrap(), 2.0);
    }

    #[test]
    fn a_dead_firm_and_a_missing_process_keep_their_targets() {
        let (mut firm, factuals) = workshop(2.0, 1.0, Some(4.0));
        ran(&mut firm, 4.0, 4.0, 1.0, 2.0, 2.0);
        firm.dead = true;
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 4.0);
        assert!(firm.is_dead());

        firm.production_line.push(ProductionLine {
            process: 1,
            target: Some(7.0),
            inputs: vec![],
            attempted: 0.0,
            completed: 0.0,
        });
        firm.dead = false;
        firm.planning(&factuals, None);
        near(firm.production_line[0].target.unwrap(), 5.0);
        near(firm.production_line[1].target.unwrap(), 7.0);

        let mut idle = Firm::new(2, "idle".into());
        idle.production_line.push(ProductionLine {
            process: 99,
            target: Some(3.0),
            inputs: vec![],
            attempted: 0.0,
            completed: 0.0,
        });
        idle.planning(&factuals, None);
        near(idle.production_line[0].target.unwrap(), 3.0);
        assert!(!idle.is_dead());

        let mut empty = Firm::new(3, "empty".into());
        empty.planning(&factuals, None);
        assert!(empty.production_line.is_empty());
        assert!(!empty.is_dead());
    }

    #[test]
    fn no_stock_and_no_market_is_dead() {
        let mut factuals = Factuals::new();
        factuals.processes.insert(
            1,
            Process::new(1, "mill", 0)
                .with_input(ProcessInput::new(INPUT, 1.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(OUTPUT, 2.0, true)),
        );
        let mut firm = Firm::new(1, "mill".into());
        firm.production_line.push(line(Some(4.0)));
        firm.planning(&factuals, None);
        assert!(firm.is_dead());
        near(firm.production_line[0].target.unwrap(), 4.0);
    }
}
