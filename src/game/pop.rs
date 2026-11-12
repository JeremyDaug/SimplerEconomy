#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};

use crate::game::effects::DesireEffect;
use crate::game::factuals::Factuals;
use crate::game::good::{GoodTag, TIME};
use crate::game::market::{Market, MarketHistory};
use crate::game::process::{InputType, Process, ProcessResult};
use crate::game::scalingfactor::ScalingFactor;
use crate::game::sentiment::{Sentiment, SentimentKind, SentimentMod};
use crate::game::{
    desire::{Desire, DesireTargetType},
    household::Household,
    job::{Job, JobLine},
};

pub use crate::game::effects::PopEffect;
pub use crate::game::pop_property::{DemoRow, PopPRow, PopRecords};

/// A population slice: one household block, its goods, and its desires.
#[derive(Debug, Clone)]
pub struct Pop {
    pub id: usize,

    /// The amount of it's day it spends on subsistence.
    pub subsistence_rate: f64,
    /// The type of subsistence it uses. Modifies its benefits and can add requirements as well.
    pub subsistence_type: SubsistenceType,
    /// This pop's cottage work. Each pop keeps its own plan.
    pub job: Job,

    /// Time spent hauling yesterday.
    ///
    /// The market day replaces this and [`Self::transport_purchased`].
    /// [`Self::planning`] reads them and does not clear them.
    pub transport_spent: f64,
    /// Transport cover bought yesterday, in the same units as [`Self::transport_spent`].
    ///
    /// One unit of cover stands in for one unit of time spent.
    pub transport_purchased: f64,
    /// Time consumed yesterday. [`Self::record_keeping`] writes it.
    pub time_consumed: f64,
    /// Desire time the last plan kept.
    held_desire_time: f64,
    /// Haul time the last plan kept.
    held_transport_time: f64,

    /// Goods on hand.
    pub property: HashMap<usize, PopPRow>,
    /// The currency savings of the pop. Anything in here is removed from [`Self::property`].
    /// It should not be counted for trade or use, and should be returned to property 
    /// after market phase so it can still get used/decayed properly.
    savings: HashMap<usize, f64>,
    /// When storing multiple currencies, this is it's value in the unit of account,
    /// defined by the market at day start.
    effective_savings: f64,

    /// Desires grouped by tier: 0 basic, 1 common, 2 luxury.
    pub desires: Vec<Vec<Desire>>,
    /// Where a pop is in satisfying it's desires.
    desire_cursor: Option<SatisfyCursor>,

    /// Species and culture for this slice.
    pub demographics: DemoRow,

    /// People the plans and desire amounts are scaled to.
    ///
    /// [`Self::growth_phase`] records the pre-growth population.
    /// [`Self::rescale`] reads it, then stores the current population.
    pub pre_growth_population: f64,

    /// Share of the market population ceiling, in people. `None` leaves this pop out.
    pub population_cap: Option<f64>,

    /// Net households gained from internal migration this day.
    ///
    /// Negative is households that left. [`Market::internal_migration`]
    /// clears it at the start of that pass. A pop that stays is `0`.
    /// This is the household count, not people.
    pub migration: f64,

    /// Same-day effects waiting for growth or decay.
    pub stored_effects: Vec<PopEffect>,

    /// Mood shares. [`Self::growth_phase`] scales births per woman from these.
    pub sentiment: Sentiment,
    /// Recent standards of living, oldest first.
    ///
    /// [`Self::update_sentiments`] appends today's reading and keeps
    /// [`SOL_AVERAGE_DAYS`] of them.
    sol_samples: VecDeque<f64>,

    /// Day-end records. The struct is a placeholder.
    pub records: PopRecords,
}

/// Units of one good a pop puts up for sale.
///
/// [`Pop::split_goods_sale_reserve`] returns these. `pop` is the seller.
#[derive(Debug, Clone, Copy)]
pub struct SellOffer {
    /// Pop selling the good.
    pub pop: usize,
    /// Good id.
    pub good: usize,
    /// Units offered.
    pub amount: f64,
}

/// # Migrant Group
///
/// Households and goods taken off one pop by [`Pop::take_migrants`].
#[derive(Debug, Clone)]
pub(crate) struct MigrantGroup {
    /// Households in this slice.
    pub households: f64,
    /// Age, sex, labor, and partnership averages of the pop they left.
    pub shape: Household,
    /// Their portion of property taken. Just quantity. 
    /// Splitting is decided early in the day so only this value should be needed.
    pub quantity: HashMap<usize, f64>,
}

/// Bookmark for one satisfaction walk.
///
/// `target_index` is into `desire.target` as stored, the target the walk
/// stopped on. `target_start_satisfaction` is the desire's satisfaction
/// when that target began receiving goods, so a resume does not spend its cap twice.
#[derive(Debug, Clone, Copy)]
struct SatisfyCursor {
    tier: usize,
    desire_index: usize,
    target_index: usize,
    target_start_satisfaction: Option<f64>,
    iter_target: f64,
}

/// Result of reserving goods for one desire.
enum SatisfyProgress {
    Done,
    Blocked {
        target_index: usize,
        target_start_satisfaction: f64,
    },
}

/// # Subsistence Type
/// 
/// The subsistence type the pop has access to, changing it's outputs, requriments and rules.
/// 
/// This may be replaced by a more flexible system later.
#[derive(Debug, Clone, Copy)]
pub enum SubsistenceType {
    // Default option, used for agrarian
    Basic,
    Nomadic,
    Tribal,
    Vagrant
}

impl SubsistenceType {
    /// # Name
    ///
    /// The short name of this subsistence.
    ///
    /// Returns `basic`, `nomadic`, `tribal`, or `vagrant`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Nomadic => "nomadic",
            Self::Tribal => "tribal",
            Self::Vagrant => "vagrant",
        }
    }
}

impl MigrantGroup {
    /// # Empty
    ///
    /// A group with no households and no goods.
    fn empty() -> Self {
        Self {
            households: 0.0,
            shape: Household::new(),
            quantity: HashMap::new(),
        }
    }
}

impl Pop {
    /// # New
    ///
    /// Empty pop. Three desire tiers, no stock, no satisfaction bookmark.
    pub fn new(id: usize) -> Self {
        let household = Household::new();
        let pre_growth_population = household.total_count();
        Self {
            id,
            subsistence_rate: 0.0,
            subsistence_type: SubsistenceType::Basic,
            job: Job::none(),
            transport_spent: 0.0,
            transport_purchased: 0.0,
            time_consumed: 0.0,
            held_desire_time: 0.0,
            held_transport_time: 0.0,
            property: HashMap::new(),
            savings: HashMap::new(),
            effective_savings: 0.0,
            desires: vec![vec![]; 3],
            desire_cursor: None,
            demographics: DemoRow {
                household,
                species: 0,
                culture: 0,
            },
            pre_growth_population,
            population_cap: None,
            migration: 0.0,
            stored_effects: vec![],
            sentiment: Sentiment::new(),
            sol_samples: VecDeque::new(),
            records: PopRecords::default(),
        }
    }

    /// # Get Scaling Factor
    ///
    /// Resolves `scaling` against this pop's household.
    ///
    /// Fixed returns its number. The other arms multiply that number by the
    /// matching household count.
    pub fn get_scaling_factor(&self, scaling: ScalingFactor) -> f64 {
        match scaling {
            ScalingFactor::Fixed(f) => f,
            ScalingFactor::All(f) => f * self.demographics.total_population(),
            ScalingFactor::Household(f) => f * self.demographics.household.count,
            ScalingFactor::Adults(f) => f * self.demographics.adult_pop(),
            ScalingFactor::Children(f) => f * self.demographics.children_pop(),
            ScalingFactor::Elders(f) => f * self.demographics.elder_pop(),
            ScalingFactor::Labor(f) => f * self.demographics.labor(),
        }
    }

    /// # Household Count
    ///
    /// Households in this pop, and at least one.
    ///
    /// A count above one is that count. A smaller count is one household.
    pub(crate) fn household_count(&self) -> f64 {
        let count = self.demographics.household.count;
        debug_assert!(count.is_finite() && count >= 0.0);
        if count > 1.0 {
            count
        } else {
            1.0
        }
    }

    /// # Has Desire For
    ///
    /// True when a desire target lists `good`.
    pub(crate) fn has_desire_for(&self, good: usize) -> bool {
        self.desires.iter().flatten().any(|desire| {
            desire.target.iter().any(|target| {
                debug_assert!(target.efficiency.is_finite() && target.efficiency > 0.0);
                target.good == good
            })
        })
    }

    /// # Makes Good
    ///
    /// True when a job line outputs `good`.
    ///
    /// `factuals` supplies the process. A missing process does not count.
    pub(crate) fn makes_good(&self, factuals: &Factuals, good: usize) -> bool {
        self.job.lines.iter().any(|line| {
            factuals.get_process(line.process).is_some_and(|process| {
                process.outputs.iter().any(|output| {
                    debug_assert!(output.amount.is_finite() && output.amount > 0.0);
                    output.good == good
                })
            })
        })
    }

    /// # Takes Good
    ///
    /// True when a job line inputs `good`.
    ///
    /// `factuals` supplies the process. A missing process does not count.
    pub(crate) fn takes_good(&self, factuals: &Factuals, good: usize) -> bool {
        self.job.lines.iter().any(|line| {
            factuals.get_process(line.process).is_some_and(|process| {
                process.inputs.iter().any(|input| {
                    debug_assert!(input.amount.is_finite() && input.amount > 0.0);
                    input.good == good
                })
            })
        })
    }

    /// # Start Day
    ///
    /// Adds the day's generated goods, each scaled by the scaling factor given.
    ///
    /// `new_goods` is pairs of good id and scaling factor. Each amount is this
    /// pop's factor for that scaling. `quantity` and `produced` each gain
    /// that amount. A missing row is created. Returns each good id and the
    /// amount added, in the same order.
    pub fn start_day(&mut self, new_goods: &[(usize, ScalingFactor)]) -> Vec<(usize, f64)> {
        let mut added = Vec::with_capacity(new_goods.len());
        for (good_id, scaling) in new_goods {
            let amount = self.get_scaling_factor(*scaling);
            let val = self.property.entry(*good_id).or_default();
            val.quantity += amount;
            val.produced += amount;
            added.push((*good_id, amount));
        }
        added
    }

    /// # Run Subsistence
    ///
    /// Destroys `subsistence_rate` of this pop's time, then adds goods for
    /// that same rate.
    ///
    /// `factuals` supplies the goods. Time on hand is good [`TIME`].
    /// A positive share is removed from `quantity` and added to `spent`.
    /// Basic cover is the rate divided by `pop.subsistence_anchor`. Common
    /// cover is that times `pop.subsistence_common_share`. Each eligible
    /// good in a desire is given the same
    /// satisfaction. An industrial good is skipped. A good missing
    /// from `factuals` is skipped. A rate of 0 adds nothing and spends no
    /// time. Desire satisfaction is left unchanged.
    pub fn run_subsistence(&mut self, factuals: &Factuals) {
        let rate = self.subsistence_rate;
        debug_assert!(rate.is_finite() && rate >= 0.0);
        if rate == 0.0 {
            return;
        }
        // Take a positive share of time out of quantity and record it as spent.
        if let Some(row) = self.property.get_mut(&TIME) {
            let spent = row.quantity * rate;
            if spent > 0.0 {
                row.quantity -= spent;
                row.spent += spent;
            }
        }
        // The same rate decides how much of each desire the goods cover.
        let anchor = factuals.config.pop.subsistence_anchor;
        let common_share = factuals.config.pop.subsistence_common_share;
        let mut made = Vec::new();
        for (tier, desires) in self.desires.iter().enumerate() {
            let cover = subsistence_cover(tier, rate, anchor, common_share);
            if cover == 0.0 {
                continue;
            }
            for desire in desires {
                made.extend(subsistence_batch(factuals, desire, cover));
            }
        }
        for (good, amount) in made {
            self.add_made(good, amount);
        }
    }

    /// # Add Made
    ///
    /// Adds `amount` of `good` that this pop just produced.
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

    /// # Split Goods Sale Reserve
    ///
    /// Splits this pop's property into reserves and sell offers.
    ///
    /// `history` supplies the tender list and exchange rates. `factuals`
    /// supplies the processes.
    /// Reserved units stay off the book. Anything unreserved is returned as
    /// a sell offer, one per good, in good-id order.
    ///
    /// The walk is production, then basic, basic savings, common, and common
    /// savings. A later tier runs only when the tier before it is fully
    /// reserved. Luxury and luxury savings then alternate until the luxury
    /// tier cannot finish another level, or a savings leg cannot reach its
    /// total. Savings are measured in days of basic desires: 1 after basic,
    /// 3 after common, and 5 after the first luxury level. Each later
    /// luxury level adds 2 days. Each total includes currency already saved.
    pub fn split_goods_sale_reserve(
        &mut self,
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> Vec<SellOffer> {
        // This call is today's split. Earlier reserves do not carry in.
        for row in self.property.values_mut() {
            row.reserved = 0.0;
        }
        // Production: job inputs, up to each line's plan.
        self.reserve_production(factuals);
        // Basic, then savings up to one day of basic desires.
        if !self.reserve_tier(0, 1.0) {
            return self.sell_offers();
        }
        self.save_currency(history, 1.0);
        // Common, then savings up to three days of basic desires.
        if !self.reserve_tier(1, 1.0) {
            return self.sell_offers();
        }
        self.save_currency(history, 3.0);
        // Luxury and luxury savings, until the tier or the currency runs out.
        // This may be consolidated into a loop of larger legs to accelerate the process
        // if needed.
        let mut level = 1.0;
        while self.tier_len(2) > 0 {
            if !self.reserve_tier(2, level) {
                break;
            }
            let days = 5.0 + (level - 1.0) * 2.0;
            if !self.save_currency(history, days) {
                break;
            }
            level += 1.0;
        }
        self.sell_offers()
    }

    /// # Reserve Production
    ///
    /// Reserves job inputs up to each line's plan.
    ///
    /// `factuals` supplies the processes. A missing process is skipped.
    /// `Some(0)` reserves nothing. `Some(n)`
    /// reserves `n` iterations. `None` reserves as many iterations as the
    /// required inputs on hand allow. A factor is reserved once, at its
    /// listed amount. An optional input is reserved only when the line
    /// lists it.
    fn reserve_production(&mut self, factuals: &Factuals) {
        let lines: Vec<(usize, Option<f64>, Vec<usize>)> = self
            .job
            .lines
            .iter()
            .map(|line| (line.process, line.target, line.inputs.clone()))
            .collect();
        for (process_id, target, optional) in lines {
            let Some(process) = factuals.get_process(process_id) else {
                continue;
            };
            let iterations = self.planned_iterations(process, target);
            if iterations <= 0.0 {
                continue;
            }
            for input in &process.inputs {
                if input.is_optional() || matches!(input.input_type, InputType::Factor) {
                    continue;
                }
                self.reserve_up_to(input.good, iterations * input.amount);
            }
            for input in &process.inputs {
                if !matches!(input.input_type, InputType::Factor) {
                    continue;
                }
                if input.is_optional() && !optional.contains(&input.good) {
                    continue;
                }
                self.reserve_up_to(input.good, input.amount);
            }
            for good in optional {
                let Some(input) = process
                    .inputs
                    .iter()
                    .find(|input| input.good == good && input.is_optional())
                else {
                    continue;
                };
                if matches!(input.input_type, InputType::Factor) {
                    continue;
                }
                self.reserve_up_to(input.good, iterations * input.amount);
            }
        }
    }

    /// # Planned Iterations
    ///
    /// How many times `process` runs for `target`.
    ///
    /// `Some(n)` is that count. `Some(0)` runs nothing. `None` is the most
    /// iterations the required inputs on hand can cover, or 0 when nothing
    /// limits the run.
    fn planned_iterations(&self, process: &Process, target: Option<f64>) -> f64 {
        match target {
            Some(n) => {
                debug_assert!(n.is_finite() && n >= 0.0);
                n
            }
            None => self.iterations_from_stock(process),
        }
    }

    /// # Iterations From Stock
    ///
    /// The most iterations `process` can run from unreserved property.
    ///
    /// Only required inputs that are not factors limit the run. Returns 0
    /// when no such input exists.
    fn iterations_from_stock(&self, process: &Process) -> f64 {
        let mut cap = None;
        for input in &process.inputs {
            if input.is_optional() || matches!(input.input_type, InputType::Factor) {
                continue;
            }
            debug_assert!(input.amount.is_finite() && input.amount > 0.0);
            let runs = self.available(input.good) / input.amount;
            cap = Some(cap.map(|held: f64| held.min(runs)).unwrap_or(runs));
        }
        cap.unwrap_or(0.0)
    }

    /// # Reserve Tier
    ///
    /// Reserves goods toward one level of desire tier `tier`.
    ///
    /// `level` starts at 1. Desires are walked in stored order. Inside a
    /// desire, a higher efficiency is reserved first. Returns whether every
    /// desire in the tier reached that level. An empty tier is reached.
    fn reserve_tier(&mut self, tier: usize, level: f64) -> bool {
        let count = self.tier_len(tier);
        let mut gained = Vec::with_capacity(count);
        for index in 0..count {
            gained.push(self.reserve_desire(tier, index, level));
        }
        self.tier_is_met(tier, level, &gained)
    }

    /// # Reserve Desire
    ///
    /// Reserves property toward one level of one desire.
    ///
    /// `tier` and `index` pick the desire. `level` starts at 1. Satisfaction
    /// already held is spent on earlier levels first. Targets are taken
    /// highest efficiency first, and a lower good id breaks a tie. A target
    /// fills no more than its stored cap of this level. Units reserved are
    /// the satisfaction taken divided by efficiency. Returns the satisfaction
    /// reserved on this level.
    fn reserve_desire(&mut self, tier: usize, index: usize, level: f64) -> f64 {
        let goal = self.desires[tier][index].amount;
        debug_assert!(goal.is_finite() && goal > 0.0);
        let mut order: Vec<(usize, f64, f64)> = self.desires[tier][index]
            .target
            .iter()
            .map(|target| {
                debug_assert!(target.efficiency.is_finite() && target.efficiency > 0.0);
                debug_assert!(target.cap.is_finite() && target.cap >= 0.0);
                (target.good, target.efficiency, target.cap)
            })
            .collect();
        order.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(left.0.cmp(&right.0))
        });
        // Each target is visited once, so its room is the whole stored cap.
        let credit = self.open_credit(tier, index, level);
        let mut gained = 0.0;
        for (good, efficiency, cap) in order {
            let remaining = goal - credit - gained;
            if remaining <= 1e-9 {
                break;
            }
            let room = cap * goal;
            if room <= 0.0 {
                continue;
            }
            let take = self.reserve_up_to(good, remaining.min(room) / efficiency);
            if take > 0.0 {
                gained += take * efficiency;
            }
        }
        gained
    }

    /// # Tier Len
    ///
    /// How many desires are stored in `tier`. A missing tier is 0.
    fn tier_len(&self, tier: usize) -> usize {
        self.desires.get(tier).map(|tier| tier.len()).unwrap_or(0)
    }

    /// # Tier Is Met
    ///
    /// Whether every desire in `tier` covers this level.
    ///
    /// `level` starts at 1. `gained` is the satisfaction reserved on each
    /// desire this call, in stored order. A missing or empty tier is met.
    /// The level's goal is the desire's amount. Satisfaction past the
    /// earlier levels counts, plus `gained`. Coverage within `1e-9` counts.
    fn tier_is_met(&self, tier: usize, level: f64, gained: &[f64]) -> bool {
        let Some(desires) = self.desires.get(tier) else {
            return true;
        };
        desires.iter().enumerate().all(|(index, desire)| {
            debug_assert!(desire.amount.is_finite() && desire.amount > 0.0);
            let added = gained.get(index).copied().unwrap_or(0.0);
            self.open_credit(tier, index, level) + added + 1e-9 >= desire.amount
        })
    }

    /// # Open Credit
    ///
    /// Satisfaction already held that still applies to this level.
    ///
    /// `level` starts at 1. Earlier levels consume the desire's amount each.
    /// Credit below this level is 0.
    fn open_credit(&self, tier: usize, index: usize, level: f64) -> f64 {
        let desire = &self.desires[tier][index];
        let amount = desire.amount;
        debug_assert!(amount.is_finite() && amount > 0.0);
        debug_assert!(level.is_finite() && level >= 1.0);
        debug_assert!(desire.satisfaction.is_finite() && desire.satisfaction >= 0.0);
        (desire.satisfaction - amount * (level - 1.0)).max(0.0)
    }

    /// # Save Currency
    ///
    /// Tops savings up to `days` of basic desires, in total.
    ///
    /// `history` supplies the tender list and exchange rates. One day is
    /// the cost of the basic desires, priced in the first tender that can
    /// price them. Currency already in savings counts. The shortfall comes
    /// out of unreserved tender, in list order, and is removed from
    /// `quantity`. [`Self::effective_savings`] becomes the value held in
    /// that tender. Returns whether the total was reached. A day that
    /// cannot be priced is reached.
    fn save_currency(&mut self, history: &MarketHistory, days: f64) -> bool {
        debug_assert!(days.is_finite() && days > 0.0);
        let Some(unit) = self.account_currency(history) else {
            return true;
        };
        let target = days * self.basic_day_cost(history, unit);
        let mut gap = target - self.savings_value(history, unit);
        if gap > 0.0 {
            for &currency in &history.currencies {
                if gap <= 0.0 {
                    break;
                }
                let worth = self.tender_worth(history, currency, unit);
                debug_assert!(worth.is_finite() && worth >= 0.0);
                if worth == 0.0 {
                    continue;
                }
                let afford = self.available(currency);
                let take = (gap / worth).min(afford);
                if take > 0.0 {
                    self.move_to_savings(currency, take);
                    gap -= take * worth;
                }
            }
        }
        let held = self.savings_value(history, unit);
        self.effective_savings = held;
        held >= target
    }

    /// # Account Currency
    ///
    /// The first tender that can price one day of basic desires.
    ///
    /// `history` supplies the tender list. Returns `None` when no tender
    /// can price a day.
    fn account_currency(&self, history: &MarketHistory) -> Option<usize> {
        history
            .currencies
            .iter()
            .copied()
            .find(|&currency| self.basic_day_cost(history, currency) > 0.0)
    }

    /// # Basic Day Cost
    ///
    /// What one day of basic desires costs in `currency`.
    ///
    /// Each basic desire is bought from its cheapest targets, up to each
    /// target's cap. Returns 0 when nothing can be priced.
    fn basic_day_cost(&self, history: &MarketHistory, currency: usize) -> f64 {
        let Some(desires) = self.desires.first() else {
            return 0.0;
        };
        desires
            .iter()
            .map(|desire| self.desire_day_cost(history, currency, desire))
            .sum()
    }

    /// # Desire Day Cost
    ///
    /// What one day of `desire` costs in `currency`.
    ///
    /// Targets are taken lowest cost per satisfaction first. A lower good
    /// id breaks a tie. A target covers no more than its cap of the
    /// desire. Returns 0 when the desire has no priced target.
    fn desire_day_cost(&self, history: &MarketHistory, currency: usize, desire: &Desire) -> f64 {
        let goal = desire.amount;
        debug_assert!(goal.is_finite() && goal > 0.0);
        let mut quotes: Vec<(usize, f64, f64)> = desire
            .target
            .iter()
            .map(|target| {
                debug_assert!(target.efficiency.is_finite() && target.efficiency > 0.0);
                debug_assert!(target.cap.is_finite() && target.cap >= 0.0);
                let rate = quoted_rate(history, currency, target.good);
                let cost_per = 1.0 / (rate * target.efficiency);
                (target.good, target.cap, cost_per)
            })
            .collect();
        quotes.sort_by(|left, right| {
            left.2
                .partial_cmp(&right.2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(left.0.cmp(&right.0))
        });
        let mut remaining = goal;
        let mut cost = 0.0;
        for (_, cap, cost_per) in quotes {
            if remaining <= 1e-9 {
                break;
            }
            let take = remaining.min(cap * goal);
            if take <= 0.0 {
                continue;
            }
            cost += take * cost_per;
            remaining -= take;
        }
        cost
    }

    /// # Savings
    ///
    /// Currency set aside, keyed by good id.
    ///
    /// Returns that map.
    pub fn savings(&self) -> &HashMap<usize, f64> {
        &self.savings
    }

    /// # Savings Value
    ///
    /// Currency already saved, priced in `unit`.
    ///
    /// Tender outside `history` adds nothing.
    fn savings_value(&self, history: &MarketHistory, unit: usize) -> f64 {
        self.savings
            .iter()
            .map(|(&good, &amount)| {
                debug_assert!(amount.is_finite() && amount >= 0.0);
                amount * self.tender_worth(history, good, unit)
            })
            .sum()
    }

    /// # Tender Worth
    ///
    /// How many units of `unit` one unit of `currency` is worth.
    ///
    /// The account tender is worth 1. Another tender is worth the ratio of
    /// their basic-day costs. A tender that cannot price a day is worth 0.
    fn tender_worth(&self, history: &MarketHistory, currency: usize, unit: usize) -> f64 {
        if !history.currencies.contains(&currency) {
            return 0.0;
        }
        if currency == unit {
            return 1.0;
        }
        let unit_cost = self.basic_day_cost(history, unit);
        let other_cost = self.basic_day_cost(history, currency);
        debug_assert!(unit_cost.is_finite() && unit_cost >= 0.0);
        debug_assert!(other_cost.is_finite() && other_cost >= 0.0);
        if unit_cost == 0.0 || other_cost == 0.0 {
            return 0.0;
        }
        unit_cost / other_cost
    }

    /// # Move To Savings
    ///
    /// Removes `amount` of `good` from property and stores it in savings.
    ///
    /// `amount` is positive and no greater than the unreserved quantity.
    /// Aged units leave before fresh.
    fn move_to_savings(&mut self, good: usize, amount: f64) {
        debug_assert!(amount.is_finite() && amount > 0.0);
        debug_assert!(amount <= self.available(good));
        let row = self
            .property
            .get_mut(&good)
            .expect("available property has a row");
        row.spend_aged_first(amount);
        row.quantity -= amount;
        *self.savings.entry(good).or_insert(0.0) += amount;
    }

    /// # Available
    ///
    /// Unreserved quantity of `good`. A missing row is 0.
    fn available(&self, good: usize) -> f64 {
        self.property
            .get(&good)
            .map(|row| row.available())
            .unwrap_or(0.0)
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
            row.reserved += take;
        }
        take
    }

    /// # Sell Offers
    ///
    /// Unreserved property, one offer per good, in good-id order.
    ///
    /// A good with nothing left unreserved is left out.
    fn sell_offers(&self) -> Vec<SellOffer> {
        let mut offers: Vec<SellOffer> = self
            .property
            .iter()
            .filter_map(|(&good, row)| {
                let amount = row.quantity - row.reserved;
                (amount > 0.0).then_some(SellOffer {
                    pop: self.id,
                    good,
                    amount,
                })
            })
            .collect();
        offers.sort_by_key(|offer| offer.good);
        offers
    }

    /// # Produce
    ///
    /// Runs this pop's job.
    ///
    /// `factuals` supplies the processes. The lines run in order. A missing
    /// process is skipped. `Some(0)` does not run. Each line records the
    /// iterations it was asked for and the iterations it finished.
    /// Subsistence is not run.
    pub fn produce(&mut self, factuals: &Factuals) {
        let lines: Vec<(usize, Option<f64>, Vec<usize>)> = self
            .job
            .lines
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
            let inputs = self.job_inputs(process, &optional);
            let result = process.do_process(&inputs, run_target, factuals);
            self.apply_job_result(&result);
            self.record_run(index, attempted, result.iterations);
        }
    }

    /// # Record Run
    ///
    /// Stores one job line's ask and its finish.
    ///
    /// `index` selects the line. `attempted` is the iterations it was asked
    /// for. `completed` is the iterations it finished.
    fn record_run(&mut self, index: usize, attempted: f64, completed: f64) {
        debug_assert!(attempted.is_finite() && attempted >= 0.0);
        debug_assert!(completed.is_finite() && completed >= 0.0);
        self.job.lines[index].attempted = attempted;
        self.job.lines[index].completed = completed;
    }

    /// # Job Inputs
    ///
    /// On-hand quantity for the inputs one job line may draw.
    ///
    /// A required input is included when its `quantity` is positive. An
    /// optional input is included only when `optional` lists it and that
    /// quantity is positive. Any other good is left out.
    fn job_inputs(&self, process: &Process, optional: &[usize]) -> HashMap<usize, f64> {
        let mut inputs = HashMap::new();
        for input in &process.inputs {
            let listed = !input.is_optional() || optional.contains(&input.good);
            if !listed {
                continue;
            }
            let quantity = self
                .property
                .get(&input.good)
                .map(|row| {
                    debug_assert!(row.quantity.is_finite() && row.quantity >= 0.0);
                    row.quantity
                })
                .unwrap_or(0.0);
            if quantity > 0.0 {
                inputs.insert(input.good, quantity);
            }
        }
        inputs
    }

    /// # Apply Job Result
    ///
    /// Writes one process result onto this pop's property.
    ///
    /// Units destroyed are removed from `quantity` and added to `spent`.
    /// Capital leaves `quantity` and is added to `used`. Aged units go
    /// before fresh. `reserved` falls by the units that left. Units put
    /// out go through [`Self::add_made`]. Effects are stored on this pop.
    fn apply_job_result(&mut self, result: &ProcessResult) {
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
                    row.spent += spent;
                }
                if used > 0.0 {
                    row.quantity -= used;
                    row.used += used;
                }
                if removed > 0.0 {
                    row.reserved = (row.reserved - removed).max(0.0);
                }
            }
            if made > 0.0 {
                self.add_made(good, made);
            }
        }
        // Wrap up by recording process effects.
        for effect in &result.effects {
            self.stored_effects.push((*effect).into());
        }
    }

    /// # Consume
    ///
    /// Satisfies this pop's desires from goods on hand.
    ///
    /// `factuals` supplies the goods. Basic runs, then common, then luxury.
    /// A tier runs only when every desire in the tier before it covers its
    /// level. Luxury repeats one level at a time until a level cannot be
    /// finished. One level is the desire's amount. Satisfaction already
    /// held covers the earlier levels of that desire first. Effects other
    /// steps spend are then stored by [`Self::record_desire_effects`].
    pub fn consume(&mut self, factuals: &Factuals) {
        if self.consume_tier(factuals, 0, 1.0) && self.consume_tier(factuals, 1, 1.0) {
            let mut level = 1.0;
            while self.tier_len(2) > 0 {
                if !self.consume_tier(factuals, 2, level) {
                    break;
                }
                level += 1.0;
            }
        }
        self.record_desire_effects();
    }

    /// # Record Desire Effects
    ///
    /// Stores the desire effects growth, decay, and resource harvest spend.
    ///
    /// The tier index is the desire's tier. Levels met are
    /// [`Desire::tiers_satisfied`].
    /// [`crate::game::effects::DesireEffect::to_pop_effect`] decides the
    /// stored magnitude. Satisfaction and sentiment stay on the desire.
    /// A desire with no effects is skipped.
    fn record_desire_effects(&mut self) {
        let mut recorded = Vec::new();
        for (tier, desires) in self.desires.iter().enumerate() {
            for desire in desires {
                if desire.effect.is_empty() {
                    continue;
                }
                debug_assert!(desire.amount.is_finite() && desire.amount > 0.0);
                let tiers = desire.tiers_satisfied();
                for effect in &desire.effect {
                    if matches!(
                        effect,
                        DesireEffect::Satisfaction(..)
                            | DesireEffect::SentimentFlat(..)
                            | DesireEffect::SentimentRelative(..)
                    ) {
                        continue;
                    }
                    if let Some(effect) = effect.to_pop_effect(tier, tiers) {
                        recorded.push(effect);
                    }
                }
            }
        }
        self.stored_effects.extend(recorded);
    }

    /// # Consume Tier
    ///
    /// Consumes goods toward one level of desire tier `tier`.
    ///
    /// `level` starts at 1. Desires are walked in stored order. The
    /// satisfaction gained on this level is added to each desire. Returns
    /// whether every desire in the tier reached that level. An empty tier
    /// is reached.
    fn consume_tier(&mut self, factuals: &Factuals, tier: usize, level: f64) -> bool {
        let count = self.tier_len(tier);
        for index in 0..count {
            let gained = self.consume_desire(factuals, tier, index, level);
            self.desires[tier][index].satisfaction += gained;
        }
        self.is_level_met(tier, level)
    }

    /// # Is Level Met
    ///
    /// Whether every desire in `tier` covers this level.
    ///
    /// `level` starts at 1. A missing or empty tier is met. Satisfaction
    /// must reach the desire's amount times `level`.
    fn is_level_met(&self, tier: usize, level: f64) -> bool {
        let Some(desires) = self.desires.get(tier) else {
            return true;
        };
        desires.iter().all(|desire| {
            debug_assert!(desire.amount.is_finite() && desire.amount > 0.0);
            debug_assert!(desire.satisfaction.is_finite() && desire.satisfaction >= 0.0);
            desire.satisfaction >= desire.amount * level
        })
    }

    /// # Consume Desire
    ///
    /// Consumes property toward one level of one desire.
    ///
    /// `tier` and `index` pick the desire. `level` starts at 1. Each good
    /// is first brought toward [`Desire::minimum`], in bucket order. The
    /// rest is taken highest efficiency first, and a lower good id breaks
    /// a tie. A good stops at [`Desire::cap`]. Both read
    /// [`Factuals::config`]. Returns the satisfaction gained on this level.
    fn consume_desire(
        &mut self, factuals: &Factuals, tier: usize, index: usize, level: f64,
    ) -> f64 {
        let goal = self.desires[tier][index].amount;
        debug_assert!(goal.is_finite() && goal > 0.0);
        if self.desires[tier][index].target.is_empty() {
            return 0.0;
        }
        let mut taken = vec![0.0; self.desires[tier][index].target.len()];
        let mut order: Vec<(usize, usize, f64, DesireTargetType)> = self.desires[tier][index]
            .target
            .iter()
            .enumerate()
            .map(|(slot, target)| {
                debug_assert!(target.efficiency.is_finite() && target.efficiency > 0.0);
                (slot, target.good, target.efficiency, target.desire_type)
            })
            .collect();
        // Each good gets a turn at its preferred minimum before efficiency.
        for (slot, good, efficiency, kind) in &order {
            let share = self.desires[tier][index].minimum(*slot, &factuals.config.pop);
            self.take_toward(
                factuals, tier, index, level, *slot, *good, *efficiency, *kind,
                goal * share, &mut taken,
            );
        }
        order.sort_by(|left, right| {
            right
                .2
                .partial_cmp(&left.2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(left.1.cmp(&right.1))
        });
        for (slot, good, efficiency, kind) in &order {
            let share = self.desires[tier][index].cap(*slot, &factuals.config.pop);
            self.take_toward(
                factuals, tier, index, level, *slot, *good, *efficiency, *kind,
                goal * share, &mut taken,
            );
        }
        taken.iter().sum()
    }

    /// # Take Toward
    ///
    /// Takes `good` up to `ceiling` satisfaction on this level.
    ///
    /// `taken` is the satisfaction already gained on this level, one entry
    /// per bucket good. Stops at the satisfaction this level still needs.
    /// Units taken are that satisfaction divided by `efficiency`.
    fn take_toward(
        &mut self, factuals: &Factuals, tier: usize, index: usize, level: f64, slot: usize,
        good: usize, efficiency: f64, kind: DesireTargetType, ceiling: f64, taken: &mut [f64],
    ) {
        let remaining = self.desires[tier][index].amount
            - self.open_credit(tier, index, level)
            - taken.iter().sum::<f64>();
        if remaining <= 0.0 {
            return;
        }
        let room = ceiling - taken[slot];
        if room <= 0.0 {
            return;
        }
        debug_assert!(efficiency.is_finite() && efficiency > 0.0);
        let take = self.take_for_desire(factuals, good, remaining.min(room) / efficiency, kind);
        if take > 0.0 {
            taken[slot] += take * efficiency;
        }
    }

    /// # Take For Desire
    ///
    /// Removes up to `want` of `good` for one desire target.
    ///
    /// `kind` decides the ledger. Consume records `consumed` and adds that
    /// good's decay results. Use records `used`. Aged units go first.
    /// `reserved` falls by the units that left. Returns the units taken.
    /// Nothing on hand takes nothing.
    fn take_for_desire(
        &mut self,
        factuals: &Factuals,
        good: usize,
        want: f64,
        kind: DesireTargetType,
    ) -> f64 {
        debug_assert!(want.is_finite() && want > 0.0);
        let take = {
            let Some(row) = self.property.get_mut(&good) else {
                return 0.0;
            };
            let take = want.min(row.quantity);
            if take <= 0.0 {
                return 0.0;
            }
            row.spend_aged_first(take);
            row.quantity -= take;
            match kind {
                DesireTargetType::Consume => row.consumed += take,
                DesireTargetType::Use => row.used += take,
            }
            row.reserved = (row.reserved - take).max(0.0);
            take
        };
        if matches!(kind, DesireTargetType::Consume) {
            self.add_consumed_byproducts(factuals, good, take);
        }
        take
    }

    /// # Add Consumed Byproducts
    ///
    /// Adds the decay results of consuming `amount` of `good`.
    ///
    /// `factuals` supplies the good. Each result is `amount` times its
    /// rate, added to `quantity`. A missing good adds nothing. A result
    /// that is not positive is skipped.
    fn add_consumed_byproducts(&mut self, factuals: &Factuals, good: usize, amount: f64) {
        let Some(kind) = factuals.get_good(good) else {
            return;
        };
        let gains: Vec<(usize, f64)> = kind
            .decay_result
            .iter()
            .filter(|(_, rate)| {
                debug_assert!(rate.is_finite() && **rate >= 0.0);
                **rate > 0.0
            })
            .map(|(&id, &rate)| (id, amount * rate))
            .collect();
        for (id, gain) in gains {
            self.property.entry(id).or_default().quantity += gain;
        }
    }

    /// # Apply Demographic Changes
    ///
    /// Updates this pop's desires from their demographic desires.
    ///
    /// `factuals` supplies the species and culture desires. Each desire
    /// looks up its source. A missing source is left as it is. Size writes
    /// `amount` as the demographic amount times this pop's scale for the
    /// demographic scalar. A bucket whose goods differ, in order, is
    /// replaced by the demographic bucket. Satisfaction and the other
    /// fields stay.
    pub fn apply_demographic_changes(&mut self, factuals: &Factuals) {
        // Read the demographic desires first. The pop is written after.
        let mut edits = Vec::new();
        for (tier_index, tier) in self.desires.iter().enumerate() {
            for (desire_index, desire) in tier.iter().enumerate() {
                let Some(demo) = factuals.source_demo_desire(desire) else {
                    continue;
                };
                let amount = demo.amount * self.get_scaling_factor(demo.scalar);
                let same_goods = desire.target.len() == demo.bucket.len()
                    && desire
                        .target
                        .iter()
                        .zip(&demo.bucket)
                        .all(|(current, updated)| current.good == updated.good);
                if desire.amount != amount || !same_goods {
                    let bucket = (!same_goods).then(|| demo.bucket.clone());
                    edits.push((tier_index, desire_index, amount, bucket));
                }
            }
        }
        for (tier_index, desire_index, amount, bucket) in edits {
            let desire = &mut self.desires[tier_index][desire_index];
            desire.amount = amount;
            if let Some(bucket) = bucket {
                desire.target = bucket;
            }
        }
    }

    /// # Growth Phase
    ///
    /// Births and deaths for this pop.
    ///
    /// `factuals` supplies species, culture, config, and birth sentiment.
    /// A household count under 1 is left for the prune. Otherwise the annual
    /// bundle starts at [`Factuals::get_demographic_rates`]. Stored birth and
    /// mortality effects are added to that bundle and removed. Other stored
    /// effects stay. Births per woman are then scaled by this pop's sentiment.
    /// [`Household::update`] applies the bundle for
    /// [`crate::game::config::PopConfig::turn_length`].
    /// The population before that update is stored for [`Self::rescale`].
    /// The cap and migration are later steps.
    pub fn growth_phase(&mut self, factuals: &Factuals) {
        if self.demographics.household.count < 1.0 {
            return;
        }
        // Plans and desire amounts still match this size.
        self.pre_growth_population = self.demographics.total_population();
        let mut rates = factuals.get_demographic_rates(self.demographics);
        let mut kept = Vec::with_capacity(self.stored_effects.len());
        for effect in self.stored_effects.drain(..) {
            match effect {
                PopEffect::Birthrate(rate) => rates.birth_per_woman += rate,
                PopEffect::Mortality(target, rate) => rates.apply_mortality(target, rate),
                other => kept.push(other),
            }
        }
        self.stored_effects = kept;
        let mood = factuals.birth_sentiment(self.demographics);
        rates.birth_per_woman *= mood.multiplier(
            self.sentiment.hope(),
            self.sentiment.happiness(),
            self.sentiment.fear(),
            self.sentiment.anger(),
        );
        let turn_length = factuals.config.pop.turn_length;
        self.demographics.household.update(&rates, turn_length, &factuals.config.pop);
    }

    /// # Rescale
    ///
    /// Brings this pop's plans and desire amounts to its current population.
    ///
    /// The ratio is the current people divided by [`Self::pre_growth_population`].
    /// Each set plan target and each desire amount is multiplied by that ratio.
    /// An open plan stays open. Satisfaction and the goods in each bucket stay.
    /// The stored population becomes the current one. [`Self::planning`] then
    /// fits those targets to whole steps of the labor grant.
    pub fn rescale(&mut self) {
        let new_pop = self.demographics.total_population();
        let old_pop = self.pre_growth_population;
        debug_assert!(old_pop.is_finite() && old_pop > 0.0);
        debug_assert!(new_pop.is_finite() && new_pop >= 0.0);
        let ratio = new_pop / old_pop;
        if ratio != 1.0 {
            for line in &mut self.job.lines {
                if let Some(target) = line.target.as_mut() {
                    debug_assert!(target.is_finite() && *target >= 0.0);
                    *target *= ratio;
                }
            }
            for tier in &mut self.desires {
                for desire in tier {
                    debug_assert!(desire.amount.is_finite() && desire.amount >= 0.0);
                    desire.amount *= ratio;
                }
            }
        }
        self.pre_growth_population = new_pop;
    }

    /// # Record Keeping
    ///
    /// Stores this pop's satisfaction and clears the day's flow counters.
    ///
    /// Satisfaction is the sum across every desire. A pop with no desires
    /// stores infinity. The time row's `consumed` is copied to
    /// [`Self::time_consumed`] before the counters clear. A day with no
    /// time row stores 0. `produced`, `consumed`, `spent`, `lost`,
    /// `reserved`, and `fresh` return to 0. Quantity stays.
    pub fn record_keeping(&mut self) {
        let mut satisfaction = 0.0;
        let mut any = false;
        for tier in &self.desires {
            for desire in tier {
                any = true;
                debug_assert!(desire.satisfaction.is_finite() && desire.satisfaction >= 0.0);
                satisfaction += desire.satisfaction;
            }
        }
        self.records.satisfaction = if any { satisfaction } else { f64::INFINITY };
        // Planning runs after this clear, so the time it needs has to outlive the row.
        self.time_consumed = self.property.get(&TIME).map(|row| row.consumed).unwrap_or(0.0);
        for row in self.property.values_mut() {
            row.produced = 0.0;
            row.consumed = 0.0;
            row.spent = 0.0;
            row.lost = 0.0;
            row.reserved = 0.0;
            row.fresh = 0.0;
        }
    }

    /// # Decay Goods
    ///
    /// End-of-day decay for this pop's stock.
    ///
    /// `factuals` supplies each good. `used` returns to `quantity`, then the
    /// aging part of `quantity` decays by the good's rate. [`PopPRow::fresh`]
    /// stays. [`GoodTag::Exposure`] skips decay while the good is owned.
    /// Byproducts are credited into `quantity`. [`PopPRow::lost`] is the
    /// units that rotted. A stored [`PopEffect::BonusGood`] is added after
    /// the rot and removed. Other stored effects stay.
    pub fn decay_goods(&mut self, factuals: &Factuals) {
        let mut gains: HashMap<usize, f64> = HashMap::new();
        for (&good_id, row) in self.property.iter_mut() {
            debug_assert!(row.used.is_finite() && row.used >= 0.0);
            if row.used > 0.0 {
                row.quantity += row.used;
                row.used = 0.0;
            }

            debug_assert!(row.quantity.is_finite() && row.quantity >= 0.0);
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

        // Bonus goods arrive after the rot, so this night does not take them.
        let mut kept = Vec::with_capacity(self.stored_effects.len());
        for effect in self.stored_effects.drain(..) {
            match effect {
                PopEffect::BonusGood { good, amount } => {
                    debug_assert!(amount.is_finite() && amount >= 0.0);
                    self.property.entry(good).or_default().quantity += amount;
                }
                other => kept.push(other),
            }
        }
        self.stored_effects = kept;
    }

    /// # Decay Desires
    ///
    /// Decays satisfaction on every desire.
    ///
    /// Each desire keeps `satisfaction * (1.0 - decay)`. A decay of 0 keeps
    /// satisfaction. A decay of 1 clears it. Amount and the bucket stay.
    pub fn decay_desires(&mut self) {
        for tier in &mut self.desires {
            for desire in tier {
                debug_assert!(desire.decay.is_finite() && (0.0..=1.0).contains(&desire.decay));
                debug_assert!(desire.satisfaction.is_finite() && desire.satisfaction >= 0.0);
                desire.satisfaction *= 1.0 - desire.decay;
            }
        }
    }

    /// # Update Sentiments
    ///
    /// Moves this pop's mood from today's satisfaction, wealth, and living.
    ///
    /// `market` prices wealth. A missing market prices every good at 1.
    /// Desire sentiment arms are read from each desire. Stored sentiment
    /// arms are applied and removed. Other stored effects stay.
    /// [`Self::push_living_mods`] then adds the tier, wealth, and living
    /// flats, and [`Self::push_contentment_pull`] moves each other emotion
    /// toward contentment. The list is applied together through
    /// [`Sentiment::apply_mods_simultaniously`]. An empty list leaves the
    /// sentiment. Today's living is recorded after that.
    pub fn update_sentiments(&mut self, market: Option<&Market>) {
        let mut mods = Vec::new();
        // Desire arms, from the satisfaction consume wrote.
        for tier in &self.desires {
            for desire in tier {
                let sat = desire.tiers_satisfied();
                if !sat.is_finite() {
                    continue;
                }
                for effect in &desire.effect {
                    match *effect {
                        DesireEffect::SentimentFlat(kind, _, _) => {
                            mods.push(SentimentMod::Flat {
                                kind,
                                delta: effect.signed_strength(sat),
                            });
                        }
                        DesireEffect::SentimentRelative(kind, _, _) => {
                            mods.push(SentimentMod::Relative {
                                kind,
                                relative: effect.signed_strength(sat),
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
        // Stored arms are spent here. Other same-day effects stay.
        let mut kept = Vec::new();
        for effect in self.stored_effects.drain(..) {
            match effect {
                PopEffect::SentimentFlat { kind, delta } => {
                    mods.push(SentimentMod::Flat { kind, delta });
                }
                PopEffect::SentimentRelative { kind, relative } => {
                    mods.push(SentimentMod::Relative { kind, relative });
                }
                other => kept.push(other),
            }
        }
        self.stored_effects = kept;
        self.push_living_mods(market, &mut mods);
        self.push_contentment_pull(&mut mods);
        if !mods.is_empty() {
            self.sentiment.apply_mods_simultaniously(mods);
        }
        self.record_sol();
    }

    /// # Standard of Living
    ///
    /// The sum [`Self::calculate_sol`] records, without removing stored
    /// satisfaction or writing the record.
    ///
    /// No desires returns `+inf`. Otherwise each desire adds its tiers
    /// satisfied and its bonus satisfaction, then each stored
    /// [`PopEffect::Satisfaction`] amount is added. Returns that sum.
    pub(crate) fn standard_of_living(&self) -> f64 {
        if self.desires.iter().all(|tier| tier.is_empty()) {
            return f64::INFINITY;
        }
        let mut sum = 0.0;
        for tier in &self.desires {
            for desire in tier {
                sum += desire.tiers_satisfied();
                sum += desire.get_bonus_satisfaction();
            }
        }
        for effect in &self.stored_effects {
            if let PopEffect::Satisfaction { amount, .. } = *effect {
                sum += amount;
            }
        }
        sum
    }

    /// # Recorded Living
    ///
    /// The latest standard of living [`Self::update_sentiments`] stored.
    ///
    /// No sample yet is `None`. Returns that reading.
    pub fn recorded_living(&self) -> Option<f64> {
        self.sol_samples.back().copied()
    }

    /// # Wealth per Household
    ///
    /// Holding value of what this pop still has, divided by its household count.
    ///
    /// `market` prices each unit through [`market_price`]. Time is the day's
    /// labor and is left out. Property quantity and [`Self::savings`] both
    /// count. A household count at or below 0, or a non-finite result,
    /// returns 0.
    pub(crate) fn wealth_per_household(&self, market: Option<&Market>) -> f64 {
        let households = self.demographics.household.count;
        if households <= 0.0 {
            return 0.0;
        }
        let mut value = 0.0;
        for (&good, row) in &self.property {
            if good == TIME || row.quantity == 0.0 {
                continue;
            }
            value += row.quantity * market_price(market, good);
        }
        for (&good, &qty) in &self.savings {
            if good == TIME || qty == 0.0 {
                continue;
            }
            value += qty * market_price(market, good);
        }
        let per = value / households;
        if per.is_finite() { per } else { 0.0 }
    }

    /// # Tier Met
    ///
    /// Mean levels met on `tier`, plus stored satisfaction for that tier.
    ///
    /// Each desire adds its tiers satisfied and its bonus satisfaction.
    /// A non-finite desire is skipped. Stored [`PopEffect::Satisfaction`]
    /// on this tier is added once, after the mean. No desires and no stored
    /// satisfaction returns 0.
    fn tier_met(&self, tier: usize) -> f64 {
        let mut sum = 0.0;
        let mut count = 0.0;
        if let Some(desires) = self.desires.get(tier) {
            for desire in desires {
                let levels = desire.tiers_satisfied();
                let bonus = desire.get_bonus_satisfaction();
                if !levels.is_finite() || !bonus.is_finite() {
                    continue;
                }
                sum += levels + bonus;
                count += 1.0;
            }
        }
        let mut stored = 0.0;
        for effect in &self.stored_effects {
            if let PopEffect::Satisfaction {
                tier: effect_tier,
                amount,
            } = *effect
            {
                if effect_tier == tier && amount.is_finite() {
                    stored += amount;
                }
            }
        }
        let mean = if count > 0.0 { sum / count } else { 0.0 };
        mean + stored
    }

    /// # Basic Unmet
    ///
    /// Mean shortfall of the basic desires, reduced by stored basic satisfaction.
    ///
    /// Each desire's shortfall is how far its levels plus bonus sit below one
    /// full level, and not below 0. One desire met past one level does not
    /// fill another desire's gap. The mean of those shortfalls is then
    /// reduced by stored satisfaction on tier 0, and not below 0. No basic
    /// desires returns 0.
    fn basic_unmet(&self) -> f64 {
        let Some(desires) = self.desires.get(0) else {
            return 0.0;
        };
        let mut shortfall = 0.0;
        let mut count = 0.0;
        for desire in desires {
            let levels = desire.tiers_satisfied();
            let bonus = desire.get_bonus_satisfaction();
            if !levels.is_finite() || !bonus.is_finite() {
                continue;
            }
            shortfall += (1.0 - (levels + bonus)).max(0.0);
            count += 1.0;
        }
        if count == 0.0 {
            return 0.0;
        }
        let mut stored = 0.0;
        for effect in &self.stored_effects {
            if let PopEffect::Satisfaction { tier, amount } = *effect {
                if tier == 0 && amount.is_finite() {
                    stored += amount;
                }
            }
        }
        (shortfall / count - stored).max(0.0)
    }

    /// # Push Living Mods
    ///
    /// Appends the tier, wealth, and living-shift flats to `mods`.
    ///
    /// Unmet basic needs move [`BASIC_UNMET_SENTIMENT`] per full share into
    /// anger and the same share into fear. Common satisfaction moves
    /// [`COMMON_SENTIMENT`] per full share into contentment, including a
    /// share above one. Luxury levels move [`LUXURY_SENTIMENT`] per level
    /// into hope and the same share into happiness, and further levels keep
    /// counting. Wealth per household moves [`WEALTH_SENTIMENT`] per point
    /// into hope when positive and fear when negative. A rise in living
    /// since its recent average moves [`SOL_CHANGE_SENTIMENT`] per point
    /// into hope and the same share into happiness. A fall moves that share
    /// into anger and the same share into fear. A zero level adds nothing.
    fn push_living_mods(&self, market: Option<&Market>, mods: &mut Vec<SentimentMod>) {
        let basic = self.basic_unmet();
        if basic > 0.0 {
            let delta = basic * BASIC_UNMET_SENTIMENT;
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Anger,
                delta,
            });
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Fear,
                delta,
            });
        }
        let common = self.tier_met(1);
        if common > 0.0 {
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Contentment,
                delta: common * COMMON_SENTIMENT,
            });
        }
        let luxury = self.tier_met(2);
        if luxury > 0.0 {
            let delta = luxury * LUXURY_SENTIMENT;
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Hope,
                delta,
            });
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Happiness,
                delta,
            });
        }
        let wealth = self.wealth_per_household(market);
        if wealth != 0.0 {
            let delta = wealth * WEALTH_SENTIMENT;
            if delta > 0.0 {
                mods.push(SentimentMod::Flat {
                    kind: SentimentKind::Hope,
                    delta,
                });
            } else if delta < 0.0 {
                mods.push(SentimentMod::Flat {
                    kind: SentimentKind::Fear,
                    delta: -delta,
                });
            }
        }
        let shift = self.sol_shift();
        if shift > 0.0 {
            let delta = shift * SOL_CHANGE_SENTIMENT;
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Hope,
                delta,
            });
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Happiness,
                delta,
            });
        } else if shift < 0.0 {
            let delta = -shift * SOL_CHANGE_SENTIMENT;
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Anger,
                delta,
            });
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Fear,
                delta,
            });
        }
    }

    /// # Push Contentment Pull
    ///
    /// Appends a transfer into contentment for each other emotion.
    ///
    /// The amount is that emotion's current share times [`CONTENTMENT_PULL`].
    /// A zero share adds nothing.
    fn push_contentment_pull(&self, mods: &mut Vec<SentimentMod>) {
        for kind in [
            SentimentKind::Happiness,
            SentimentKind::Anger,
            SentimentKind::Fear,
            SentimentKind::Hope,
        ] {
            let share = self.sentiment.get(kind);
            if share > 0.0 {
                mods.push(SentimentMod::Transfer {
                    from: kind,
                    to: SentimentKind::Contentment,
                    amount: share * CONTENTMENT_PULL,
                });
            }
        }
    }

    /// # Standard of Living Shift
    ///
    /// Today's living minus the mean of the recent samples.
    ///
    /// The samples are earlier days only. No samples, or a non-finite
    /// living, returns 0.
    fn sol_shift(&self) -> f64 {
        let today = self.standard_of_living();
        if !today.is_finite() || self.sol_samples.is_empty() {
            return 0.0;
        }
        let average = self.sol_samples.iter().sum::<f64>() / self.sol_samples.len() as f64;
        let shift = today - average;
        if shift.is_finite() { shift } else { 0.0 }
    }

    /// # Record Standard of Living
    ///
    /// Appends today's living to the samples [`Self::sol_shift`] reads.
    ///
    /// A non-finite living is left out. The list keeps the most recent
    /// [`SOL_AVERAGE_DAYS`] readings.
    fn record_sol(&mut self) {
        let today = self.standard_of_living();
        if !today.is_finite() {
            return;
        }
        if self.sol_samples.len() == SOL_AVERAGE_DAYS {
            self.sol_samples.pop_front();
        }
        self.sol_samples.push_back(today);
    }

    /// # Calculate Standard of Living
    ///
    /// Records this pop's standard of living and drops stored satisfaction.
    ///
    /// No desires stores infinity on [`PopRecords::satisfaction`] and returns
    /// it. Stored effects stay in that case. Otherwise the sum is
    /// [`Self::standard_of_living`]. Stored satisfaction is then removed.
    /// The sum is stored on the record and returned.
    pub fn calculate_sol(&mut self) -> f64 {
        if self.desires.iter().all(|tier| tier.is_empty()) {
            self.records.satisfaction = f64::INFINITY;
            return f64::INFINITY;
        }
        let sum = self.standard_of_living();
        // Drop the stored satisfaction now that the sum includes it.
        let _ = self.satisfaction_from_pop_effects();
        self.records.satisfaction = sum;
        sum
    }

    /// # Satisfaction from Pop Effects
    ///
    /// Sums stored satisfaction and removes those effects.
    ///
    /// The tier is not used. Other stored effects stay. Returns the sum.
    fn satisfaction_from_pop_effects(&mut self) -> f64 {
        let mut sum = 0.0;
        let mut kept = Vec::new();
        for effect in &self.stored_effects {
            if let PopEffect::Satisfaction { amount, .. } = *effect {
                sum += amount;
            } else {
                kept.push(*effect);
            }
        }
        self.stored_effects = kept;
        sum
    }

    /// # Market Day
    ///
    /// One pop does not buy here.
    ///
    /// A purchase takes goods from another pop, so the pop phase is
    /// [`buy_from_offers`]. `market` prices the goods. `factuals` supplies
    /// them. `offers` are the sell offers. This method does not buy.
    pub fn market_day(&mut self, market: &Market, factuals: &Factuals, offers: &[SellOffer]) {
        let _ = (self, market, factuals, offers);
    }

    /// # Plan Purchases
    ///
    /// The goods this pop buys from `offers` on this pass.
    ///
    /// `market` prices them. `factuals` says which goods can be bought.
    /// `currency` is the tender that pays. Basic runs, then common, then
    /// luxury one level at a time. A tier is shopped only when the tier
    /// before it is covered. One desire takes held goods first, cheapest
    /// satisfaction first, then buys the same way. A buy is whole units.
    /// The pop does not buy its own offer, the tender, time, or a good that
    /// cannot be bought. Loose tender pays before savings. Returns those
    /// purchases.
    fn plan_purchases(
        &self, market: &Market, factuals: &Factuals, offers: &[SellOffer], currency: usize,
    ) -> Vec<Purchase> {
        let mut money = self.available(currency) + self.savings.get(&currency).copied().unwrap_or(0.0);
        debug_assert!(money.is_finite() && money >= 0.0);
        let mut claimed: HashMap<usize, f64> = HashMap::new();
        let mut book: Vec<Listing> = offers
            .iter()
            .map(|offer| Listing { pop: offer.pop, good: offer.good, amount: offer.amount })
            .collect();
        let mut purchases = Vec::new();
        // A later tier waits until the one before it is covered.
        if !self.shop_tier(
            market, factuals, 0, 1.0, currency, &mut money, &mut claimed, &mut book, &mut purchases,
        ) {
            return purchases;
        }
        if !self.shop_tier(
            market, factuals, 1, 1.0, currency, &mut money, &mut claimed, &mut book, &mut purchases,
        ) {
            return purchases;
        }
        let mut level = 1.0;
        while self.tier_len(2) > 0 {
            if !self.shop_tier(
                market, factuals, 2, level, currency, &mut money, &mut claimed, &mut book,
                &mut purchases,
            ) {
                break;
            }
            level += 1.0;
        }
        purchases
    }

    /// # Shop Tier
    ///
    /// Buys toward one level of desire tier `tier`.
    ///
    /// `level` starts at 1. Every desire is shopped. `money` is the tender
    /// still spendable. `claimed` is units already spoken for. `book` is
    /// the offers still open. `purchases` collects the buys. Returns whether
    /// every desire in the tier covers that level. An empty tier is covered.
    fn shop_tier(
        &self, market: &Market, factuals: &Factuals, tier: usize, level: f64, currency: usize,
        money: &mut f64, claimed: &mut HashMap<usize, f64>, book: &mut [Listing],
        purchases: &mut Vec<Purchase>,
    ) -> bool {
        let count = self.tier_len(tier);
        let mut covered = true;
        for index in 0..count {
            if !self.shop_desire(
                market, factuals, tier, index, level, currency, money, claimed, book, purchases,
            ) {
                covered = false;
            }
        }
        covered
    }

    /// # Shop Desire
    ///
    /// Covers one level of one desire from holdings, then from `book`.
    ///
    /// `tier` and `index` pick the desire. `level` starts at 1. Holdings and
    /// offers are taken lowest cost per satisfaction first, and a lower good
    /// id breaks a tie. Returns whether the level is covered.
    fn shop_desire(
        &self, market: &Market, factuals: &Factuals, tier: usize, index: usize, level: f64,
        currency: usize, money: &mut f64, claimed: &mut HashMap<usize, f64>, book: &mut [Listing],
        purchases: &mut Vec<Purchase>,
    ) -> bool {
        let desire = &self.desires[tier][index];
        debug_assert!(desire.amount.is_finite() && desire.amount > 0.0);
        debug_assert!(desire.satisfaction.is_finite() && desire.satisfaction >= 0.0);
        let mut remaining = desire.amount * level - desire.satisfaction;
        if remaining < 0.0 {
            remaining = 0.0;
        }
        let ranked = ranked_targets(market, desire);
        for (good, efficiency, _) in &ranked {
            if remaining <= 1e-9 {
                break;
            }
            let held = self.property.get(good).map(|row| row.quantity).unwrap_or(0.0);
            let spoken = claimed.get(good).copied().unwrap_or(0.0);
            let free = (held - spoken).max(0.0);
            let use_units = (remaining / efficiency).min(free);
            if use_units > 0.0 {
                *claimed.entry(*good).or_insert(0.0) += use_units;
                remaining -= use_units * efficiency;
            }
        }
        for (good, efficiency, _) in &ranked {
            if remaining <= 1e-9 {
                break;
            }
            // The tender cannot buy itself, and time is not a purchase.
            if *good == currency || *good == TIME {
                continue;
            }
            if !is_for_sale(factuals, *good) {
                continue;
            }
            let price = market_price(Some(market), *good);
            if !(price.is_finite() && price > 0.0) {
                continue;
            }
            // Whole units. A fraction of a unit stays unmet.
            let want = (remaining / efficiency).floor();
            let afford = (*money / price).floor();
            let mut take = want.min(afford);
            if take < 1.0 {
                continue;
            }
            for listing in book.iter_mut() {
                if take < 1.0 {
                    break;
                }
                if listing.good != *good || listing.pop == self.id {
                    continue;
                }
                let whole = listing.amount.floor();
                let bought = whole.min(take);
                if bought < 1.0 {
                    continue;
                }
                listing.amount -= bought;
                take -= bought;
                let cost = bought * price;
                *money -= cost;
                remaining -= bought * efficiency;
                *claimed.entry(*good).or_insert(0.0) += bought;
                purchases.push(Purchase {
                    seller: listing.pop,
                    good: *good,
                    units: bought,
                    cost,
                });
            }
        }
        remaining <= 1e-9
    }

    /// # Planning

    ///
    /// Sets this pop's next subsistence rate and job targets.
    ///
    /// `factuals` supplies processes, goods, and the morning time grant.
    /// `market` prices goods. A missing market prices every good at 1.
    /// Desire time aims at [`Self::time_consumed`] plus one [`PLAN_STEP_SHARE`]
    /// of the grant. Haul time aims at [`Self::transport_spent`] minus
    /// [`Self::transport_purchased`], and not below 0, plus that same share.
    /// Each hold moves toward its aim by [`rise_or_step_down`]. The two holds
    /// shrink to the grant when they ask for more than it. Each job line asks
    /// [`plan_direction`] for one step. Its current time is rounded to a whole
    /// number of steps of this grant first. A positive answer steps up one
    /// step. A negative answer steps down one step. Zero holds that step.
    /// Decreases run first. Lines that still
    /// take more time than is left are scaled down to fit, and an increase
    /// then takes only the time those decreases left free. Subsistence takes
    /// the time left. A line with no process is `Some(0)`. A grant of 0
    /// clears the plan.
    pub fn planning(&mut self, factuals: &Factuals, market: Option<&Market>) {
        // Morning time is the labor grant this plan spends.
        let budget = self.get_scaling_factor(ScalingFactor::Labor(
            factuals.config.labor.time_per_labor,
        ));
        debug_assert!(budget.is_finite() && budget >= 0.0);
        if budget == 0.0 {
            self.subsistence_rate = 0.0;
            self.held_desire_time = 0.0;
            self.held_transport_time = 0.0;
            for line in &mut self.job.lines {
                line.target = Some(0.0);
            }
            return;
        }
        debug_assert!(self.time_consumed.is_finite() && self.time_consumed >= 0.0);
        debug_assert!(self.transport_spent.is_finite() && self.transport_spent >= 0.0);
        debug_assert!(self.transport_purchased.is_finite() && self.transport_purchased >= 0.0);
        let step = PLAN_STEP_SHARE * budget;
        // Yesterday's use, plus one step so a small change still has time.
        let desire_goal = self.time_consumed + step;
        let haul = (self.transport_spent - self.transport_purchased).max(0.0);
        let transport_goal = haul + step;
        self.held_desire_time = rise_or_step_down(self.held_desire_time, desire_goal, step);
        self.held_transport_time = rise_or_step_down(self.held_transport_time, transport_goal, step);
        let mut spare = self.held_desire_time + self.held_transport_time;
        if spare > budget {
            let scale = budget / spare;
            self.held_desire_time *= scale;
            self.held_transport_time *= scale;
            spare = budget;
        }
        let remaining = budget - spare;
        let bar = subsistence_value_per_time(self, factuals, market, budget);
        // Read each line's time before the targets are replaced.
        let mut planned = Vec::new();
        for (index, line) in self.job.lines.iter().enumerate() {
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            let time_per = process.required_time();
            let direction = plan_direction(self, process, line, market, factuals, bar);
            // Whole steps of this grant, so a hold cannot drift off the increment.
            let time = (line_time(line.target, time_per) / step).round() * step;
            planned.push(LinePlan {
                index,
                time,
                time_per,
                direction,
            });
        }
        // Decreases first, so an increase can use the time they free on this same day.
        for line in &mut planned {
            if line.direction < 0 {
                line.time = (line.time - step).max(0.0);
            }
        }
        let mut used: f64 = planned.iter().map(|line| line.time).sum();
        if used > remaining && used > 0.0 {
            let scale = remaining / used;
            for line in &mut planned {
                line.time *= scale;
            }
            used = remaining;
        }
        for line in &mut planned {
            if line.direction <= 0 {
                continue;
            }
            let room = (remaining - used).max(0.0);
            let gain = step.min(room);
            line.time += gain;
            used += gain;
        }
        // A missing process stays off. A planned line overwrites its target.
        for line in &mut self.job.lines {
            line.target = Some(0.0);
        }
        for line in &planned {
            // Iterations are that line's time divided by the time one run consumes.
            let iterations = if line.time_per > 0.0 {
                line.time / line.time_per
            } else {
                line.time
            };
            self.job.lines[line.index].target = Some(iterations);
        }
        let free = (remaining - used).max(0.0);
        self.subsistence_rate = free / budget;
    }
}

/// One good this pop buys from one seller.
struct Purchase {
    seller: usize,
    good: usize,
    units: f64,
    cost: f64,
}

/// One sell offer while a pop is choosing what to buy.
struct Listing {
    pop: usize,
    good: usize,
    amount: f64,
}

/// # Buy From Offers
///
/// Pops buy from `offers` for their desires.
///
/// `pops` are the market's pops. `market` prices the goods and supplies
/// the tender. The first tender pays. `factuals` says which goods can be
/// bought. The wealthiest household shops first. Equal wealth shops in id
/// order. A desire takes the offered good with the lowest cost per
/// satisfaction. The units are whole. Loose tender pays before savings.
/// A pop does not buy its own offer. No tender buys nothing.
pub(crate) fn buy_from_offers(
    pops: &mut [Pop], market: &Market, factuals: &Factuals, offers: &mut [SellOffer],
) {
    let Some(&currency) = market.currencies.first() else {
        return;
    };
    let mut order: Vec<usize> = (0..pops.len()).collect();
    // The wealthiest household shops first. The same wealth shops in id order.
    order.sort_by(|&left, &right| {
        pops[right]
            .wealth_per_household(Some(market))
            .partial_cmp(&pops[left].wealth_per_household(Some(market)))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(pops[left].id.cmp(&pops[right].id))
    });
    for buyer_at in order {
        let purchases = pops[buyer_at].plan_purchases(market, factuals, offers, currency);
        for purchase in purchases {
            apply_purchase(pops, buyer_at, currency, &purchase, offers);
        }
    }
}

/// # Apply Purchase
///
/// Moves `purchase` from the seller to the buyer and pays `currency`.
///
/// Loose tender on the buyer pays first. Savings pays the rest. Aged
/// units leave before fresh. The seller's offer and the buyer's tender
/// offer fall by what moved. Goods received and tender received are not
/// marked fresh.
fn apply_purchase(
    pops: &mut [Pop], buyer_at: usize, currency: usize, purchase: &Purchase, offers: &mut [SellOffer],
) {
    let seller_at = pops
        .iter()
        .position(|pop| pop.id == purchase.seller)
        .expect("seller is in the market");
    debug_assert!(seller_at != buyer_at);
    let buyer_id = pops[buyer_at].id;
    let liquid = pops[buyer_at].available(currency).min(purchase.cost);
    if liquid > 0.0 {
        let row = pops[buyer_at]
            .property
            .get_mut(&currency)
            .expect("loose tender has a row");
        row.spend_aged_first(liquid);
        row.quantity -= liquid;
        reduce_offer(offers, buyer_id, currency, liquid);
    }
    let from_savings = purchase.cost - liquid;
    if from_savings > 0.0 {
        let saved = pops[buyer_at].savings.entry(currency).or_insert(0.0);
        debug_assert!(*saved + 1e-9 >= from_savings);
        *saved = (*saved - from_savings).max(0.0);
    }
    pops[seller_at].property.entry(currency).or_default().quantity += purchase.cost;
    let row = pops[seller_at]
        .property
        .get_mut(&purchase.good)
        .expect("offered good has a row");
    row.spend_aged_first(purchase.units);
    row.quantity -= purchase.units;
    debug_assert!(row.quantity + 1e-9 >= row.reserved);
    reduce_offer(offers, purchase.seller, purchase.good, purchase.units);
    pops[buyer_at].property.entry(purchase.good).or_default().quantity += purchase.units;
}

/// # Reduce Offer
///
/// Lowers the offer of `good` from `pop` by `units`.
///
/// An offer that is not listed stays absent. The amount does not fall
/// below 0.
fn reduce_offer(offers: &mut [SellOffer], pop: usize, good: usize, units: f64) {
    if let Some(offer) = offers.iter_mut().find(|offer| offer.pop == pop && offer.good == good) {
        offer.amount = (offer.amount - units).max(0.0);
    }
}

/// # Ranked Targets
///
/// `desire`'s goods, cheapest satisfaction first.
///
/// The cost is [`market_price`] divided by efficiency. A lower good id
/// breaks a tie. Returns the good, its efficiency, and that cost.
fn ranked_targets(market: &Market, desire: &Desire) -> Vec<(usize, f64, f64)> {
    let mut rows: Vec<(usize, f64, f64)> = desire
        .target
        .iter()
        .map(|target| {
            debug_assert!(target.efficiency.is_finite() && target.efficiency > 0.0);
            let price = market_price(Some(market), target.good);
            (target.good, target.efficiency, price / target.efficiency)
        })
        .collect();
    rows.sort_by(|left, right| {
        left.2
            .partial_cmp(&right.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.0.cmp(&right.0))
    });
    rows
}

/// # Is For Sale
///
/// Whether `good` can be bought.
///
/// A missing good cannot. [`Good::is_buyable`] decides a loaded good.
/// Returns that check.
fn is_for_sale(factuals: &Factuals, good: usize) -> bool {
    factuals.get_good(good).is_some_and(|good| good.is_buyable())
}

/// One job line while its next target is chosen.
struct LinePlan {
    index: usize,
    time: f64,
    time_per: f64,
    direction: i32,
}

/// Salability above this votes to expand.
///
/// A share from 0 to 1. Below [`PLAN_SALABILITY_NAY`] votes to contract.
/// The band between them casts no vote.
const PLAN_SALABILITY_YAY: f64 = 0.8;

/// Salability below this votes to contract.
const PLAN_SALABILITY_NAY: f64 = 0.4;

/// Living change that casts one vote.
///
/// A rise of this size votes to expand. A drop of this size votes to contract.
const PLAN_SOL_VOTE: f64 = 1.0;

/// # Plan Direction
///
/// One planning step for a job line: expand, hold, or contract.
///
/// `pop` supplies the output's stock, consumption, rot, and living samples.
/// `process` is the recipe. `line` supplies the iterations finished.
/// `market` prices the goods and supplies salability. `factuals` supplies
/// the default salability. `bar` is [`subsistence_value_per_time`]. The
/// votes are profit, salability, luck, decay, price durability, and living.
/// Each is +1, -1, or silent. A total above 1 returns 1. A total below -1
/// returns -1. A total between them returns 0. An expansion that does not
/// beat `bar` returns 0.
fn plan_direction(
    pop: &Pop, process: &Process, line: &JobLine, market: Option<&Market>,
    factuals: &Factuals, bar: f64,
) -> i32 {
    debug_assert!(line.completed.is_finite() && line.completed >= 0.0);
    debug_assert!(bar.is_finite());
    let profit = line_value_per_time(process, market);
    // The largest non-time output is the good these votes measure.
    let mut output_good = None;
    let mut output_amount = 0.0;
    for row in &process.outputs {
        if row.good == TIME {
            continue;
        }
        if output_good.is_none() || row.amount > output_amount {
            output_good = Some(row.good);
            output_amount = row.amount;
        }
    }
    // Units finished: the iterations times the recipe amount.
    let output = line.completed * output_amount;
    let price = output_good.map(|good| market_price(market, good)).unwrap_or(1.0);
    // Destroyed and consumed inputs of those iterations, priced in the output good.
    let mut input_value = 0.0;
    for input in &process.inputs {
        if input.good == TIME || input.is_optional() {
            continue;
        }
        let counts = matches!(input.input_type, InputType::Destroyed | InputType::Consumed);
        if !counts {
            continue;
        }
        input_value += input.amount * line.completed * market_price(market, input.good);
    }
    let input_cost = if price > 0.0 { input_value / price } else { 0.0 };
    let held = output_good.and_then(|good| pop.property.get(&good));
    let consumed = held.map(|row| row.consumed).unwrap_or(0.0);
    let decay_losses = held.map(|row| row.lost).unwrap_or(0.0);
    let stock = held.map(|row| row.quantity).unwrap_or(0.0);
    let salability = output_good
        .and_then(|good| market.and_then(|market| market.goods.get(&good)))
        .map(|good| good.salability)
        .unwrap_or(factuals.config.market.salability_default);
    // No sale is stored on the pop. Realized units are what this row consumed.
    let sold = 0.0;
    // No earlier price is stored, so a price move cannot change the tally yet.
    let price_change = 0.0;
    let sample_count = pop.sol_samples.len();
    let sol_change = if sample_count >= 2 {
        pop.sol_samples[sample_count - 1] - pop.sol_samples[sample_count - 2]
    } else {
        0.0
    };

    let mut tally = 0;
    // Profit: the recipe earns more than it spends. A loss should not gain time.
    if profit > 0.0 {
        tally += 1;
    } else if profit < 0.0 {
        tally -= 1;
    }
    // Salability: the market is taking the good. A missing record stays in the quiet band.
    if salability > PLAN_SALABILITY_YAY {
        tally += 1;
    } else if salability < PLAN_SALABILITY_NAY {
        tally -= 1;
    }
    // Luck: this run was realized less often than the market sells. A lucky day adds no vote.
    if output > 0.0 {
        let luck = (sold + consumed) / output - salability;
        if luck < 0.0 {
            tally -= 1;
        }
    }
    // Decay: use covered the inputs and the rot. Uncovered rot destroyed wealth.
    let decay = sold + consumed - input_cost - decay_losses;
    if decay > 0.0 {
        tally += 1;
    } else if decay < 0.0 {
        tally -= 1;
    }
    // Durability: surviving output is still worth more than the stock lost to price.
    let durability = price_change * stock + price * (output - decay_losses);
    if durability > 0.0 {
        tally += 1;
    } else if durability < 0.0 {
        tally -= 1;
    }
    // Living: the day moved standard of living by a whole point. A smaller swing is ordinary.
    if sol_change >= PLAN_SOL_VOTE {
        tally += 1;
    } else if sol_change <= -PLAN_SOL_VOTE {
        tally -= 1;
    }
    // One vote is not a reason to move. Two in the same direction are.
    let mut direction = if tally > 1 {
        1
    } else if tally < -1 {
        -1
    } else {
        0
    };
    // Expansion still has to beat subsistence per unit of time.
    if direction > 0 && profit <= bar {
        direction = 0;
    }
    direction
}

/// Share of the morning grant that one planning step moves.
///
/// A desire or haul hold keeps this much beyond yesterday's use.
/// A craft or labor line moves by this much of the grant.
const PLAN_STEP_SHARE: f64 = 0.05;

/// Share of the pop moved into anger and into fear for each full share of
/// basic needs left unmet.
const BASIC_UNMET_SENTIMENT: f64 = 0.04;

/// Share of the pop moved into contentment for each full share of common
/// needs met. A share above one still counts.
const COMMON_SENTIMENT: f64 = 0.02;

/// Share of the pop moved into hope and into happiness for each luxury
/// level met. Further levels still count.
const LUXURY_SENTIMENT: f64 = 0.01;

/// Share of the pop moved per point of wealth per household.
///
/// Positive wealth moves that share into hope. Negative wealth moves the
/// absolute share into fear.
const WEALTH_SENTIMENT: f64 = 0.01;

/// Market days kept for the living average.
const SOL_AVERAGE_DAYS: usize = 5;

/// Share of the pop moved per point of living above or below its recent average.
const SOL_CHANGE_SENTIMENT: f64 = 0.25;

/// Fraction of each other emotion moved into contentment.
///
/// A larger share moves more. A content pop moves nothing.
const CONTENTMENT_PULL: f64 = 0.15;

/// # Rise Or Step Down
///
/// Moves `held` toward `goal`.
///
/// A hold below `goal` becomes `goal`. A hold above `goal` falls by
/// `step` and not past `goal`. Returns the new hold.
fn rise_or_step_down(held: f64, goal: f64, step: f64) -> f64 {
    debug_assert!(held.is_finite() && held >= 0.0);
    debug_assert!(goal.is_finite() && goal >= 0.0);
    debug_assert!(step.is_finite() && step >= 0.0);
    if held < goal { goal } else { (held - step).max(goal) }
}

/// # Subsistence Value Per Time
///
/// Value of one anchor pass of subsistence per unit of time.
///
/// `pop` supplies the desires. `factuals` supplies the goods and the
/// anchor. `market` prices them. `budget` is the morning grant. The pass
/// uses [`subsistence_cover`] with the rate set to the anchor, so the
/// goods are the ones [`Pop::run_subsistence`] would add at that rate.
/// The time charged is the anchor share of `budget`. No priced goods
/// returns 0. Returns value divided by that time.
fn subsistence_value_per_time(
    pop: &Pop,
    factuals: &Factuals,
    market: Option<&Market>,
    budget: f64,
) -> f64 {
    let anchor = factuals.config.pop.subsistence_anchor;
    let common_share = factuals.config.pop.subsistence_common_share;
    let time = anchor * budget;
    debug_assert!(time.is_finite() && time > 0.0);
    let mut value = 0.0;
    for (tier, desires) in pop.desires.iter().enumerate() {
        let cover = subsistence_cover(tier, anchor, anchor, common_share);
        if cover == 0.0 {
            continue;
        }
        for desire in desires {
            debug_assert!(desire.amount.is_finite() && desire.amount >= 0.0);
            if desire.amount == 0.0 {
                continue;
            }
            for (good, amount) in subsistence_batch(factuals, desire, cover) {
                value += amount * market_price(market, good);
            }
        }
    }
    debug_assert!(value.is_finite() && value >= 0.0);
    value / time
}

/// # Line Value Per Time
///
/// Value of one job process per unit of required time.
///
/// `market` prices the goods. Good [`TIME`] is left out of both sides.
/// [`Process::required_time`] is the divisor. [`Process::is_labor`] returns
/// the non-time output value divided by that time. Any other process
/// subtracts the value of required inputs that are destroyed or consumed.
/// Optional inputs, [`InputType::Factor`], and [`InputType::Capital`] are
/// left out of that cost. No required time returns 0. Returns that value
/// per unit of time.
fn line_value_per_time(process: &Process, market: Option<&Market>) -> f64 {
    let time = process.required_time();
    if time == 0.0 {
        return 0.0;
    }
    let mut output = 0.0;
    for row in &process.outputs {
        if row.good == TIME {
            continue;
        }
        output += row.amount * market_price(market, row.good);
    }
    debug_assert!(output.is_finite() && output >= 0.0);
    if process.is_labor() {
        return output / time;
    }
    let mut cost = 0.0;
    for input in &process.inputs {
        if input.good == TIME || input.is_optional() {
            continue;
        }
        let counts = matches!(input.input_type, InputType::Destroyed | InputType::Consumed);
        if !counts {
            continue;
        }
        cost += input.amount * market_price(market, input.good);
    }
    debug_assert!(cost.is_finite() && cost >= 0.0);
    (output - cost) / time
}

/// # Line Time
///
/// Time a job target already claims.
///
/// `target` is the line's iterations. `None` is no time. `time_per` is
/// the required time of one iteration. No required time treats the target
/// as time already. Returns that time.
fn line_time(target: Option<f64>, time_per: f64) -> f64 {
    let iterations = target.unwrap_or(0.0);
    debug_assert!(iterations.is_finite() && iterations >= 0.0);
    debug_assert!(time_per.is_finite() && time_per >= 0.0);
    if time_per > 0.0 {
        iterations * time_per
    } else {
        iterations
    }
}

/// # Market Price
///
/// Currency units one unit of `good` costs on `market`.
///
/// The currency is the market's first tender. The stored rate is goods per
/// currency unit, and the price is its inverse. A missing market, tender,
/// or rate costs 1. The tender priced in itself costs 1. Returns that price.
pub fn market_price(market: Option<&Market>, good: usize) -> f64 {
    let Some(market) = market else {
        return 1.0;
    };
    let Some(&currency) = market.currencies.first() else {
        return 1.0;
    };
    if good == currency {
        return 1.0;
    }
    match market
        .exchange_rates
        .get(&currency)
        .and_then(|row| row.get(&good))
        .copied()
    {
        Some(rate) => {
            debug_assert!(rate.is_finite() && rate > 0.0, "exchange rate must be > 0");
            1.0 / rate
        }
        None => 1.0,
    }
}

/// # Quoted Rate
///
/// Goods of `good` bought by one unit of `currency` on `history`.
///
/// The rate is read from [`MarketHistory::exchange_rates`]. A missing
/// entry is 1.
fn quoted_rate(history: &MarketHistory, currency: usize, good: usize) -> f64 {
    match history
        .exchange_rates
        .iter()
        .find(|rate| rate.currency == currency && rate.good == good)
        .map(|rate| rate.rate)
    {
        Some(rate) => {
            debug_assert!(rate.is_finite() && rate > 0.0);
            rate
        }
        None => 1.0,
    }
}

/// # Subsistence Cover
///
/// The share of one desire tier that `rate` produces.
///
/// `tier` 0 is basic. Its cover is `rate` divided by `anchor`. `tier` 1 is
/// common. Its cover is the basic cover times `common_share`. Those two
/// values are [`crate::game::config::PopConfig::subsistence_anchor`] and
/// [`crate::game::config::PopConfig::subsistence_common_share`]. Any other
/// tier is 0.
fn subsistence_cover(tier: usize, rate: f64, anchor: f64, common_share: f64) -> f64 {
    debug_assert!(rate.is_finite() && rate > 0.0);
    debug_assert!(anchor.is_finite() && anchor > 0.0);
    debug_assert!(common_share.is_finite() && (0.0..=1.0).contains(&common_share));
    let basic = rate / anchor;
    match tier {
        0 => basic,
        1 => basic * common_share,
        _ => 0.0,
    }
}

/// # Subsistence Batch
///
/// The goods one desire receives at `cover`.
///
/// `cover` is the share of the desire's amount to meet. Eligible targets
/// split that satisfaction equally. A good stored in `factuals` that is
/// not industrial is eligible. A repeated good is kept once, at its first
/// efficiency. Returns each good id and the units to add.
fn subsistence_batch(factuals: &Factuals, desire: &Desire, cover: f64) -> Vec<(usize, f64)> {
    let wanted = desire.amount * cover;
    debug_assert!(wanted.is_finite() && wanted > 0.0);
    let mut eligible = Vec::new();
    for target in &desire.target {
        debug_assert!(target.efficiency.is_finite() && target.efficiency > 0.0);
        if eligible.iter().any(|(good, _)| *good == target.good) {
            continue;
        }
        let Some(good) = factuals.get_good(target.good) else {
            continue;
        };
        if good.is_industrial() {
            continue;
        }
        eligible.push((target.good, target.efficiency));
    }
    if eligible.is_empty() {
        return Vec::new();
    }
    let each = wanted / eligible.len() as f64;
    eligible
        .into_iter()
        .map(|(good, efficiency)| (good, each / efficiency))
        .collect()
}

#[cfg(test)]
mod start_day_should {
    use super::Pop;
    use crate::game::scalingfactor::ScalingFactor;

    #[test]
    fn each_morning_adds_to_quantity_and_produced() {
        let mut pop = Pop::new(1);
        let added = pop.start_day(&[(0, ScalingFactor::Fixed(4.0))]);
        assert_eq!(added, vec![(0, 4.0)]);
        let row = pop.property[&0];
        assert_eq!(row.quantity, 4.0);
        assert_eq!(row.produced, 4.0);
        assert_eq!(row.fresh, 0.0);
        pop.start_day(&[(0, ScalingFactor::Fixed(4.0))]);
        let row = pop.property[&0];
        assert_eq!(row.quantity, 8.0);
        assert_eq!(row.produced, 8.0);
        assert_eq!(row.fresh, 0.0);
    }
}

#[cfg(test)]
mod run_subsistence_should {
    use std::collections::HashSet;

    use super::*;
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{Desire, DesireTarget, DesireTargetType};
    use crate::game::factuals::Factuals;
    use crate::game::good::{Good, GoodTag};
    use crate::game::pop_property::PopPRow;
    use crate::game::scalingfactor::ScalingFactor;

    fn near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{actual} != {expected}"
        );
    }

    fn plain_good(id: usize, industrial: bool) -> Good {
        let mut tags = HashSet::new();
        if industrial {
            tags.insert(GoodTag::Industrial);
        }
        Good {
            id,
            name: format!("good {id}"),
            class: None,
            decay_rate: 0.0,
            decay_result: Default::default(),
            mass: 1.0,
            volume: 1.0,
            tags,
            categories: vec![],
        }
    }

    fn desire(amount: f64, targets: Vec<DesireTarget>) -> Desire {
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 0,
            target: targets,
            amount,
            satisfaction: 0.0,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        }
    }

    fn target(good: usize, efficiency: f64) -> DesireTarget {
        DesireTarget::new(good, DesireTargetType::Consume, efficiency)
    }

    fn world() -> Factuals {
        let mut factuals = Factuals::new();
        for id in [TIME, 1, 2, 4] {
            factuals.goods.insert(id, plain_good(id, false));
        }
        factuals.goods.insert(3, plain_good(3, true));
        factuals
    }

    /// Basic goods 1 and 2, common good 4, luxury good 1, and 10 time.
    fn stocked() -> Pop {
        let mut pop = Pop::new(7);
        pop.subsistence_rate = 0.6;
        pop.property.insert(TIME, PopPRow::new(10.0));
        pop.desires[0].push(desire(12.0, vec![target(1, 1.0), target(2, 2.0)]));
        pop.desires[1].push(desire(8.0, vec![target(4, 1.0)]));
        pop.desires[2].push(desire(10.0, vec![target(1, 1.0)]));
        pop
    }

    fn quantity(pop: &Pop, good: usize) -> f64 {
        pop.property.get(&good).map(|row| row.quantity).unwrap_or(0.0)
    }

    #[test]
    fn anchor_rate_spends_time_and_covers_basic_and_half_of_common() {
        let mut pop = stocked();
        pop.run_subsistence(&world());
        let time = pop.property.get(&TIME).unwrap();
        near(time.quantity, 4.0);
        near(time.spent, 6.0);
        // Basic 12 is split into 6 satisfaction each. Efficiency 2 needs half the units.
        near(quantity(&pop, 1), 6.0);
        near(quantity(&pop, 2), 3.0);
        near(pop.property.get(&1).unwrap().produced, 6.0);
        near(pop.property.get(&1).unwrap().fresh, 6.0);
        // Common is half of 8. Luxury does not add more of good 1.
        near(quantity(&pop, 4), 4.0);
        near(pop.desires[0][0].satisfaction, 0.0);
    }

    #[test]
    fn half_the_anchor_rate_makes_half_the_goods() {
        let mut pop = stocked();
        pop.subsistence_rate = 0.3;
        pop.run_subsistence(&world());
        near(quantity(&pop, TIME), 7.0);
        near(quantity(&pop, 1), 3.0);
        near(quantity(&pop, 2), 1.5);
        near(quantity(&pop, 4), 2.0);
    }

    #[test]
    fn an_industrial_good_is_left_out_and_the_other_good_takes_the_desire() {
        let mut pop = Pop::new(1);
        pop.subsistence_rate = 0.6;
        pop.property.insert(TIME, PopPRow::new(10.0));
        pop.desires[0].push(desire(10.0, vec![target(1, 1.0), target(3, 1.0)]));
        pop.run_subsistence(&world());
        near(quantity(&pop, 1), 10.0);
        assert!(pop.property.get(&3).is_none());
    }

    #[test]
    fn a_missing_good_is_left_out() {
        let mut pop = Pop::new(1);
        pop.subsistence_rate = 0.6;
        pop.desires[0].push(desire(10.0, vec![target(1, 1.0), target(99, 1.0)]));
        pop.run_subsistence(&world());
        near(quantity(&pop, 1), 10.0);
        assert!(pop.property.get(&99).is_none());
    }

    #[test]
    fn a_non_positive_rate_changes_nothing() {
        let mut pop = stocked();
        pop.subsistence_rate = 0.0;
        pop.run_subsistence(&world());
        near(quantity(&pop, TIME), 10.0);
        assert!(pop.property.get(&1).is_none());
    }

}

#[cfg(test)]
mod split_goods_sale_reserve_should {
    use super::*;
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{Desire, DesireTarget, DesireTargetType};
    use crate::game::factuals::Factuals;
    use crate::game::job::{Job, JobLine};
    use crate::game::pop_property::PopPRow;
    use crate::game::process::{InputType, Process, ProcessInput};
    use crate::game::scalingfactor::ScalingFactor;

    fn near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{actual} != {expected}"
        );
    }

    fn desire(amount: f64, targets: Vec<DesireTarget>) -> Desire {
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 0,
            target: targets,
            amount,
            satisfaction: 0.0,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        }
    }

    fn target(good: usize, efficiency: f64) -> DesireTarget {
        DesireTarget::new(good, DesireTargetType::Consume, efficiency)
    }

    fn stock(pop: &mut Pop, good: usize, quantity: f64) {
        pop.property.insert(good, PopPRow::new(quantity));
    }

    fn reserved(pop: &Pop, good: usize) -> f64 {
        pop.property.get(&good).map(|row| row.reserved).unwrap_or(0.0)
    }

    fn quantity(pop: &Pop, good: usize) -> f64 {
        pop.property.get(&good).map(|row| row.quantity).unwrap_or(0.0)
    }

    fn saved(pop: &Pop, good: usize) -> f64 {
        pop.savings.get(&good).copied().unwrap_or(0.0)
    }

    fn offered(offers: &[SellOffer], good: usize) -> f64 {
        offers
            .iter()
            .find(|offer| offer.good == good)
            .map(|offer| offer.amount)
            .unwrap_or(0.0)
    }

    fn history(coins: &[usize]) -> MarketHistory {
        let mut history = MarketHistory::new();
        history.currencies.extend(coins);
        history
    }

    fn world(processes: Vec<Process>) -> Factuals {
        let mut factuals = Factuals::new();
        for process in processes {
            factuals.processes.insert(process.id, process);
        }
        factuals
    }

    fn line(process: usize, target: Option<f64>, inputs: Vec<usize>) -> JobLine {
        JobLine::new(process, target, inputs)
    }

    fn input(good: usize, amount: f64, kind: InputType, optional: bool) -> ProcessInput {
        ProcessInput::new(good, amount, false, kind, optional)
    }

    fn work(id: usize, inputs: Vec<ProcessInput>) -> Process {
        let mut process = Process::new(id, "work", 0);
        for input in inputs {
            process = process.with_input(input);
        }
        process
    }

    /// Good 1 is the plan's input. Good 5 is only held.
    #[test]
    fn a_fixed_plan_reserves_its_inputs_and_offers_the_rest() {
        let mut pop = Pop::new(4);
        pop.job = Job::new([line(1, Some(3.0), Vec::new()), line(0, Some(0.0), Vec::new())]);
        pop.property
            .insert(1, PopPRow::new(10.0).with_reserve(7.0));
        stock(&mut pop, 5, 4.0);
        let factuals = world(vec![work(
            1,
            vec![input(1, 2.0, InputType::Destroyed, false)],
        )]);
        let offers = pop.split_goods_sale_reserve(&history(&[]), &factuals);
        near(reserved(&pop, 1), 6.0);
        near(offered(&offers, 1), 4.0);
        near(offered(&offers, 5), 4.0);
        assert_eq!(offers.iter().map(|offer| offer.good).collect::<Vec<_>>(), vec![1, 5]);
        assert!(offers.iter().all(|offer| offer.pop == 4));
    }

    #[test]
    fn an_open_plan_stops_at_the_scarcer_required_input() {
        let mut pop = Pop::new(1);
        pop.job = Job::new([line(1, None, Vec::new()), line(0, Some(0.0), Vec::new())]);
        stock(&mut pop, 1, 10.0);
        stock(&mut pop, 2, 10.0);
        let factuals = world(vec![work(
            1,
            vec![
                input(1, 1.0, InputType::Destroyed, false),
                input(2, 4.0, InputType::Destroyed, false),
            ],
        )]);
        let offers = pop.split_goods_sale_reserve(&history(&[]), &factuals);
        near(reserved(&pop, 1), 2.5);
        near(reserved(&pop, 2), 10.0);
        near(offered(&offers, 1), 7.5);
        near(offered(&offers, 2), 0.0);
    }

    #[test]
    fn a_zero_plan_reserves_nothing() {
        let mut pop = Pop::new(1);
        pop.job = Job::new([line(1, Some(0.0), Vec::new()), line(0, Some(0.0), Vec::new())]);
        stock(&mut pop, 1, 5.0);
        let factuals = world(vec![work(
            1,
            vec![input(1, 1.0, InputType::Destroyed, false)],
        )]);
        let offers = pop.split_goods_sale_reserve(&history(&[]), &factuals);
        near(reserved(&pop, 1), 0.0);
        near(offered(&offers, 1), 5.0);
    }

    #[test]
    fn an_optional_input_is_reserved_only_when_the_line_lists_it() {
        let factuals = world(vec![work(
            1,
            vec![
                input(1, 1.0, InputType::Destroyed, false),
                input(2, 3.0, InputType::Destroyed, true),
            ],
        )]);
        let mut quiet = Pop::new(1);
        quiet.job = Job::new([line(1, Some(2.0), Vec::new()), line(0, Some(0.0), Vec::new())]);
        stock(&mut quiet, 1, 10.0);
        stock(&mut quiet, 2, 10.0);
        quiet.split_goods_sale_reserve(&history(&[]), &factuals);
        near(reserved(&quiet, 1), 2.0);
        near(reserved(&quiet, 2), 0.0);

        let mut listed = Pop::new(1);
        listed.job = Job::new([line(1, Some(2.0), vec![2]), line(0, Some(0.0), Vec::new())]);
        stock(&mut listed, 1, 10.0);
        stock(&mut listed, 2, 10.0);
        listed.split_goods_sale_reserve(&history(&[]), &factuals);
        near(reserved(&listed, 1), 2.0);
        near(reserved(&listed, 2), 6.0);
    }

    #[test]
    fn an_open_plan_does_not_let_an_optional_input_limit_the_run() {
        let mut pop = Pop::new(1);
        pop.job = Job::new([line(1, None, vec![2]), line(0, Some(0.0), Vec::new())]);
        stock(&mut pop, 1, 4.0);
        stock(&mut pop, 2, 1.0);
        let factuals = world(vec![work(
            1,
            vec![
                input(1, 1.0, InputType::Destroyed, false),
                input(2, 1.0, InputType::Destroyed, true),
            ],
        )]);
        pop.split_goods_sale_reserve(&history(&[]), &factuals);
        near(reserved(&pop, 1), 4.0);
        near(reserved(&pop, 2), 1.0);
    }

    #[test]
    fn a_factor_is_reserved_once() {
        let mut pop = Pop::new(1);
        pop.job = Job::new([line(1, Some(4.0), Vec::new()), line(0, Some(0.0), Vec::new())]);
        stock(&mut pop, 1, 20.0);
        stock(&mut pop, 3, 20.0);
        let factuals = world(vec![work(
            1,
            vec![
                input(1, 1.0, InputType::Destroyed, false),
                input(3, 2.0, InputType::Factor, false),
            ],
        )]);
        pop.split_goods_sale_reserve(&history(&[]), &factuals);
        near(reserved(&pop, 1), 4.0);
        near(reserved(&pop, 3), 2.0);
    }

    #[test]
    fn a_later_line_reserves_what_the_first_line_left() {
        let mut pop = Pop::new(1);
        pop.job = Job::new([line(1, Some(3.0), Vec::new()), line(2, Some(3.0), Vec::new())]);
        stock(&mut pop, 1, 5.0);
        let factuals = world(vec![
            work(1, vec![input(1, 1.0, InputType::Destroyed, false)]),
            work(2, vec![input(1, 1.0, InputType::Destroyed, false)]),
        ]);
        pop.split_goods_sale_reserve(&history(&[]), &factuals);
        near(reserved(&pop, 1), 5.0);
    }

    #[test]
    fn the_most_efficient_good_is_reserved_first() {
        let mut pop = Pop::new(1);
        stock(&mut pop, 1, 100.0);
        stock(&mut pop, 2, 2.0);
        pop.desires[0].push(desire(10.0, vec![target(1, 1.0), target(2, 2.0)]));
        pop.split_goods_sale_reserve(&history(&[]), &world(vec![]));
        // Good 2 covers 4 satisfaction. Good 1 covers the other 6.
        near(reserved(&pop, 2), 2.0);
        near(reserved(&pop, 1), 6.0);
    }

    #[test]
    fn a_cap_sends_the_rest_of_the_level_to_the_next_good() {
        let mut pop = Pop::new(1);
        stock(&mut pop, 1, 100.0);
        stock(&mut pop, 2, 100.0);
        pop.desires[0].push(desire(
            10.0,
            vec![target(1, 1.0), target(2, 2.0).with_cap(0.5)],
        ));
        pop.split_goods_sale_reserve(&history(&[]), &world(vec![]));
        near(reserved(&pop, 2), 2.5);
        near(reserved(&pop, 1), 5.0);
    }

    #[test]
    fn an_efficiency_tie_reserves_the_lower_good_id() {
        let mut pop = Pop::new(1);
        stock(&mut pop, 5, 10.0);
        stock(&mut pop, 2, 10.0);
        pop.desires[0].push(desire(4.0, vec![target(5, 1.0), target(2, 1.0)]));
        pop.split_goods_sale_reserve(&history(&[]), &world(vec![]));
        near(reserved(&pop, 2), 4.0);
        near(reserved(&pop, 5), 0.0);
    }

    #[test]
    fn an_unfinished_basic_tier_stops_before_savings_and_common() {
        let mut pop = Pop::new(1);
        stock(&mut pop, 1, 4.0);
        stock(&mut pop, 4, 10.0);
        stock(&mut pop, 9, 5.0);
        pop.desires[0].push(desire(10.0, vec![target(1, 1.0)]));
        pop.desires[1].push(desire(3.0, vec![target(4, 1.0)]));
        let offers = pop.split_goods_sale_reserve(&history(&[9]), &world(vec![]));
        near(reserved(&pop, 1), 4.0);
        near(reserved(&pop, 4), 0.0);
        near(saved(&pop, 9), 0.0);
        near(quantity(&pop, 9), 5.0);
        near(offered(&offers, 4), 10.0);
        near(offered(&offers, 9), 5.0);
        near(offered(&offers, 1), 0.0);
    }

    #[test]
    fn savings_reaches_three_basic_days_and_counts_coins_already_held() {
        let mut pop = Pop::new(1);
        // One basic day costs 4. Seven is already more than one day and less than three.
        pop.savings.insert(9, 7.0);
        stock(&mut pop, 1, 4.0);
        stock(&mut pop, 4, 10.0);
        stock(&mut pop, 9, 5.4);
        stock(&mut pop, 8, 0.5);
        pop.desires[0].push(desire(4.0, vec![target(1, 1.0)]));
        pop.desires[1].push(desire(3.0, vec![target(4, 1.0)]));
        let offers = pop.split_goods_sale_reserve(&history(&[9, 8]), &world(vec![]));
        near(reserved(&pop, 1), 4.0);
        near(reserved(&pop, 4), 3.0);
        near(saved(&pop, 9), 12.0);
        near(saved(&pop, 8), 0.0);
        near(pop.effective_savings, 12.0);
        near(quantity(&pop, 9), 0.4);
        near(quantity(&pop, 8), 0.5);
        near(offered(&offers, 4), 7.0);
        near(offered(&offers, 9), 0.4);
    }

    #[test]
    fn basic_then_common_saves_three_days_in_total() {
        let mut pop = Pop::new(1);
        stock(&mut pop, 1, 2.0);
        stock(&mut pop, 9, 20.0);
        pop.desires[0].push(desire(2.0, vec![target(1, 1.0)]));
        pop.split_goods_sale_reserve(&history(&[9]), &world(vec![]));
        // One day costs 2. Common's total is 3 days, not 1 plus another 3.
        near(saved(&pop, 9), 6.0);
        near(quantity(&pop, 9), 14.0);
        near(pop.effective_savings, 6.0);
    }

    #[test]
    fn saving_fresh_coins_leaves_fresh_inside_what_remains() {
        // Ten coins, eight of them fresh. Saving six takes the two aged coins and four fresh.
        let mut pop = Pop::new(1);
        stock(&mut pop, 1, 2.0);
        pop.property.insert(9, PopPRow::new(10.0).with_fresh(8.0));
        pop.desires[0].push(desire(2.0, vec![target(1, 1.0)]));
        pop.split_goods_sale_reserve(&history(&[9]), &world(vec![]));
        near(saved(&pop, 9), 6.0);
        near(quantity(&pop, 9), 4.0);
        near(pop.property[&9].fresh, 4.0);
    }

    #[test]
    fn a_coin_that_buys_two_goods_covers_a_day_in_half_the_units() {
        let mut pop = Pop::new(1);
        stock(&mut pop, 1, 4.0);
        stock(&mut pop, 9, 10.0);
        pop.desires[0].push(desire(4.0, vec![target(1, 1.0)]));
        let mut history = history(&[9]);
        history.exchange_rates.push(crate::game::market::ExchangeRate {
            currency: 9,
            good: 1,
            rate: 2.0,
        });
        pop.split_goods_sale_reserve(&history, &world(vec![]));
        // A day costs 4 goods. At 2 goods per coin that is 2 coins, and the total is 3 days.
        near(saved(&pop, 9), 6.0);
        near(pop.effective_savings, 6.0);
    }

    #[test]
    fn a_short_coin_stack_is_saved_and_common_still_reserves() {
        let mut pop = Pop::new(1);
        stock(&mut pop, 1, 4.0);
        stock(&mut pop, 4, 3.0);
        stock(&mut pop, 9, 0.5);
        pop.desires[0].push(desire(4.0, vec![target(1, 1.0)]));
        pop.desires[1].push(desire(3.0, vec![target(4, 1.0)]));
        pop.split_goods_sale_reserve(&history(&[9]), &world(vec![]));
        near(saved(&pop, 9), 0.5);
        near(quantity(&pop, 9), 0.0);
        near(reserved(&pop, 4), 3.0);
    }

    #[test]
    fn satisfaction_already_covering_basic_reserves_nothing() {
        let mut pop = Pop::new(1);
        stock(&mut pop, 1, 10.0);
        stock(&mut pop, 4, 3.0);
        stock(&mut pop, 9, 5.0);
        let mut basic = desire(4.0, vec![target(1, 1.0)]);
        basic.satisfaction = 4.0;
        pop.desires[0].push(basic);
        pop.desires[1].push(desire(3.0, vec![target(4, 1.0)]));
        pop.split_goods_sale_reserve(&history(&[9]), &world(vec![]));
        near(reserved(&pop, 1), 0.0);
        near(reserved(&pop, 4), 3.0);
        // One day costs 4. The stack can cover that day and one coin toward the three-day total.
        near(saved(&pop, 9), 5.0);
        near(quantity(&pop, 9), 0.0);
    }

    #[test]
    fn luxury_and_savings_alternate_until_the_coin_runs_out() {
        let mut pop = Pop::new(1);
        stock(&mut pop, 1, 1.0);
        stock(&mut pop, 4, 1.0);
        stock(&mut pop, 6, 100.0);
        stock(&mut pop, 9, 6.0);
        pop.desires[0].push(desire(1.0, vec![target(1, 1.0)]));
        pop.desires[1].push(desire(1.0, vec![target(4, 1.0)]));
        pop.desires[2].push(desire(4.0, vec![target(6, 1.0)]));
        let offers = pop.split_goods_sale_reserve(&history(&[9]), &world(vec![]));
        // One day costs 1. The totals are 1, then 3, then 5, then 7.
        // Six coins reach 5 and one coin of the 7, so the third luxury level does not start.
        near(reserved(&pop, 6), 8.0);
        near(saved(&pop, 9), 6.0);
        near(quantity(&pop, 9), 0.0);
        near(offered(&offers, 6), 92.0);
    }

    #[test]
    fn luxury_stops_when_the_next_level_cannot_be_finished() {
        let mut pop = Pop::new(1);
        stock(&mut pop, 1, 1.0);
        stock(&mut pop, 4, 1.0);
        stock(&mut pop, 6, 4.0);
        stock(&mut pop, 9, 100.0);
        pop.desires[0].push(desire(1.0, vec![target(1, 1.0)]));
        pop.desires[1].push(desire(1.0, vec![target(4, 1.0)]));
        pop.desires[2].push(desire(4.0, vec![target(6, 1.0)]));
        pop.split_goods_sale_reserve(&history(&[9]), &world(vec![]));
        // The finished luxury level holds the total at 5 days. The next level does not start.
        near(reserved(&pop, 6), 4.0);
        near(saved(&pop, 9), 5.0);
        near(quantity(&pop, 9), 95.0);
    }

    #[test]
    fn a_finished_luxury_level_opens_the_next_levels_caps() {
        let mut pop = Pop::new(1);
        stock(&mut pop, 1, 1.0);
        stock(&mut pop, 4, 1.0);
        stock(&mut pop, 2, 3.0);
        stock(&mut pop, 3, 100.0);
        stock(&mut pop, 9, 5.0);
        pop.desires[0].push(desire(1.0, vec![target(1, 1.0)]));
        pop.desires[1].push(desire(1.0, vec![target(4, 1.0)]));
        pop.desires[2].push(desire(
            10.0,
            vec![target(3, 1.0), target(2, 2.0)],
        ));
        pop.split_goods_sale_reserve(&history(&[9]), &world(vec![]));
        // Level 1: good 2 covers 6, good 3 covers 4. Level 2 gets a fresh cap, so good 3 covers 10 more.
        // Five coins reach the 5-day total, so that second level is reserved, and the 7-day total is not.
        near(reserved(&pop, 2), 3.0);
        near(reserved(&pop, 3), 14.0);
        near(saved(&pop, 9), 5.0);
    }
}

#[cfg(test)]
mod produce_should {
    use super::*;
    use crate::game::desire::{Desire, DesireTarget, DesireTargetType};
    use crate::game::demographic_source::DemographicSource;
    use crate::game::effects::ProcessEffect;
    use crate::game::factuals::Factuals;
    use crate::game::job::{Job, JobLine};
    use crate::game::pop_property::PopPRow;
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput};
    use crate::game::scalingfactor::ScalingFactor;

    fn near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{actual} != {expected}"
        );
    }

    fn line(process: usize, target: Option<f64>, inputs: Vec<usize>) -> JobLine {
        JobLine::new(process, target, inputs)
    }

    fn input(good: usize, amount: f64, kind: InputType, optional: bool) -> ProcessInput {
        ProcessInput::new(good, amount, false, kind, optional)
    }

    fn work(id: usize, inputs: Vec<ProcessInput>, output: Option<(usize, f64)>) -> Process {
        let mut process = Process::new(id, "work", 0);
        for input in inputs {
            process = process.with_input(input);
        }
        if let Some((good, amount)) = output {
            process = process.with_output(ProcessOutput::new(good, amount, false));
        }
        process
    }

    fn world(processes: Vec<Process>) -> Factuals {
        let mut factuals = Factuals::new();
        for process in processes {
            factuals.processes.insert(process.id, process);
        }
        factuals
    }

    fn row(pop: &Pop, good: usize) -> PopPRow {
        pop.property.get(&good).copied().unwrap_or_default()
    }

    #[test]
    fn a_plan_spends_its_input_and_records_the_output() {
        let mut pop = Pop::new(1);
        pop.job = Job::new([line(1, Some(3.0), Vec::new()), line(0, Some(0.0), Vec::new())]);
        pop.property
            .insert(1, PopPRow::new(10.0).with_reserve(8.0).with_fresh(8.0));
        let factuals = world(vec![work(
            1,
            vec![input(1, 2.0, InputType::Destroyed, false)],
            Some((2, 5.0)),
        )]);
        pop.produce(&factuals);
        let spent = row(&pop, 1);
        // Two aged units go first, so four of the eight fresh units remain.
        near(spent.quantity, 4.0);
        near(spent.spent, 6.0);
        near(spent.fresh, 4.0);
        near(spent.reserved, 2.0);
        let made = row(&pop, 2);
        near(made.quantity, 15.0);
        near(made.produced, 15.0);
        near(made.fresh, 15.0);
    }

    #[test]
    fn an_idle_line_does_not_run_subsistence() {
        let mut pop = Pop::new(1);
        pop.subsistence_rate = 0.6;
        pop.property.insert(TIME, PopPRow::new(10.0));
        pop.property.insert(1, PopPRow::new(10.0));
        pop.desires[0].push(Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 0,
            target: vec![DesireTarget::new(4, DesireTargetType::Consume, 1.0)],
            amount: 12.0,
            satisfaction: 0.0,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        });
        pop.job = Job::new([line(1, Some(0.0), Vec::new()), line(9, None, Vec::new())]);
        let factuals = world(vec![work(
            1,
            vec![input(1, 1.0, InputType::Destroyed, false)],
            Some((2, 1.0)),
        )]);
        pop.produce(&factuals);
        near(row(&pop, TIME).quantity, 10.0);
        near(row(&pop, 1).quantity, 10.0);
        assert!(pop.property.get(&2).is_none());
        assert!(pop.property.get(&4).is_none());
    }

    #[test]
    fn the_second_line_uses_what_the_first_line_left() {
        let mut pop = Pop::new(1);
        pop.job = Job::new([line(1, Some(2.0), Vec::new()), line(2, None, Vec::new())]);
        pop.property.insert(1, PopPRow::new(10.0));
        let factuals = world(vec![
            work(
                1,
                vec![input(1, 2.0, InputType::Destroyed, false)],
                Some((3, 1.0)),
            ),
            work(
                2,
                vec![input(3, 1.0, InputType::Destroyed, false)],
                Some((4, 1.0)),
            ),
        ]);
        pop.produce(&factuals);
        near(row(&pop, 1).quantity, 6.0);
        near(row(&pop, 3).quantity, 0.0);
        near(row(&pop, 3).spent, 2.0);
        near(row(&pop, 4).quantity, 2.0);
        near(row(&pop, 4).produced, 2.0);
    }

    #[test]
    fn an_unlisted_optional_does_not_limit_the_run() {
        let mut pop = Pop::new(1);
        pop.job = Job::new([line(1, None, Vec::new()), line(0, Some(0.0), Vec::new())]);
        pop.property.insert(1, PopPRow::new(4.0));
        pop.property.insert(5, PopPRow::new(1.0));
        let factuals = world(vec![work(
            1,
            vec![
                input(1, 1.0, InputType::Destroyed, false),
                input(5, 1.0, InputType::Destroyed, true),
            ],
            Some((2, 1.0)),
        )]);
        pop.produce(&factuals);
        near(row(&pop, 1).quantity, 0.0);
        near(row(&pop, 5).quantity, 1.0);
        near(row(&pop, 2).quantity, 4.0);
    }

    #[test]
    fn a_listed_optional_is_drawn() {
        let mut pop = Pop::new(1);
        pop.job = Job::new([line(1, None, vec![5]), line(0, Some(0.0), Vec::new())]);
        pop.property.insert(1, PopPRow::new(4.0));
        pop.property.insert(5, PopPRow::new(1.0));
        let factuals = world(vec![work(
            1,
            vec![
                input(1, 1.0, InputType::Destroyed, false),
                input(5, 1.0, InputType::Destroyed, true),
            ],
            Some((2, 1.0)),
        )]);
        pop.produce(&factuals);
        // The optional is spent while it lasts. It does not stop the rest of the run.
        near(row(&pop, 1).quantity, 0.0);
        near(row(&pop, 5).quantity, 0.0);
        near(row(&pop, 5).spent, 1.0);
        near(row(&pop, 2).quantity, 4.0);
    }

    #[test]
    fn capital_is_set_aside_as_used() {
        let mut pop = Pop::new(1);
        pop.job = Job::new([line(1, Some(3.0), Vec::new()), line(0, Some(0.0), Vec::new())]);
        pop.property
            .insert(7, PopPRow::new(10.0).with_reserve(6.0).with_fresh(10.0));
        let factuals = world(vec![work(
            1,
            vec![input(7, 2.0, InputType::Capital, false)],
            None,
        )]);
        pop.produce(&factuals);
        let tool = row(&pop, 7);
        near(tool.quantity, 4.0);
        near(tool.used, 6.0);
        near(tool.spent, 0.0);
        near(tool.reserved, 0.0);
        near(tool.fresh, 4.0);
    }

    #[test]
    fn a_missing_required_factor_runs_nothing() {
        let mut pop = Pop::new(1);
        pop.job = Job::new([line(1, Some(2.0), Vec::new()), line(0, Some(0.0), Vec::new())]);
        pop.property.insert(TIME, PopPRow::new(5.0));
        let factuals = world(vec![work(
            1,
            vec![
                input(8, 1.0, InputType::Factor, false),
                input(TIME, 1.0, InputType::Destroyed, false),
            ],
            Some((9, 1.0)),
        )]);
        pop.produce(&factuals);
        near(row(&pop, TIME).quantity, 5.0);
        near(row(&pop, TIME).spent, 0.0);
        assert!(pop.property.get(&9).is_none());
    }

    #[test]
    fn a_good_that_is_spent_and_made_records_the_output() {
        let mut pop = Pop::new(1);
        pop.job = Job::new([line(1, Some(2.0), Vec::new()), line(0, Some(0.0), Vec::new())]);
        pop.property.insert(1, PopPRow::new(10.0));
        let factuals = world(vec![work(
            1,
            vec![input(1, 1.0, InputType::Destroyed, false)],
            Some((1, 3.0)),
        )]);
        pop.produce(&factuals);
        let good = row(&pop, 1);
        near(good.quantity, 14.0);
        near(good.spent, 2.0);
        near(good.produced, 6.0);
        near(good.fresh, 6.0);
    }

    #[test]
    fn a_process_effect_is_stored() {
        let mut pop = Pop::new(1);
        pop.job = Job::new([line(1, Some(2.0), Vec::new()), line(0, Some(0.0), Vec::new())]);
        let process = work(1, vec![], Some((2, 1.0))).with_effect(ProcessEffect::Development(4.0));
        pop.produce(&world(vec![process]));
        // Two iterations scale the yield of 4.
        assert_eq!(pop.stored_effects, vec![PopEffect::Development(8.0)]);
        near(row(&pop, 2).quantity, 2.0);
    }
}

#[cfg(test)]
mod consume_should {
    use std::collections::{HashMap, HashSet};

    use super::*;
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{Desire, DesireTarget, DesireTargetType};
    use crate::game::effects::DesireEffect;
    use crate::game::factuals::Factuals;
    use crate::game::good::Good;
    use crate::game::pop_property::PopPRow;
    use crate::game::scalingfactor::ScalingFactor;

    fn near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{actual} != {expected}"
        );
    }

    fn desire(amount: f64, targets: Vec<DesireTarget>) -> Desire {
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 0,
            target: targets,
            amount,
            satisfaction: 0.0,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        }
    }

    fn target(good: usize, efficiency: f64) -> DesireTarget {
        DesireTarget::new(good, DesireTargetType::Consume, efficiency)
    }

    fn plain(id: usize) -> Good {
        Good {
            id,
            name: format!("good {id}"),
            class: None,
            decay_rate: 0.0,
            decay_result: HashMap::new(),
            mass: 1.0,
            volume: 1.0,
            tags: HashSet::new(),
            categories: vec![],
        }
    }

    fn world(goods: Vec<Good>) -> Factuals {
        let mut factuals = Factuals::new();
        for good in goods {
            factuals.goods.insert(good.id, good);
        }
        factuals
    }

    fn row(pop: &Pop, good: usize) -> PopPRow {
        pop.property.get(&good).copied().unwrap_or_default()
    }

    #[test]
    fn each_good_reaches_its_minimum_before_efficiency_fills_the_rest() {
        let mut pop = Pop::new(1);
        pop.desires[0].push(desire(10.0, vec![target(1, 1.0), target(2, 2.0)]));
        pop.property
            .insert(1, PopPRow::new(10.0).with_reserve(8.0).with_fresh(8.0));
        pop.property.insert(2, PopPRow::new(10.0));
        pop.consume(&world(vec![plain(1), plain(2)]));
        // Two goods: minimum 2.5 each, cap 7.5. Good 2 then takes the other 5.
        // Good 1 spends two aged units and half a fresh unit.
        near(row(&pop, 1).quantity, 7.5);
        near(row(&pop, 1).consumed, 2.5);
        near(row(&pop, 1).fresh, 7.5);
        near(row(&pop, 1).reserved, 5.5);
        near(row(&pop, 2).quantity, 6.25);
        near(row(&pop, 2).consumed, 3.75);
        near(pop.desires[0][0].satisfaction, 10.0);
    }

    #[test]
    fn an_unfinished_basic_tier_does_not_consume_common() {
        let mut pop = Pop::new(1);
        pop.desires[0].push(desire(10.0, vec![target(1, 1.0)]));
        pop.desires[1].push(desire(5.0, vec![target(2, 1.0)]));
        pop.property
            .insert(1, PopPRow::new(6.0).with_reserve(4.0).with_fresh(2.0));
        pop.property.insert(2, PopPRow::new(5.0));
        pop.consume(&world(vec![plain(1), plain(2)]));
        let basic = row(&pop, 1);
        // Four aged units go first, so the two fresh units go as well.
        near(basic.quantity, 0.0);
        near(basic.consumed, 6.0);
        near(basic.fresh, 0.0);
        near(basic.reserved, 0.0);
        near(pop.desires[0][0].satisfaction, 6.0);
        near(row(&pop, 2).quantity, 5.0);
        near(row(&pop, 2).consumed, 0.0);
        near(pop.desires[1][0].satisfaction, 0.0);
    }

    #[test]
    fn one_unmet_basic_desire_blocks_the_next_tier() {
        let mut pop = Pop::new(1);
        pop.desires[0].push(desire(2.0, vec![target(1, 1.0)]));
        pop.desires[0].push(desire(2.0, vec![target(2, 1.0)]));
        pop.desires[1].push(desire(5.0, vec![target(3, 1.0)]));
        pop.property.insert(1, PopPRow::new(2.0));
        pop.property.insert(3, PopPRow::new(5.0));
        pop.consume(&world(vec![]));
        near(pop.desires[0][0].satisfaction, 2.0);
        near(pop.desires[0][1].satisfaction, 0.0);
        near(row(&pop, 3).quantity, 5.0);
    }

    #[test]
    fn an_empty_basic_tier_opens_common() {
        let mut pop = Pop::new(1);
        pop.desires[1].push(desire(4.0, vec![target(2, 1.0)]));
        pop.property.insert(2, PopPRow::new(4.0));
        pop.consume(&world(vec![]));
        near(row(&pop, 2).quantity, 0.0);
        near(pop.desires[1][0].satisfaction, 4.0);
    }

    #[test]
    fn satisfaction_already_held_opens_the_next_tier() {
        let mut pop = Pop::new(1);
        pop.desires[0].push(desire(10.0, vec![target(1, 1.0)]));
        pop.desires[0][0].satisfaction = 10.0;
        pop.desires[1].push(desire(4.0, vec![target(2, 1.0)]));
        pop.property.insert(1, PopPRow::new(10.0));
        pop.property.insert(2, PopPRow::new(4.0));
        pop.consume(&world(vec![]));
        near(row(&pop, 1).quantity, 10.0);
        near(row(&pop, 2).quantity, 0.0);
        near(pop.desires[1][0].satisfaction, 4.0);
    }

    #[test]
    fn the_least_efficient_good_still_receives_its_minimum() {
        let mut pop = Pop::new(1);
        pop.desires[0].push(desire(
            12.0,
            vec![target(1, 10.0), target(2, 5.0), target(3, 1.0)],
        ));
        pop.property.insert(1, PopPRow::new(100.0));
        pop.property.insert(2, PopPRow::new(100.0));
        pop.property.insert(3, PopPRow::new(100.0));
        pop.consume(&world(vec![]));
        // Three goods: minimum 2, cap 8. Good 1 takes the rest after the minimums.
        near(row(&pop, 1).consumed, 0.8);
        near(row(&pop, 2).consumed, 0.4);
        near(row(&pop, 3).consumed, 2.0);
        near(pop.desires[0][0].satisfaction, 12.0);
    }

    #[test]
    fn one_good_stops_at_the_bucket_cap() {
        let mut pop = Pop::new(1);
        pop.desires[0].push(desire(8.0, vec![target(1, 1.0), target(2, 1.0)]));
        pop.desires[1].push(desire(4.0, vec![target(3, 1.0)]));
        pop.property.insert(1, PopPRow::new(100.0));
        pop.property.insert(3, PopPRow::new(4.0));
        pop.consume(&world(vec![]));
        // Two goods: cap 6. The missing good leaves the level short, so common stays.
        near(row(&pop, 1).consumed, 6.0);
        near(pop.desires[0][0].satisfaction, 6.0);
        near(row(&pop, 3).quantity, 4.0);
        near(pop.desires[1][0].satisfaction, 0.0);
    }

    #[test]
    fn an_efficiency_tie_consumes_the_lower_good_id() {
        let mut pop = Pop::new(1);
        pop.desires[0].push(desire(4.0, vec![target(2, 1.0), target(1, 1.0)]));
        pop.property.insert(1, PopPRow::new(2.0));
        pop.property.insert(2, PopPRow::new(10.0));
        pop.consume(&world(vec![]));
        near(row(&pop, 1).quantity, 0.0);
        near(row(&pop, 2).quantity, 8.0);
        near(pop.desires[0][0].satisfaction, 4.0);
    }

    #[test]
    fn a_consumed_good_adds_its_decay_results() {
        let mut pop = Pop::new(1);
        pop.desires[0].push(desire(4.0, vec![target(1, 1.0)]));
        pop.property.insert(1, PopPRow::new(4.0));
        let mut good = plain(1);
        good.decay_result.insert(7, 0.5);
        pop.consume(&world(vec![good]));
        near(row(&pop, 1).quantity, 0.0);
        near(row(&pop, 1).consumed, 4.0);
        near(row(&pop, 7).quantity, 2.0);
        near(row(&pop, 7).produced, 0.0);
    }

    #[test]
    fn a_use_target_is_set_aside() {
        let mut pop = Pop::new(1);
        pop.desires[0].push(desire(
            4.0,
            vec![DesireTarget::new(1, DesireTargetType::Use, 1.0)],
        ));
        pop.property.insert(1, PopPRow::new(10.0).with_fresh(10.0));
        let mut good = plain(1);
        good.decay_result.insert(7, 1.0);
        pop.consume(&world(vec![good]));
        let tool = row(&pop, 1);
        near(tool.quantity, 6.0);
        near(tool.used, 4.0);
        near(tool.consumed, 0.0);
        near(tool.fresh, 6.0);
        assert!(pop.property.get(&7).is_none());
        near(pop.desires[0][0].satisfaction, 4.0);
    }

    #[test]
    fn luxury_repeats_until_a_level_cannot_be_finished() {
        let mut pop = Pop::new(1);
        pop.desires[0].push(desire(1.0, vec![target(1, 1.0)]));
        pop.desires[0][0].satisfaction = 1.0;
        pop.desires[1].push(desire(1.0, vec![target(2, 1.0)]));
        pop.desires[1][0].satisfaction = 1.0;
        pop.desires[2].push(desire(4.0, vec![target(3, 1.0).with_cap(1.0)]));
        pop.property.insert(1, PopPRow::new(5.0));
        pop.property.insert(3, PopPRow::new(10.0));
        pop.consume(&world(vec![]));
        // Two full levels take eight. The third level takes the two that remain.
        near(row(&pop, 1).quantity, 5.0);
        near(row(&pop, 3).quantity, 0.0);
        near(row(&pop, 3).consumed, 10.0);
        near(pop.desires[2][0].satisfaction, 10.0);
    }

    #[test]
    fn an_unfinished_common_tier_does_not_consume_luxury() {
        let mut pop = Pop::new(1);
        pop.desires[0].push(desire(1.0, vec![target(1, 1.0)]));
        pop.desires[1].push(desire(4.0, vec![target(2, 1.0)]));
        pop.desires[2].push(desire(4.0, vec![target(3, 1.0)]));
        pop.property.insert(1, PopPRow::new(1.0));
        pop.property.insert(2, PopPRow::new(1.0));
        pop.property.insert(3, PopPRow::new(4.0));
        pop.consume(&world(vec![]));
        near(pop.desires[0][0].satisfaction, 1.0);
        near(pop.desires[1][0].satisfaction, 1.0);
        near(row(&pop, 3).quantity, 4.0);
        near(pop.desires[2][0].satisfaction, 0.0);
    }

    #[test]
    fn a_met_desire_records_its_bonus() {
        let mut pop = Pop::new(1);
        pop.desires[0].push(desire(4.0, vec![target(1, 1.0)]));
        pop.desires[0][0].effect.push(DesireEffect::Development(5.0, true));
        pop.desires[0][0]
            .effect
            .push(DesireEffect::Satisfaction(2.0, true));
        pop.property.insert(1, PopPRow::new(4.0));
        pop.consume(&world(vec![plain(1)]));
        assert_eq!(pop.stored_effects, vec![PopEffect::Development(5.0)]);
    }

    #[test]
    fn an_unmet_desire_records_its_malus() {
        let mut pop = Pop::new(1);
        pop.desires[0].push(desire(4.0, vec![target(1, 1.0)]));
        pop.desires[0][0].effect.push(DesireEffect::Birthrate(0.2, false));
        pop.consume(&world(vec![]));
        assert_eq!(pop.stored_effects, vec![PopEffect::Birthrate(-0.2)]);
    }

    #[test]
    fn a_half_met_bonus_keeps_half_its_rate() {
        let mut pop = Pop::new(1);
        pop.desires[0].push(desire(4.0, vec![target(1, 1.0)]));
        pop.desires[0][0].effect.push(DesireEffect::Development(8.0, true));
        pop.property.insert(1, PopPRow::new(2.0));
        pop.consume(&world(vec![plain(1)]));
        assert_eq!(pop.stored_effects, vec![PopEffect::Development(4.0)]);
    }

    #[test]
    fn an_unsatisfied_bonus_is_left_out() {
        let mut pop = Pop::new(1);
        pop.desires[0].push(desire(4.0, vec![target(1, 1.0)]));
        pop.desires[0][0].effect.push(DesireEffect::Development(5.0, true));
        pop.consume(&world(vec![]));
        assert!(pop.stored_effects.is_empty());
    }

    #[test]
    fn luxury_development_keeps_levels_past_the_first() {
        let mut pop = Pop::new(1);
        pop.desires[2].push(desire(4.0, vec![target(1, 1.0)]));
        pop.desires[2][0].satisfaction = 8.0;
        pop.desires[2][0].effect.push(DesireEffect::Development(3.0, true));
        pop.consume(&world(vec![]));
        assert_eq!(pop.stored_effects, vec![PopEffect::Development(6.0)]);
    }

    #[test]
    fn common_development_stops_at_one_level() {
        let mut pop = Pop::new(1);
        pop.desires[1].push(desire(4.0, vec![target(1, 1.0)]));
        pop.desires[1][0].satisfaction = 8.0;
        pop.desires[1][0].effect.push(DesireEffect::Development(3.0, true));
        pop.consume(&world(vec![]));
        assert_eq!(pop.stored_effects, vec![PopEffect::Development(3.0)]);
    }
}

#[cfg(test)]
mod growth_phase_should {
    use super::{Pop, PopEffect};
    use crate::game::factuals::Factuals;
    use crate::game::household::{DemographicRates, Household, HouseholdTarget};
    use crate::game::sentiment::Sentiment;
    use crate::game::species::Species;

    fn tick(household: Household, rates: &DemographicRates, factuals: &Factuals) -> Household {
        let mut household = household;
        household.update(rates, factuals.config.pop.turn_length, &factuals.config.pop);
        household
    }

    fn pop_at(count: f64) -> Pop {
        let mut pop = Pop::new(1);
        pop.demographics.household = Household::with_count(count);
        pop
    }

    #[test]
    fn a_content_pop_follows_the_baseline_for_one_year() {
        let factuals = Factuals::new();
        let mut pop = pop_at(10.0);
        let expected = tick(pop.demographics.household, &DemographicRates::baseline(), &factuals);
        pop.growth_phase(&factuals);
        assert_eq!(pop.demographics.household, expected);
    }

    #[test]
    fn a_zero_species_bundle_keeps_the_baseline_partnership() {
        let factuals = Factuals::new().with_species(Species::new(0, "human"));
        let mut pop = pop_at(10.0);
        pop.demographics.species = 0;
        let expected = tick(pop.demographics.household, &DemographicRates::baseline(), &factuals);
        pop.growth_phase(&factuals);
        assert_eq!(pop.demographics.household, expected);
    }

    #[test]
    fn hope_scales_births_per_woman() {
        let factuals = Factuals::new().with_species(Species::new(0, "human"));
        let mut pop = pop_at(10.0);
        pop.demographics.species = 0;
        pop.sentiment = Sentiment::from_parts(0.0, 0.0, 0.0, 0.0, 1.0);
        let mut rates = DemographicRates::baseline();
        rates.birth_per_woman *= 1.25;
        let expected = tick(pop.demographics.household, &rates, &factuals);
        pop.growth_phase(&factuals);
        assert_eq!(pop.demographics.household, expected);
    }

    #[test]
    fn stored_birth_and_mortality_are_applied_and_removed() {
        let factuals = Factuals::new();
        let mut pop = pop_at(10.0);
        pop.stored_effects = vec![
            PopEffect::Birthrate(0.1),
            PopEffect::Mortality(HouseholdTarget::INFANTS, 0.2),
            PopEffect::Development(2.0),
        ];
        let mut rates = DemographicRates::baseline();
        rates.birth_per_woman += 0.1;
        rates.apply_mortality(HouseholdTarget::INFANTS, 0.2);
        let expected = tick(pop.demographics.household, &rates, &factuals);
        pop.growth_phase(&factuals);
        assert_eq!(pop.demographics.household, expected);
        assert_eq!(pop.stored_effects, vec![PopEffect::Development(2.0)]);
    }

    #[test]
    fn a_pop_under_one_household_skips_the_tick() {
        let factuals = Factuals::new();
        let mut pop = pop_at(0.5);
        pop.stored_effects = vec![PopEffect::Birthrate(0.1)];
        pop.growth_phase(&factuals);
        assert_eq!(pop.demographics.household.count, 0.5);
        assert_eq!(pop.stored_effects, vec![PopEffect::Birthrate(0.1)]);
    }

    #[test]
    fn turn_length_comes_from_config() {
        let mut factuals = Factuals::new();
        factuals.config.pop.turn_length = 4.0;
        let mut pop = pop_at(10.0);
        let expected = tick(pop.demographics.household, &DemographicRates::baseline(), &factuals);
        pop.growth_phase(&factuals);
        assert_eq!(pop.demographics.household, expected);
    }

    #[test]
    fn growth_records_the_population_before_the_tick() {
        let factuals = Factuals::new();
        let mut pop = pop_at(10.0);
        let before = pop.demographics.total_population();
        pop.growth_phase(&factuals);
        assert_eq!(pop.pre_growth_population, before);
    }
}

#[cfg(test)]
mod apply_demographic_changes_should {
    use super::Pop;
    use crate::game::culture::Culture;
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{DemoDesire, Desire, DesireEffect, DesireTarget, DesireTargetType};
    use crate::game::factuals::Factuals;
    use crate::game::household::Household;
    use crate::game::scalingfactor::ScalingFactor;
    use crate::game::species::Species;

    fn target(good: usize, efficiency: f64) -> DesireTarget {
        DesireTarget::new(good, DesireTargetType::Consume, efficiency)
    }

    fn pop_desire(source: DemographicSource, goods: Vec<DesireTarget>, amount: f64) -> Desire {
        Desire {
            source,
            demo_desire_id: 1,
            priority: 3,
            target: goods,
            amount,
            satisfaction: 4.0,
            category: None,
            effect: vec![DesireEffect::Satisfaction(2.0, true)],
            scalar: ScalingFactor::Household(1.0),
            decay: 0.0,
        }
    }

    #[test]
    fn a_changed_amount_is_scaled_and_satisfaction_stays() {
        let demo = DemoDesire::new(1)
            .with_tier(0)
            .with_scalar(ScalingFactor::Household(1.0))
            .with_good(target(1, 1.0))
            .with_amount(2.0);
        let factuals = Factuals::new().with_species(Species::new(0, "human").with_desire(demo));
        let mut pop = Pop::new(1);
        pop.demographics.species = 0;
        pop.demographics.household = Household::with_count(10.0);
        pop.desires[0].push(pop_desire(
            DemographicSource::Species(0),
            vec![target(1, 1.0)],
            10.0,
        ));
        pop.apply_demographic_changes(&factuals);
        assert_eq!(pop.desires[0][0].amount, 20.0);
        assert_eq!(pop.desires[0][0].satisfaction, 4.0);
        assert_eq!(
            pop.desires[0][0].effect,
            vec![DesireEffect::Satisfaction(2.0, true)]
        );
        assert_eq!(pop.desires[0][0].priority, 3);
    }

    #[test]
    fn a_changed_bucket_replaces_the_goods() {
        let demo = DemoDesire::new(1)
            .with_tier(0)
            .with_good(target(1, 1.0))
            .with_good(target(3, 4.0));
        let factuals = Factuals::new().with_species(Species::new(0, "human").with_desire(demo));
        let mut pop = Pop::new(1);
        pop.desires[0].push(pop_desire(
            DemographicSource::Species(0),
            vec![target(1, 9.0), target(2, 9.0)],
            1.0,
        ));
        pop.apply_demographic_changes(&factuals);
        assert_eq!(pop.desires[0][0].target[0].good, 1);
        assert_eq!(pop.desires[0][0].target[0].efficiency, 1.0);
        assert_eq!(pop.desires[0][0].target[1].good, 3);
        assert_eq!(pop.desires[0][0].target[1].efficiency, 4.0);
        assert_eq!(pop.desires[0][0].satisfaction, 4.0);
    }

    #[test]
    fn a_reordered_bucket_is_replaced() {
        let demo = DemoDesire::new(1)
            .with_tier(0)
            .with_good(target(2, 1.0))
            .with_good(target(1, 6.0));
        let factuals = Factuals::new().with_species(Species::new(0, "human").with_desire(demo));
        let mut pop = Pop::new(1);
        pop.desires[0].push(pop_desire(
            DemographicSource::Species(0),
            vec![target(1, 9.0), target(2, 9.0)],
            1.0,
        ));
        pop.apply_demographic_changes(&factuals);
        assert_eq!(pop.desires[0][0].target[0].good, 2);
        assert_eq!(pop.desires[0][0].target[1].good, 1);
        assert_eq!(pop.desires[0][0].target[1].efficiency, 6.0);
    }

    #[test]
    fn the_same_goods_keep_the_pop_bucket() {
        let demo = DemoDesire::new(1)
            .with_tier(0)
            .with_good(target(1, 1.0))
            .with_good(target(2, 1.0));
        let factuals = Factuals::new().with_species(Species::new(0, "human").with_desire(demo));
        let mut pop = Pop::new(1);
        pop.desires[0].push(pop_desire(
            DemographicSource::Species(0),
            vec![target(1, 9.0), target(2, 8.0)],
            1.0,
        ));
        pop.apply_demographic_changes(&factuals);
        assert_eq!(pop.desires[0][0].target[0].efficiency, 9.0);
        assert_eq!(pop.desires[0][0].target[1].efficiency, 8.0);
        assert_eq!(pop.desires[0][0].amount, 1.0);
    }

    #[test]
    fn a_missing_demographic_desire_is_left_as_it_is() {
        let factuals = Factuals::new();
        let mut pop = Pop::new(1);
        pop.desires[1].push(pop_desire(
            DemographicSource::Culture(4),
            vec![target(1, 3.0)],
            6.0,
        ));
        pop.apply_demographic_changes(&factuals);
        assert_eq!(pop.desires[1][0].amount, 6.0);
        assert_eq!(pop.desires[1][0].target[0].efficiency, 3.0);
    }

    #[test]
    fn a_culture_desire_scales_with_the_pop() {
        let demo = DemoDesire::new(1)
            .with_tier(1)
            .with_scalar(ScalingFactor::Household(1.0))
            .with_good(target(5, 1.0))
            .with_amount(3.0);
        let factuals = Factuals::new().with_culture(Culture::new(2, "river").with_desire(demo));
        let mut pop = Pop::new(1);
        pop.demographics.household = Household::with_count(4.0);
        pop.desires[1].push(pop_desire(
            DemographicSource::Culture(2),
            vec![target(5, 1.0)],
            4.0,
        ));
        pop.apply_demographic_changes(&factuals);
        assert_eq!(pop.desires[1][0].amount, 12.0);
    }
}

#[cfg(test)]
mod decay_should {
    use std::collections::{HashMap, HashSet};

    use super::Pop;
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::Desire;
    use crate::game::effects::PopEffect;
    use crate::game::factuals::Factuals;
    use crate::game::good::{Good, GoodTag, TIME};
    use crate::game::pop_property::PopPRow;
    use crate::game::scalingfactor::ScalingFactor;

    fn near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{actual} != {expected}"
        );
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

    fn desire(amount: f64, satisfaction: f64, decay: f64) -> Desire {
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 1,
            target: vec![],
            amount,
            satisfaction,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Household(1.0),
            decay,
        }
    }

    #[test]
    fn aging_stock_rots_and_fresh_stays() {
        let mut factuals = Factuals::new();
        factuals.goods.insert(1, good(1, 0.25));
        let mut pop = Pop::new(1);
        pop.property.insert(
            1,
            PopPRow::new(10.0)
                .with_fresh(6.0)
                .with_consumed(3.0)
                .with_reserve(2.0),
        );
        pop.decay_goods(&factuals);
        let row = pop.property.get(&1).unwrap();
        near(row.quantity, 9.0);
        near(row.fresh, 6.0);
        near(row.lost, 1.0);
        near(row.consumed, 3.0);
        near(row.reserved, 2.0);
    }

    #[test]
    fn a_zero_rate_keeps_the_stock() {
        let mut factuals = Factuals::new();
        factuals.goods.insert(1, good(1, 0.0));
        let mut pop = Pop::new(1);
        pop.property.insert(1, PopPRow::new(10.0));
        pop.decay_goods(&factuals);
        let row = pop.property.get(&1).unwrap();
        near(row.quantity, 10.0);
        near(row.lost, 0.0);
    }

    #[test]
    fn excess_time_is_lost() {
        let mut factuals = Factuals::new();
        factuals.goods.insert(TIME, good(TIME, 1.0));
        let mut pop = Pop::new(1);
        pop.property.insert(TIME, PopPRow::new(12.0));
        pop.decay_goods(&factuals);
        let row = pop.property.get(&TIME).unwrap();
        near(row.quantity, 0.0);
        near(row.lost, 12.0);
    }

    #[test]
    fn an_owned_exposure_good_stays_whole() {
        let mut kind = good(1, 1.0);
        kind.tags.insert(GoodTag::Exposure);
        let mut factuals = Factuals::new();
        factuals.goods.insert(1, kind);
        let mut pop = Pop::new(1);
        pop.property.insert(1, PopPRow::new(10.0));
        pop.decay_goods(&factuals);
        let row = pop.property.get(&1).unwrap();
        near(row.quantity, 10.0);
        near(row.lost, 0.0);
    }

    #[test]
    fn rot_credits_the_decay_result() {
        let mut kind = good(1, 1.0);
        kind.decay_result.insert(2, 0.5);
        let mut factuals = Factuals::new();
        factuals.goods.insert(1, kind);
        let mut pop = Pop::new(1);
        pop.property.insert(1, PopPRow::new(4.0));
        pop.decay_goods(&factuals);
        near(pop.property.get(&1).unwrap().quantity, 0.0);
        near(pop.property.get(&1).unwrap().lost, 4.0);
        near(pop.property.get(&2).unwrap().quantity, 2.0);
    }

    #[test]
    fn used_returns_and_then_rots() {
        let mut factuals = Factuals::new();
        factuals.goods.insert(1, good(1, 0.5));
        let mut pop = Pop::new(1);
        pop.property
            .insert(1, PopPRow::new(4.0).with_fresh(2.0).with_used(6.0));
        pop.decay_goods(&factuals);
        let row = pop.property.get(&1).unwrap();
        near(row.quantity, 6.0);
        near(row.used, 0.0);
        near(row.fresh, 2.0);
        near(row.lost, 4.0);
    }

    #[test]
    fn a_bonus_good_is_added_after_the_rot() {
        let mut factuals = Factuals::new();
        factuals.goods.insert(4, good(4, 1.0));
        let mut pop = Pop::new(1);
        pop.property.insert(4, PopPRow::new(10.0));
        pop.stored_effects = vec![
            PopEffect::BonusGood { good: 4, amount: 1.5 },
            PopEffect::Development(2.0),
        ];
        pop.decay_goods(&factuals);
        let row = pop.property.get(&4).unwrap();
        near(row.quantity, 1.5);
        near(row.lost, 10.0);
        assert_eq!(pop.stored_effects, vec![PopEffect::Development(2.0)]);
    }

    #[test]
    fn satisfaction_is_kept_by_one_minus_decay() {
        let mut pop = Pop::new(1);
        pop.desires[0].push(desire(10.0, 10.0, 0.2));
        pop.desires[1].push(desire(8.0, 4.0, 1.0));
        pop.desires[2].push(desire(6.0, 3.0, 0.0));
        pop.decay_desires();
        near(pop.desires[0][0].satisfaction, 8.0);
        near(pop.desires[1][0].satisfaction, 0.0);
        near(pop.desires[2][0].satisfaction, 3.0);
        assert_eq!(pop.desires[0][0].amount, 10.0);
        assert_eq!(pop.desires[1][0].amount, 8.0);
        assert_eq!(pop.desires[2][0].amount, 6.0);
    }
}

#[cfg(test)]
mod rescale_should {
    use super::Pop;
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{Desire, DesireTarget, DesireTargetType};
    use crate::game::household::Household;
    use crate::game::job::JobLine;
    use crate::game::scalingfactor::ScalingFactor;

    fn near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{actual} != {expected}"
        );
    }

    fn desire(amount: f64, satisfaction: f64) -> Desire {
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 1,
            target: vec![DesireTarget::new(1, DesireTargetType::Consume, 9.0)],
            amount,
            satisfaction,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Household(1.0),
            decay: 0.0,
        }
    }

    fn pop_of(count: f64, recorded: f64) -> Pop {
        let mut pop = Pop::new(1);
        pop.demographics.household = Household::with_count(count);
        pop.pre_growth_population = recorded;
        pop
    }

    #[test]
    fn a_larger_pop_raises_plans_and_desire_amounts() {
        let mut pop = pop_of(3.0, 10.0);
        pop.job.lines[0] = JobLine::new(1, Some(4.0), vec![]);
        pop.job.lines[1] = JobLine::new(2, None, vec![]);
        pop.desires[0].push(desire(20.0, 8.0));
        pop.desires[1].push(desire(10.0, 4.0));
        pop.rescale();
        near(pop.job.lines[0].target.unwrap(), 6.0);
        assert!(pop.job.lines[1].target.is_none());
        near(pop.desires[0][0].amount, 30.0);
        near(pop.desires[0][0].satisfaction, 8.0);
        near(pop.desires[0][0].target[0].efficiency, 9.0);
        near(pop.desires[1][0].amount, 15.0);
        near(pop.pre_growth_population, 15.0);
        pop.rescale();
        near(pop.job.lines[0].target.unwrap(), 6.0);
        near(pop.desires[0][0].amount, 30.0);
    }

    #[test]
    fn a_smaller_pop_lowers_them() {
        let mut pop = pop_of(1.0, 10.0);
        pop.job.lines[0] = JobLine::new(1, Some(2.0), vec![]);
        pop.job.lines[1] = JobLine::new(2, Some(0.0), vec![]);
        pop.desires[0].push(desire(8.0, 3.0));
        pop.rescale();
        near(pop.job.lines[0].target.unwrap(), 1.0);
        near(pop.job.lines[1].target.unwrap(), 0.0);
        near(pop.desires[0][0].amount, 4.0);
        near(pop.desires[0][0].satisfaction, 3.0);
    }

    #[test]
    fn the_same_population_keeps_the_values() {
        let mut pop = pop_of(2.0, 10.0);
        pop.job.lines[0] = JobLine::new(1, Some(4.0), vec![]);
        pop.desires[0].push(desire(20.0, 8.0));
        pop.rescale();
        near(pop.job.lines[0].target.unwrap(), 4.0);
        near(pop.desires[0][0].amount, 20.0);
        near(pop.desires[0][0].satisfaction, 8.0);
    }
}

#[cfg(test)]
mod record_keeping_should {
    use super::Pop;
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::Desire;
    use crate::game::pop_property::PopPRow;
    use crate::game::scalingfactor::ScalingFactor;

    fn desire(satisfaction: f64) -> Desire {
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 1,
            target: vec![],
            amount: 10.0,
            satisfaction,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Household(1.0),
            decay: 0.0,
        }
    }

    #[test]
    fn satisfaction_is_summed_and_the_day_flows_clear() {
        let mut pop = Pop::new(1);
        pop.desires[0].push(desire(4.0));
        pop.desires[2].push(desire(6.0));
        pop.property.insert(
            1,
            PopPRow::new(7.0)
                .with_produced(4.0)
                .with_consumed(1.0)
                .with_spent(2.0)
                .with_lost(0.5)
                .with_fresh(3.0)
                .with_reserve(2.0)
                .with_used(1.0),
        );
        pop.record_keeping();
        assert_eq!(pop.records.satisfaction, 10.0);
        let row = pop.property[&1];
        assert_eq!(row.quantity, 7.0);
        assert_eq!(row.produced, 0.0);
        assert_eq!(row.consumed, 0.0);
        assert_eq!(row.spent, 0.0);
        assert_eq!(row.lost, 0.0);
        assert_eq!(row.fresh, 0.0);
        assert_eq!(row.reserved, 0.0);
        assert_eq!(row.used, 1.0);
    }

    #[test]
    fn a_pop_with_no_desires_is_fully_satisfied() {
        let mut pop = Pop::new(1);
        pop.record_keeping();
        assert!(pop.records.satisfaction.is_infinite());
    }
}

#[cfg(test)]
mod plan_direction_should {
    use super::{plan_direction, Pop, PopPRow};
    use crate::game::factuals::Factuals;
    use crate::game::good::TIME;
    use crate::game::job::JobLine;
    use crate::game::market::{Market, MarketGood};
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput};

    /// # Craft
    ///
    /// One iteration takes 1 time and, when `input` is above 0, destroys that
    /// much of good 2. It makes `output` of good 1. At a price of 1 the profit
    /// per unit of time is `output - input`.
    fn craft(output: f64, input: f64) -> Process {
        let mut process = Process::new(1, "craft", 0)
            .with_input(ProcessInput::new(TIME, 1.0, false, InputType::Destroyed, false));
        if input > 0.0 {
            process = process.with_input(ProcessInput::new(2, input, false, InputType::Destroyed, false));
        }
        process.with_output(ProcessOutput::new(1, output, false))
    }

    /// # Line
    ///
    /// A job line that finished `completed` iterations of process 1.
    fn line(completed: f64) -> JobLine {
        let mut line = JobLine::new(1, Some(0.0), vec![]);
        line.completed = completed;
        line
    }

    /// # Held
    ///
    /// A pop whose good 1 row has this stock, use, and rot.
    ///
    /// `living` is the previous standard of living and the latest one. The
    /// change is the latest minus the previous. `None` leaves no samples.
    fn held(stock: f64, consumed: f64, lost: f64, living: Option<(f64, f64)>) -> Pop {
        let mut pop = Pop::new(1);
        if stock != 0.0 || consumed != 0.0 || lost != 0.0 {
            pop.property.insert(1, PopPRow::new(stock).with_consumed(consumed).with_lost(lost));
        }
        if let Some((earlier, later)) = living {
            pop.sol_samples.push_back(earlier);
            pop.sol_samples.push_back(later);
        }
        pop
    }

    /// # Market For
    ///
    /// A market where good 1 has this salability.
    fn market_for(salability: f64) -> Market {
        let mut market = Market::new(1);
        market.goods.insert(1, MarketGood::new().with_salability(salability));
        market
    }

    /// # Ask
    ///
    /// The planning direction for this pop, recipe, and finished iterations.
    ///
    /// `market` prices the goods and supplies salability. `None` uses the
    /// default salability. `bar` is the subsistence value an expansion has to
    /// beat. Returns +1, 0, or -1.
    fn ask(
        pop: &Pop, process: &Process, completed: f64, market: Option<&Market>, bar: f64,
    ) -> i32 {
        plan_direction(pop, process, &line(completed), market, &Factuals::new(), bar)
    }

    #[test]
    fn one_vote_holds() {
        let quiet = Pop::new(1);
        // Nothing has been made, so luck, rot, and price stay quiet. Paper profit is the only yay.
        assert_eq!(ask(&quiet, &craft(4.0, 0.0), 0.0, None, 0.0), 0);
        // A paper loss is the only nay.
        assert_eq!(ask(&quiet, &craft(1.0, 2.0), 0.0, None, 0.0), 0);
        // A rise under one point stays quiet, so paper profit is still the only vote.
        let small = held(0.0, 0.0, 0.0, Some((0.0, 0.9)));
        assert_eq!(ask(&small, &craft(4.0, 0.0), 0.0, None, 0.0), 0);
        // A whole point either way is one vote on a break-even recipe, and one vote holds.
        let rose = held(0.0, 0.0, 0.0, Some((0.0, 1.0)));
        assert_eq!(ask(&rose, &craft(1.0, 1.0), 0.0, None, 0.0), 0);
        let fell = held(0.0, 0.0, 0.0, Some((1.0, 0.0)));
        assert_eq!(ask(&fell, &craft(1.0, 1.0), 0.0, None, 0.0), 0);
    }

    #[test]
    fn two_votes_move() {
        // Profit and a whole point of living are two yays, and the recipe beats the bar.
        let rose = held(0.0, 0.0, 0.0, Some((0.0, 1.0)));
        assert_eq!(ask(&rose, &craft(4.0, 0.0), 0.0, None, 0.0), 1);
        // A paper loss and a whole point drop are two nays.
        let fell = held(0.0, 0.0, 0.0, Some((1.0, 0.0)));
        assert_eq!(ask(&fell, &craft(1.0, 2.0), 0.0, None, 0.0), -1);
        // The market is taking the good. Nothing has been made, so luck and rot stay quiet.
        let hot = market_for(0.81);
        assert_eq!(ask(&Pop::new(1), &craft(4.0, 0.0), 0.0, Some(&hot), 0.0), 1);
        // A cold market joins a paper loss.
        let cold = market_for(0.39);
        assert_eq!(ask(&Pop::new(1), &craft(1.0, 2.0), 0.0, Some(&cold), 0.0), -1);
    }

    #[test]
    fn salability_outside_the_band_votes() {
        let pop = Pop::new(1);
        let even = craft(1.0, 1.0);
        // The edges of the band are silent. A break-even line with no run holds.
        let high_edge = market_for(0.8);
        assert_eq!(ask(&pop, &even, 0.0, Some(&high_edge), 0.0), 0);
        let low_edge = market_for(0.4);
        assert_eq!(ask(&pop, &even, 0.0, Some(&low_edge), 0.0), 0);
        // One step outside the band is a single vote, which still holds.
        let hot = market_for(0.81);
        assert_eq!(ask(&pop, &even, 0.0, Some(&hot), 0.0), 0);
        let cold = market_for(0.39);
        assert_eq!(ask(&pop, &even, 0.0, Some(&cold), 0.0), 0);
        // No market record uses the default, and that default sits inside the band.
        assert_eq!(ask(&pop, &craft(4.0, 0.0), 0.0, None, 0.0), 0);
    }

    #[test]
    fn luck_votes_only_when_short_of_the_market() {
        let idle = Pop::new(1);
        // Nothing of the run was used, so luck nays. The output is still worth its price, so
        // durability yays. Those two cancel profit, and the line holds.
        assert_eq!(ask(&idle, &craft(10.0, 0.0), 1.0, None, 0.0), 0);
        // The whole run was used, which beats the market, so luck adds no yay. Rot matches the
        // run and that use covers it, so durability and decay stay quiet. Profit alone holds.
        let lucky = held(0.0, 10.0, 10.0, None);
        assert_eq!(ask(&lucky, &craft(10.0, 0.0), 1.0, None, 0.0), 0);
        // No iterations finished, so luck is skipped. A pile is not a sale, and profit alone holds.
        let piled = held(100.0, 0.0, 0.0, None);
        assert_eq!(ask(&piled, &craft(10.0, 0.0), 0.0, None, 0.0), 0);
    }

    #[test]
    fn decay_and_price_vote_on_sign() {
        // The output good was used and nothing rotted. No iterations finished, so luck and
        // durability stay quiet. That use yays beside profit, and the line expands.
        let used = held(0.0, 5.0, 0.0, None);
        assert_eq!(ask(&used, &craft(4.0, 0.0), 0.0, None, 0.0), 1);
        // Break-even, rot equal to the run, and use equal to the market's share. Luck and
        // durability are even. The input plus the rot still exceed that use, so decay is the
        // only nay and the line holds.
        let short = held(0.0, 5.0, 10.0, None);
        assert_eq!(ask(&short, &craft(10.0, 10.0), 1.0, None, 0.0), 0);
        // The same run, with use covering the input and the rot. Decay is the only yay and holds.
        let covered = held(0.0, 21.0, 10.0, None);
        assert_eq!(ask(&covered, &craft(10.0, 10.0), 1.0, None, 0.0), 0);
        // The pile lost more than the run made. Luck, decay, and durability nay, and profit
        // cannot hold them, so the line contracts.
        let rotted = held(0.0, 0.0, 15.0, None);
        assert_eq!(ask(&rotted, &craft(10.0, 0.0), 1.0, None, 0.0), -1);
        // No earlier price is stored, so a price move is 0. A pile with no run and no rot,
        // on a break-even recipe, does not vote.
        let stock = held(100.0, 0.0, 0.0, None);
        assert_eq!(ask(&stock, &craft(1.0, 1.0), 0.0, None, 0.0), 0);
    }

    #[test]
    fn expansion_must_beat_subsistence() {
        // Two yays would expand, but the recipe does not beat subsistence per unit of time.
        let rose = held(0.0, 0.0, 0.0, Some((0.0, 1.0)));
        assert_eq!(ask(&rose, &craft(4.0, 0.0), 0.0, None, 1000.0), 0);
        // A contraction does not have to beat that bar.
        let fell = held(0.0, 0.0, 0.0, Some((1.0, 0.0)));
        assert_eq!(ask(&fell, &craft(1.0, 2.0), 0.0, None, 1000.0), -1);
    }

    #[test]
    fn the_largest_output_is_measured() {
        // Good 3 is listed first and time is largest, but good 1 is the largest non-time output.
        // The use sits on good 1, so decay yays beside profit. Measuring either other good would
        // leave decay quiet and the line would hold.
        let process = Process::new(1, "craft", 0)
            .with_input(ProcessInput::new(TIME, 1.0, false, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 4.0, false))
            .with_output(ProcessOutput::new(TIME, 99.0, false))
            .with_output(ProcessOutput::new(1, 10.0, false));
        let used = held(0.0, 5.0, 0.0, None);
        assert_eq!(ask(&used, &process, 0.0, None, 0.0), 1);
    }
}

#[cfg(test)]
mod planning_should {
    use std::collections::HashMap;

    use super::{Pop, PopPRow};
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{Desire, DesireTarget, DesireTargetType};
    use crate::game::factuals::Factuals;
    use crate::game::good::{Good, TIME};
    use crate::game::job::JobLine;
    use crate::game::market::{Market, MarketGood};
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput, ProcessTag};
    use crate::game::scalingfactor::ScalingFactor;

    fn near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{actual} != {expected}"
        );
    }

    fn good(id: usize, decay: f64) -> Good {
        Good {
            id,
            name: format!("g{id}"),
            class: None,
            decay_rate: decay,
            decay_result: HashMap::new(),
            mass: 1.0,
            volume: 0.0,
            tags: Default::default(),
            categories: vec![],
        }
    }

    fn world(goods: &[(usize, f64)]) -> Factuals {
        let mut factuals = Factuals::new();
        factuals.config.labor.time_per_labor = 10.0;
        for &(id, decay) in goods {
            factuals.goods.insert(id, good(id, decay));
        }
        factuals
    }

    fn worker() -> Pop {
        let mut pop = Pop::new(1);
        let home = &mut pop.demographics.household;
        home.count = 1.0;
        home.adult = 1.0;
        home.elder = 0.0;
        home.child = 0.0;
        home.adult_labor = 1.0;
        pop
    }

    fn desire(amount: f64, good_id: usize) -> Desire {
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 0,
            target: vec![DesireTarget::new(good_id, DesireTargetType::Consume, 1.0)],
            amount,
            satisfaction: 0.0,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        }
    }

    fn input(good_id: usize, amount: f64) -> ProcessInput {
        ProcessInput::new(good_id, amount, false, InputType::Destroyed, false)
    }

    fn process(id: usize, inputs: Vec<ProcessInput>, output: usize, amount: f64) -> Process {
        let mut process = Process::new(id, format!("p{id}"), 0);
        for row in inputs {
            process = process.with_input(row);
        }
        process.with_output(ProcessOutput::new(output, amount, false))
    }

    #[test]
    fn idle_time_goes_to_subsistence() {
        let mut pop = worker();
        pop.planning(&world(&[]), None);
        // Each hold rises to the cushion. The other 9 of the grant is subsistence.
        near(pop.held_desire_time, 0.5);
        near(pop.held_transport_time, 0.5);
        near(pop.subsistence_rate, 0.9);
        assert_eq!(pop.job.lines[0].target, Some(0.0));
        assert_eq!(pop.job.lines[1].target, Some(0.0));
    }

    #[test]
    fn consumed_time_is_held_after_record_keeping() {
        let mut empty = worker();
        empty.record_keeping();
        assert_eq!(empty.time_consumed, 0.0);

        let mut pop = worker();
        pop.property.insert(TIME, PopPRow::new(10.0).with_consumed(4.0));
        pop.record_keeping();
        assert_eq!(pop.time_consumed, 4.0);
        assert_eq!(pop.property[&TIME].consumed, 0.0);
        pop.planning(&world(&[]), None);
        near(pop.held_desire_time, 4.5);
        near(pop.held_transport_time, 0.5);
        near(pop.subsistence_rate, 0.5);
    }

    #[test]
    fn haul_steps_down_when_less_time_was_spent() {
        let mut pop = worker();
        pop.transport_spent = 3.5;
        pop.planning(&world(&[]), None);
        near(pop.held_transport_time, 4.0);
        near(pop.held_desire_time, 0.5);
        near(pop.subsistence_rate, 0.55);

        pop.transport_spent = 0.0;
        pop.planning(&world(&[]), None);
        near(pop.held_transport_time, 3.5);
        near(pop.subsistence_rate, 0.6);
    }

    #[test]
    fn purchased_transport_replaces_time_spent() {
        let mut pop = worker();
        pop.transport_spent = 2.0;
        pop.transport_purchased = 2.0;
        pop.planning(&world(&[]), None);
        near(pop.held_transport_time, 0.5);
        near(pop.subsistence_rate, 0.9);
    }

    #[test]
    fn a_held_line_stays_on_the_step() {
        let mut pop = worker();
        pop.desires[0].push(desire(6.0, 1));
        // 0.52 sits between no time and one step. A hold returns it to the step.
        pop.job.lines[0] = JobLine::new(1, Some(0.52), vec![]);
        let mut factuals = world(&[(TIME, 0.0), (1, 0.0)]);
        factuals.processes.insert(1, process(1, vec![input(TIME, 1.0)], 1, 4.0));
        pop.planning(&factuals, None);
        near(pop.job.lines[0].target.unwrap(), 0.5);
        near(pop.subsistence_rate, 0.85);
    }

    #[test]
    fn a_profitable_craft_holds_on_one_vote() {
        let mut pop = worker();
        pop.desires[0].push(desire(6.0, 1));
        pop.job.lines[0] = JobLine::new(1, None, vec![9]);
        let mut factuals = world(&[(TIME, 0.0), (1, 0.0)]);
        factuals.processes.insert(1, process(1, vec![input(TIME, 1.0)], 1, 4.0));
        pop.planning(&factuals, None);
        assert_eq!(pop.job.lines[0].target, Some(0.0));
        assert_eq!(pop.job.lines[0].inputs, vec![9]);
    }

    #[test]
    fn living_and_profit_step_a_craft_up() {
        let mut pop = worker();
        pop.desires[0].push(desire(6.0, 1));
        pop.job.lines[0] = JobLine::new(1, Some(0.0), vec![]);
        pop.sol_samples.push_back(0.0);
        pop.sol_samples.push_back(1.0);
        let mut factuals = world(&[(TIME, 0.0), (1, 0.0)]);
        factuals.processes.insert(1, process(1, vec![input(TIME, 1.0)], 1, 4.0));
        pop.planning(&factuals, None);
        near(pop.job.lines[0].target.unwrap(), 0.5);
    }

    #[test]
    fn a_negative_direction_steps_down() {
        let mut pop = worker();
        pop.desires[0].push(desire(6.0, 1));
        pop.job.lines[0] = JobLine::new(1, Some(4.0), vec![]);
        pop.job.lines[0].completed = 1.0;
        pop.sol_samples.push_back(2.0);
        pop.sol_samples.push_back(1.0);
        let mut factuals = world(&[(TIME, 0.0), (1, 0.0), (2, 0.0)]);
        factuals.processes.insert(
            1,
            process(1, vec![input(TIME, 1.0), input(2, 1.0)], 1, 4.0),
        );
        let mut market = Market::new(1);
        market.goods.insert(1, MarketGood::new().with_salability(0.2));
        pop.planning(&factuals, Some(&market));
        near(pop.job.lines[0].target.unwrap(), 3.5);
    }

    #[test]
    fn a_craft_under_the_subsistence_bar_stays_off() {
        let mut pop = worker();
        pop.desires[0].push(desire(6.0, 1));
        pop.job.lines[0] = JobLine::new(1, None, vec![]);
        let mut factuals = world(&[(TIME, 0.0), (1, 0.0)]);
        factuals.processes.insert(1, process(1, vec![input(TIME, 1.0)], 1, 0.5));
        pop.planning(&factuals, None);
        assert_eq!(pop.job.lines[0].target, Some(0.0));
    }

    #[test]
    fn an_even_tally_holds_both_lines() {
        let mut pop = worker();
        pop.job.lines[0] = JobLine::new(1, Some(4.0), vec![]);
        pop.job.lines[1] = JobLine::new(2, Some(4.0), vec![]);
        let mut factuals = world(&[(TIME, 0.0), (1, 0.0), (5, 0.0)]);
        factuals.processes.insert(1, process(1, vec![input(TIME, 1.0)], 1, 4.0));
        let mut labor = process(2, vec![input(TIME, 1.0)], 5, 1.0);
        labor.process_tags.push(ProcessTag::Labor);
        factuals.processes.insert(2, labor);
        pop.planning(&factuals, None);
        near(pop.job.lines[0].target.unwrap(), 4.0);
        near(pop.job.lines[1].target.unwrap(), 4.0);
        near(pop.subsistence_rate, 0.1);
    }

    #[test]
    fn no_morning_time_clears_the_plan() {
        let mut pop = worker();
        pop.demographics.household.adult_labor = 0.0;
        pop.held_desire_time = 2.0;
        pop.held_transport_time = 2.0;
        pop.job.lines[0] = JobLine::new(1, Some(4.0), vec![]);
        let mut factuals = world(&[(TIME, 0.0), (1, 0.0)]);
        factuals.processes.insert(1, process(1, vec![input(TIME, 1.0)], 1, 4.0));
        pop.planning(&factuals, None);
        near(pop.subsistence_rate, 0.0);
        assert_eq!(pop.job.lines[0].target, Some(0.0));
        assert_eq!(pop.job.lines[1].target, Some(0.0));
        assert_eq!(pop.held_desire_time, 0.0);
        assert_eq!(pop.held_transport_time, 0.0);
    }
}

#[cfg(test)]
mod buy_should {
    use std::collections::HashSet;

    use super::{buy_from_offers, Pop, PopPRow, SellOffer};
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{Desire, DesireTarget, DesireTargetType};
    use crate::game::factuals::Factuals;
    use crate::game::good::{Good, GoodTag};
    use crate::game::market::Market;
    use crate::game::scalingfactor::ScalingFactor;

    const GRAIN: usize = 1;
    const BREAD: usize = 3;
    const BEER: usize = 25;
    const CABIN: usize = 8;

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }

    fn plain(id: usize, untradeable: bool) -> Good {
        let mut tags = HashSet::new();
        if untradeable {
            tags.insert(GoodTag::Untradeable);
        }
        Good {
            id,
            name: format!("g{id}"),
            class: None,
            decay_rate: 0.0,
            decay_result: Default::default(),
            mass: 1.0,
            volume: 0.0,
            tags,
            categories: vec![],
        }
    }

    fn world(untradeable: &[usize]) -> Factuals {
        let mut factuals = Factuals::new();
        for id in [GRAIN, BREAD, BEER, CABIN, 7] {
            factuals.goods.insert(id, plain(id, untradeable.contains(&id)));
        }
        factuals
    }

    fn market() -> Market {
        let mut market = Market::new(1);
        market.currencies.push(GRAIN);
        market
    }

    fn desire(amount: f64, targets: Vec<(usize, f64)>) -> Desire {
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 0,
            target: targets
                .into_iter()
                .map(|(good, efficiency)| DesireTarget::new(good, DesireTargetType::Consume, efficiency))
                .collect(),
            amount,
            satisfaction: 0.0,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 1.0,
        }
    }

    fn with_grain(id: usize, loose: f64) -> Pop {
        let mut pop = Pop::new(id);
        if loose > 0.0 {
            pop.property.insert(GRAIN, PopPRow::new(loose));
        }
        pop
    }

    fn offer(pop: usize, good: usize, amount: f64) -> SellOffer {
        SellOffer { pop, good, amount }
    }

    fn qty(pop: &Pop, good: usize) -> f64 {
        pop.property.get(&good).map(|row| row.quantity).unwrap_or(0.0)
    }

    #[test]
    fn buys_the_offered_good_and_pays_the_seller() {
        // Four satisfaction at efficiency 1 is four loaves. The seller keeps the rest.
        let factuals = world(&[]);
        let market = market();
        let mut buyer = with_grain(1, 10.0);
        buyer.desires[0].push(desire(4.0, vec![(BREAD, 1.0)]));
        let mut seller = Pop::new(2);
        seller.property.insert(BREAD, PopPRow::new(6.0));
        let mut pops = [buyer, seller];
        let mut offers = vec![offer(2, BREAD, 6.0), offer(1, GRAIN, 10.0)];
        buy_from_offers(&mut pops, &market, &factuals, &mut offers);
        near(qty(&pops[0], BREAD), 4.0);
        near(qty(&pops[0], GRAIN), 6.0);
        near(qty(&pops[1], BREAD), 2.0);
        near(qty(&pops[1], GRAIN), 4.0);
        near(offers[0].amount, 2.0);
        near(offers[1].amount, 6.0);
    }

    #[test]
    fn does_not_buy_its_own_offer() {
        // Four loaves on hand cover half the desire. The rest of the offer is this pop's own.
        let factuals = world(&[]);
        let market = market();
        let mut pop = with_grain(1, 10.0);
        pop.desires[0].push(desire(8.0, vec![(BREAD, 1.0)]));
        pop.property.insert(BREAD, PopPRow::new(4.0));
        let mut pops = [pop];
        let mut offers = vec![offer(1, BREAD, 4.0), offer(1, GRAIN, 10.0)];
        buy_from_offers(&mut pops, &market, &factuals, &mut offers);
        near(qty(&pops[0], BREAD), 4.0);
        near(qty(&pops[0], GRAIN), 10.0);
        near(offers[0].amount, 4.0);
    }

    #[test]
    fn holdings_cover_the_desire() {
        // The loaves already in the house cover the desire, so the offer stays up.
        let factuals = world(&[]);
        let market = market();
        let mut buyer = with_grain(1, 10.0);
        buyer.desires[0].push(desire(4.0, vec![(BREAD, 1.0)]));
        buyer.property.insert(BREAD, PopPRow::new(4.0));
        let mut seller = Pop::new(2);
        seller.property.insert(BREAD, PopPRow::new(4.0));
        let mut pops = [buyer, seller];
        let mut offers = vec![offer(2, BREAD, 4.0)];
        buy_from_offers(&mut pops, &market, &factuals, &mut offers);
        near(qty(&pops[0], BREAD), 4.0);
        near(qty(&pops[1], BREAD), 4.0);
        near(offers[0].amount, 4.0);
    }

    #[test]
    fn cheapest_satisfaction_is_bought() {
        // Beer satisfies twice what bread does, at the same price, so the pop buys beer.
        let factuals = world(&[]);
        let market = market();
        let mut buyer = with_grain(1, 10.0);
        buyer.desires[0].push(desire(4.0, vec![(BREAD, 1.0), (BEER, 2.0)]));
        let mut seller = Pop::new(2);
        seller.property.insert(BREAD, PopPRow::new(8.0));
        seller.property.insert(BEER, PopPRow::new(8.0));
        let mut pops = [buyer, seller];
        let mut offers = vec![offer(2, BREAD, 8.0), offer(2, BEER, 8.0)];
        buy_from_offers(&mut pops, &market, &factuals, &mut offers);
        near(qty(&pops[0], BEER), 2.0);
        near(qty(&pops[0], BREAD), 0.0);
        near(qty(&pops[1], GRAIN), 2.0);
    }

    #[test]
    fn stops_at_money_and_whole_units() {
        // 2.4 grain buys two whole loaves. An offer of 1.9 sells one.
        let factuals = world(&[]);
        let market = market();
        let mut short = with_grain(1, 2.4);
        short.desires[0].push(desire(5.0, vec![(BREAD, 1.0)]));
        let mut seller = Pop::new(2);
        seller.property.insert(BREAD, PopPRow::new(10.0));
        let mut pops = [short, seller];
        let mut offers = vec![offer(2, BREAD, 10.0)];
        buy_from_offers(&mut pops, &market, &factuals, &mut offers);
        near(qty(&pops[0], BREAD), 2.0);
        near(qty(&pops[0], GRAIN), 0.4);

        let mut buyer = with_grain(1, 10.0);
        buyer.desires[0].push(desire(5.0, vec![(BREAD, 1.0)]));
        let mut seller = Pop::new(2);
        seller.property.insert(BREAD, PopPRow::new(1.9));
        let mut pops = [buyer, seller];
        let mut offers = vec![offer(2, BREAD, 1.9)];
        buy_from_offers(&mut pops, &market, &factuals, &mut offers);
        near(qty(&pops[0], BREAD), 1.0);
        near(offers[0].amount, 0.9);
    }

    #[test]
    fn pays_savings_after_loose_tender() {
        // One loose coin and three saved. Three loaves take the loose coin and two saved.
        let factuals = world(&[]);
        let market = market();
        let mut buyer = with_grain(1, 1.0);
        buyer.savings.insert(GRAIN, 3.0);
        buyer.desires[0].push(desire(3.0, vec![(BREAD, 1.0)]));
        let mut seller = Pop::new(2);
        seller.property.insert(BREAD, PopPRow::new(3.0));
        let mut pops = [buyer, seller];
        let mut offers = vec![offer(2, BREAD, 3.0), offer(1, GRAIN, 1.0)];
        buy_from_offers(&mut pops, &market, &factuals, &mut offers);
        near(qty(&pops[0], BREAD), 3.0);
        near(qty(&pops[0], GRAIN), 0.0);
        near(pops[0].savings()[&GRAIN], 1.0);
        near(qty(&pops[1], GRAIN), 3.0);
        near(offers[1].amount, 0.0);
    }

    #[test]
    fn reserved_tender_stays_put() {
        // Four of the five coins are earmarked. Only the loose coin can pay.
        let factuals = world(&[]);
        let market = market();
        let mut buyer = Pop::new(1);
        buyer.property.insert(GRAIN, PopPRow::new(5.0).with_reserve(4.0));
        buyer.desires[0].push(desire(3.0, vec![(BREAD, 1.0)]));
        let mut seller = Pop::new(2);
        seller.property.insert(BREAD, PopPRow::new(3.0));
        let mut pops = [buyer, seller];
        let mut offers = vec![offer(2, BREAD, 3.0), offer(1, GRAIN, 1.0)];
        buy_from_offers(&mut pops, &market, &factuals, &mut offers);
        near(qty(&pops[0], BREAD), 1.0);
        near(qty(&pops[0], GRAIN), 4.0);
        near(pops[0].property[&GRAIN].reserved, 4.0);
    }

    #[test]
    fn common_waits_until_basic_is_covered() {
        let factuals = world(&[]);
        let market = market();
        let mut buyer = with_grain(1, 10.0);
        buyer.desires[0].push(desire(5.0, vec![(BREAD, 1.0)]));
        buyer.desires[1].push(desire(4.0, vec![(CABIN, 2.0)]));
        let mut seller = Pop::new(2);
        seller.property.insert(CABIN, PopPRow::new(4.0));
        let mut pops = [buyer, seller];
        let mut offers = vec![offer(2, CABIN, 4.0)];
        buy_from_offers(&mut pops, &market, &factuals, &mut offers);
        // Basic bread is not on offer and not in hand, so the cabin stays unsold.
        near(qty(&pops[0], CABIN), 0.0);
        near(offers[0].amount, 4.0);

        buyer = with_grain(1, 10.0);
        buyer.desires[0].push(desire(5.0, vec![(BREAD, 1.0)]));
        buyer.property.insert(BREAD, PopPRow::new(5.0));
        buyer.desires[1].push(desire(4.0, vec![(CABIN, 2.0)]));
        seller = Pop::new(2);
        seller.property.insert(CABIN, PopPRow::new(4.0));
        pops = [buyer, seller];
        offers = vec![offer(2, CABIN, 4.0)];
        buy_from_offers(&mut pops, &market, &factuals, &mut offers);
        // Basic is covered. Two cabins cover four satisfaction.
        near(qty(&pops[0], CABIN), 2.0);
        near(qty(&pops[1], GRAIN), 2.0);
    }

    #[test]
    fn an_untradeable_good_is_skipped() {
        // Wood would be the cheaper satisfaction, and it cannot be bought. Bread can.
        let factuals = world(&[7]);
        let market = market();
        let mut buyer = with_grain(1, 10.0);
        buyer.desires[0].push(desire(4.0, vec![(7, 10.0), (BREAD, 1.0)]));
        let mut seller = Pop::new(2);
        seller.property.insert(7, PopPRow::new(4.0));
        seller.property.insert(BREAD, PopPRow::new(4.0));
        let mut pops = [buyer, seller];
        let mut offers = vec![offer(2, 7, 4.0), offer(2, BREAD, 4.0)];
        buy_from_offers(&mut pops, &market, &factuals, &mut offers);
        near(qty(&pops[0], BREAD), 4.0);
        near(qty(&pops[0], 7), 0.0);
        near(offers[0].amount, 4.0);
    }

    #[test]
    fn the_wealthier_pop_buys_first() {
        // One loaf is offered. The pop holding jewelry is wealthier and takes it.
        let factuals = world(&[]);
        let market = market();
        let mut poor = with_grain(1, 5.0);
        poor.desires[0].push(desire(1.0, vec![(BREAD, 1.0)]));
        let mut rich = with_grain(2, 1.0);
        rich.property.insert(6, PopPRow::new(10.0));
        rich.desires[0].push(desire(1.0, vec![(BREAD, 1.0)]));
        let mut seller = Pop::new(3);
        seller.property.insert(BREAD, PopPRow::new(1.0));
        let mut pops = [poor, rich, seller];
        let mut offers = vec![offer(3, BREAD, 1.0)];
        buy_from_offers(&mut pops, &market, &factuals, &mut offers);
        near(qty(&pops[1], BREAD), 1.0);
        near(qty(&pops[0], BREAD), 0.0);
        near(offers[0].amount, 0.0);
    }

    #[test]
    fn a_sale_keeps_fresh_inside_quantity() {
        // The loaf pile has two aged units. Selling three takes one fresh unit with them.
        // The coins that pay are mostly fresh, and the same rule applies. Receipts stay unfresh.
        let factuals = world(&[]);
        let market = market();
        let mut buyer = Pop::new(1);
        buyer.property.insert(GRAIN, PopPRow::new(5.0).with_fresh(4.0));
        buyer.desires[0].push(desire(3.0, vec![(BREAD, 1.0)]));
        let mut seller = Pop::new(2);
        seller.property.insert(BREAD, PopPRow::new(6.0).with_fresh(4.0));
        let mut pops = [buyer, seller];
        let mut offers = vec![offer(2, BREAD, 6.0)];
        buy_from_offers(&mut pops, &market, &factuals, &mut offers);
        near(qty(&pops[1], BREAD), 3.0);
        near(pops[1].property[&BREAD].fresh, 3.0);
        near(qty(&pops[0], GRAIN), 2.0);
        near(pops[0].property[&GRAIN].fresh, 2.0);
        near(pops[0].property[&BREAD].fresh, 0.0);
        near(pops[1].property[&GRAIN].fresh, 0.0);
    }
}

#[cfg(test)]
mod sentiments_should {
    use std::collections::VecDeque;

    use super::{Pop, PopEffect, PopPRow};
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{Desire, DesireEffect, DesireTarget, DesireTargetType};
    use crate::game::good::TIME;
    use crate::game::market::Market;
    use crate::game::scalingfactor::ScalingFactor;
    use crate::game::sentiment::{Sentiment, SentimentKind};

    fn near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{actual} != {expected}"
        );
    }

    fn make_pop() -> Pop {
        Pop::new(1)
    }

    fn desire(id: usize, targets: Vec<DesireTarget>, amount: f64) -> Desire {
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: id,
            priority: 0,
            target: targets,
            amount,
            satisfaction: 0.0,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        }
    }

    #[test]
    fn standard_of_living_sums_tiers_and_desire_satisfaction_effects() {
        let mut pop = make_pop();
        let mut basic = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        basic.satisfaction = 2.0;
        basic.effect.push(DesireEffect::Satisfaction(6.0, true));
        let mut common = desire(2, vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)], 4.0);
        common.satisfaction = 4.0;
        common.effect.push(DesireEffect::Satisfaction(2.0, false));
        pop.desires[0].push(basic);
        pop.desires[1].push(common);

        let sol = pop.calculate_sol();

        near(sol, 4.5);
        near(pop.records.satisfaction, 4.5);
    }

    #[test]
    fn standard_of_living_adds_pop_satisfaction_and_drops_that_effect() {
        let mut pop = make_pop();
        let mut basic = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        basic.satisfaction = 4.0;
        pop.desires[0].push(basic);
        pop.stored_effects.push(PopEffect::Satisfaction { tier: 1, amount: 1.5 });
        pop.stored_effects.push(PopEffect::Birthrate(0.01));

        let sol = pop.calculate_sol();

        near(sol, 2.5);
        assert_eq!(pop.stored_effects, vec![PopEffect::Birthrate(0.01)]);
    }

    #[test]
    fn calculate_sol_records_infinity_when_there_are_no_desires() {
        let mut pop = make_pop();
        let sol = pop.calculate_sol();
        assert!(sol.is_infinite());
        assert!(pop.records.satisfaction.is_infinite());
    }

    #[test]
    fn update_sentiments_shifts_happiness_from_a_met_desire() {
        let mut pop = make_pop();
        let mut basic = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        basic.satisfaction = 4.0;
        basic.effect.push(DesireEffect::SentimentFlat(SentimentKind::Happiness, 0.2, true));
        pop.desires[0].push(basic);

        pop.update_sentiments(None);

        near(pop.sentiment.happiness(), 0.2 / 1.2);
        near(pop.sentiment.contentment(), 1.0 / 1.2);
        assert_eq!(pop.sentiment.anger(), 0.0);
        assert_eq!(pop.sentiment.fear(), 0.0);
        assert_eq!(pop.sentiment.hope(), 0.0);
    }

    #[test]
    fn update_sentiments_applies_stored_hope_and_drops_it() {
        let mut pop = make_pop();
        let mut basic = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        basic.effect.push(DesireEffect::SentimentFlat(SentimentKind::Anger, -0.2, false));
        pop.desires[0].push(basic);
        pop.stored_effects.push(PopEffect::SentimentFlat {
            kind: SentimentKind::Hope,
            delta: 0.2,
        });
        pop.stored_effects.push(PopEffect::Birthrate(0.01));

        pop.update_sentiments(None);

        near(pop.sentiment.anger(), 0.24 / 1.48);
        near(pop.sentiment.fear(), 0.04 / 1.48);
        near(pop.sentiment.hope(), 0.2 / 1.48);
        near(pop.sentiment.contentment(), 1.0 / 1.48);
        assert_eq!(pop.stored_effects, vec![PopEffect::Birthrate(0.01)]);
    }

    #[test]
    fn update_sentiments_scales_one_emotion_from_its_current_share() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::even();
        let mut basic = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        basic.satisfaction = 4.0;
        basic.effect.push(DesireEffect::SentimentRelative(SentimentKind::Hope, 0.5, true));
        pop.desires[0].push(basic);

        pop.update_sentiments(None);

        near(pop.sentiment.hope(), 0.27 / 1.1);
        near(pop.sentiment.happiness(), 0.17 / 1.1);
        near(pop.sentiment.contentment(), 0.32 / 1.1);
    }

    #[test]
    fn update_sentiments_moves_hope_from_wealth_per_household() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(10.0));

        pop.update_sentiments(None);

        near(pop.sentiment.hope(), 0.10 / 1.10);
        near(pop.sentiment.contentment(), 1.0 / 1.10);
        assert_eq!(pop.sentiment.happiness(), 0.0);
        assert_eq!(pop.sentiment.fear(), 0.0);
    }

    #[test]
    fn update_sentiments_prices_wealth_from_the_market() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(10.0));
        let mut market = Market::new(1);
        market.currencies = vec![9];
        market.exchange_rates.entry(9).or_default().insert(1, 2.0);

        pop.update_sentiments(Some(&market));

        near(pop.sentiment.hope(), 0.05 / 1.05);
        near(pop.sentiment.contentment(), 1.0 / 1.05);
    }

    #[test]
    fn update_sentiments_splits_basic_shortfalls_across_desires() {
        let mut pop = make_pop();
        let mut met = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        met.satisfaction = 8.0;
        let unmet = desire(2, vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)], 4.0);
        pop.desires[0].push(met);
        pop.desires[0].push(unmet);

        pop.update_sentiments(None);

        near(pop.sentiment.anger(), 0.02 / 1.04);
        near(pop.sentiment.fear(), 0.02 / 1.04);
        near(pop.sentiment.contentment(), 1.0 / 1.04);
    }

    #[test]
    fn update_sentiments_lets_common_satisfaction_pass_one_level() {
        let mut pop = make_pop();
        let mut common = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        common.satisfaction = 4.0;
        common.effect.push(DesireEffect::Satisfaction(0.5, true));
        pop.desires[1].push(common);

        pop.update_sentiments(None);

        near(pop.sentiment.contentment(), 1.03 / 1.03);
        assert_eq!(pop.sentiment.anger(), 0.0);
        assert_eq!(pop.sentiment.hope(), 0.0);
    }

    #[test]
    fn update_sentiments_counts_each_luxury_level() {
        let mut pop = make_pop();
        for id in [1, 2] {
            let mut luxury = desire(id, vec![DesireTarget::new(id, DesireTargetType::Consume, 1.0)], 4.0);
            luxury.satisfaction = 8.0;
            pop.desires[2].push(luxury);
        }

        pop.update_sentiments(None);

        near(pop.sentiment.hope(), 0.02 / 1.04);
        near(pop.sentiment.happiness(), 0.02 / 1.04);
        near(pop.sentiment.contentment(), 1.0 / 1.04);
    }

    #[test]
    fn update_sentiments_adds_stored_satisfaction_on_its_tier() {
        let mut pop = make_pop();
        pop.desires[1].push(desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0));
        pop.stored_effects.push(PopEffect::Satisfaction { tier: 1, amount: 1.5 });
        pop.stored_effects.push(PopEffect::Birthrate(0.01));

        pop.update_sentiments(None);

        near(pop.sentiment.contentment(), 1.03 / 1.03);
        assert_eq!(
            pop.stored_effects,
            vec![
                PopEffect::Satisfaction { tier: 1, amount: 1.5 },
                PopEffect::Birthrate(0.01),
            ]
        );
    }

    #[test]
    fn update_sentiments_records_living_before_it_can_shift() {
        let mut pop = make_pop();
        let mut basic = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        basic.satisfaction = 4.0;
        pop.desires[0].push(basic);

        pop.update_sentiments(None);

        assert_eq!(pop.sentiment.contentment(), 1.0);
        assert_eq!(pop.sol_samples.len(), 1);
        near(pop.sol_samples[0], 1.0);
        assert_eq!(pop.recorded_living(), Some(1.0));
    }

    #[test]
    fn update_sentiments_moves_hope_and_happiness_from_a_rising_living() {
        let mut pop = make_pop();
        pop.sol_samples.push_back(0.0);
        let mut basic = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        basic.satisfaction = 4.0;
        pop.desires[0].push(basic);

        pop.update_sentiments(None);

        near(pop.sentiment.hope(), 0.25 / 1.50);
        near(pop.sentiment.happiness(), 0.25 / 1.50);
        near(pop.sentiment.contentment(), 1.0 / 1.50);
        assert_eq!(pop.sentiment.anger(), 0.0);
        assert_eq!(pop.sentiment.fear(), 0.0);
    }

    #[test]
    fn update_sentiments_moves_anger_and_fear_from_a_falling_living() {
        let mut pop = make_pop();
        pop.sol_samples.push_back(2.0);
        let mut basic = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        basic.satisfaction = 4.0;
        pop.desires[0].push(basic);

        pop.update_sentiments(None);

        near(pop.sentiment.anger(), 0.25 / 1.50);
        near(pop.sentiment.fear(), 0.25 / 1.50);
        near(pop.sentiment.contentment(), 1.0 / 1.50);
        assert_eq!(pop.sentiment.hope(), 0.0);
        assert_eq!(pop.sentiment.happiness(), 0.0);
    }

    #[test]
    fn update_sentiments_lets_a_rising_living_outweigh_wealth() {
        let mut pop = make_pop();
        pop.sol_samples.push_back(0.0);
        pop.property.insert(1, PopPRow::new(10.0));
        let mut basic = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        basic.satisfaction = 4.0;
        pop.desires[0].push(basic);

        pop.update_sentiments(None);

        near(pop.sentiment.hope(), 0.35 / 1.60);
        near(pop.sentiment.happiness(), 0.25 / 1.60);
        near(pop.sentiment.contentment(), 1.0 / 1.60);
    }

    #[test]
    fn update_sentiments_drops_the_oldest_living_sample() {
        let mut pop = make_pop();
        pop.sol_samples = VecDeque::from([0.0, 1.0, 1.0, 1.0, 1.0]);
        let mut basic = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        basic.satisfaction = 4.0;
        pop.desires[0].push(basic);

        pop.update_sentiments(None);

        near(pop.sentiment.hope(), 0.05 / 1.10);
        near(pop.sentiment.happiness(), 0.05 / 1.10);
        near(pop.sentiment.contentment(), 1.0 / 1.10);
        assert_eq!(pop.sol_samples.len(), 5);
        assert!(pop.sol_samples.iter().all(|sample| (sample - 1.0).abs() < 1e-9));
    }

    #[test]
    fn update_sentiments_skips_an_infinite_living() {
        let mut pop = make_pop();
        pop.sol_samples.push_back(1.0);

        pop.update_sentiments(None);

        assert_eq!(pop.sentiment.contentment(), 1.0);
        assert_eq!(pop.sol_samples, VecDeque::from([1.0]));
    }

    #[test]
    fn update_sentiments_divides_wealth_by_households_and_skips_time() {
        let mut pop = make_pop();
        pop.demographics.household.count = 5.0;
        pop.property.insert(TIME, PopPRow::new(100.0));
        pop.property.insert(1, PopPRow::new(10.0));

        pop.update_sentiments(None);

        near(pop.sentiment.hope(), 0.02 / 1.02);
        near(pop.sentiment.contentment(), 1.0 / 1.02);
        assert_eq!(pop.sentiment.fear(), 0.0);
    }

    #[test]
    fn update_sentiments_moves_fear_from_negative_wealth() {
        let mut pop = make_pop();
        pop.savings.insert(1, -10.0);

        pop.update_sentiments(None);

        near(pop.sentiment.fear(), 0.10 / 1.10);
        near(pop.sentiment.contentment(), 1.0 / 1.10);
        assert_eq!(pop.sentiment.hope(), 0.0);
        assert_eq!(pop.sentiment.anger(), 0.0);
    }

    #[test]
    fn update_sentiments_skips_wealth_when_there_are_no_households() {
        let mut pop = make_pop();
        pop.demographics.household.count = 0.0;
        pop.property.insert(1, PopPRow::new(10.0));

        pop.update_sentiments(None);

        assert_eq!(pop.sentiment.contentment(), 1.0);
        assert_eq!(pop.sentiment.hope(), 0.0);
    }

    #[test]
    fn update_sentiments_reduces_basic_unmet_by_stored_satisfaction() {
        let mut pop = make_pop();
        pop.desires[0].push(desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0));
        pop.stored_effects.push(PopEffect::Satisfaction { tier: 0, amount: 0.5 });

        pop.update_sentiments(None);

        near(pop.sentiment.anger(), 0.02 / 1.04);
        near(pop.sentiment.fear(), 0.02 / 1.04);
        near(pop.sentiment.contentment(), 1.0 / 1.04);
        assert_eq!(
            pop.stored_effects,
            vec![PopEffect::Satisfaction { tier: 0, amount: 0.5 }]
        );
    }

    #[test]
    fn update_sentiments_lets_stored_basic_satisfaction_clear_the_gap() {
        let mut pop = make_pop();
        pop.desires[0].push(desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0));
        pop.stored_effects.push(PopEffect::Satisfaction { tier: 0, amount: 1.5 });

        pop.update_sentiments(None);

        assert_eq!(pop.sentiment.contentment(), 1.0);
        assert_eq!(pop.sentiment.anger(), 0.0);
        assert_eq!(pop.sentiment.fear(), 0.0);
        assert_eq!(
            pop.stored_effects,
            vec![PopEffect::Satisfaction { tier: 0, amount: 1.5 }]
        );
    }

    #[test]
    fn update_sentiments_deepens_basic_unmet_with_a_satisfaction_malus() {
        let mut pop = make_pop();
        let mut basic = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        basic.effect.push(DesireEffect::Satisfaction(0.5, false));
        pop.desires[0].push(basic);

        pop.update_sentiments(None);

        near(pop.sentiment.anger(), 0.06 / 1.12);
        near(pop.sentiment.fear(), 0.06 / 1.12);
        near(pop.sentiment.contentment(), 1.0 / 1.12);
    }

    #[test]
    fn update_sentiments_leaves_a_met_basic_bonus_alone() {
        let mut pop = make_pop();
        let mut basic = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        basic.satisfaction = 4.0;
        basic.effect.push(DesireEffect::Satisfaction(0.5, true));
        pop.desires[0].push(basic);

        pop.update_sentiments(None);

        assert_eq!(pop.sentiment.contentment(), 1.0);
        assert_eq!(pop.sentiment.hope(), 0.0);
        assert_eq!(pop.sentiment.happiness(), 0.0);
        assert_eq!(pop.sentiment.anger(), 0.0);
    }

    #[test]
    fn update_sentiments_shows_common_satisfaction_above_one_level() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::even();
        let mut common = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        common.satisfaction = 4.0;
        common.effect.push(DesireEffect::Satisfaction(0.5, true));
        pop.desires[1].push(common);

        pop.update_sentiments(None);

        near(pop.sentiment.contentment(), 0.35 / 1.03);
        near(pop.sentiment.happiness(), 0.17 / 1.03);
        near(pop.sentiment.hope(), 0.17 / 1.03);
        near(pop.sentiment.anger(), 0.17 / 1.03);
        near(pop.sentiment.fear(), 0.17 / 1.03);
    }

    #[test]
    fn update_sentiments_adds_luxury_bonus_levels() {
        let mut pop = make_pop();
        let mut luxury = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        luxury.satisfaction = 8.0;
        luxury.effect.push(DesireEffect::Satisfaction(0.5, true));
        pop.desires[2].push(luxury);

        pop.update_sentiments(None);

        near(pop.sentiment.hope(), 0.03 / 1.06);
        near(pop.sentiment.happiness(), 0.03 / 1.06);
        near(pop.sentiment.contentment(), 1.0 / 1.06);
    }

    #[test]
    fn update_sentiments_ignores_a_negative_common_level() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::even();
        let mut common = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        common.effect.push(DesireEffect::Satisfaction(0.5, false));
        pop.desires[1].push(common);

        pop.update_sentiments(None);

        near(pop.sentiment.contentment(), 0.32);
        near(pop.sentiment.happiness(), 0.17);
        near(pop.sentiment.anger(), 0.17);
        near(pop.sentiment.fear(), 0.17);
        near(pop.sentiment.hope(), 0.17);
    }

    #[test]
    fn update_sentiments_counts_stored_satisfaction_on_an_empty_tier() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::even();
        pop.stored_effects.push(PopEffect::Satisfaction { tier: 1, amount: 1.5 });

        pop.update_sentiments(None);

        near(pop.sentiment.contentment(), 0.35 / 1.03);
        near(pop.sentiment.happiness(), 0.17 / 1.03);
        near(pop.sentiment.hope(), 0.17 / 1.03);
        assert!(pop.sol_samples.is_empty());
        assert_eq!(
            pop.stored_effects,
            vec![PopEffect::Satisfaction { tier: 1, amount: 1.5 }]
        );
    }

    #[test]
    fn update_sentiments_adds_nothing_when_living_matches_the_average() {
        let mut pop = make_pop();
        pop.sol_samples.push_back(1.0);
        let mut basic = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        basic.satisfaction = 4.0;
        pop.desires[0].push(basic);

        pop.update_sentiments(None);

        assert_eq!(pop.sentiment.contentment(), 1.0);
        assert_eq!(pop.sol_samples, VecDeque::from([1.0, 1.0]));
    }

    #[test]
    fn update_sentiments_adds_the_living_shift_again_on_the_next_day() {
        let mut pop = make_pop();
        pop.sol_samples.push_back(0.0);
        let mut basic = desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0);
        basic.satisfaction = 4.0;
        pop.desires[0].push(basic);

        pop.update_sentiments(None);
        pop.update_sentiments(None);

        let carried_hope = 0.25 / 1.50;
        let carried_contentment = 1.0 / 1.50;
        let hope = carried_hope * (1.0 - 0.15) + 0.125;
        let contentment = carried_contentment + 0.15 * carried_hope * 2.0;
        let mass = hope + hope + contentment;
        near(pop.sentiment.hope(), hope / mass);
        near(pop.sentiment.happiness(), hope / mass);
        near(pop.sentiment.contentment(), contentment / mass);
        assert_eq!(pop.sentiment.anger(), 0.0);
        assert_eq!(pop.sol_samples, VecDeque::from([0.0, 1.0, 1.0]));
    }

    #[test]
    fn update_sentiments_lets_a_falling_living_outweigh_unmet_basic() {
        let mut pop = make_pop();
        pop.sol_samples.push_back(1.0);
        pop.desires[0].push(desire(1, vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)], 4.0));

        pop.update_sentiments(None);

        near(pop.sentiment.anger(), 0.29 / 1.58);
        near(pop.sentiment.fear(), 0.29 / 1.58);
        near(pop.sentiment.contentment(), 1.0 / 1.58);
        assert_eq!(pop.sentiment.hope(), 0.0);
        assert_eq!(pop.sentiment.happiness(), 0.0);
    }

    #[test]
    fn update_sentiments_pulls_a_larger_share_harder_toward_contentment() {
        let mut far = make_pop();
        far.sentiment = Sentiment::from_parts(0.0, 0.2, 0.8, 0.0, 0.0);
        let mut near_pop = make_pop();
        near_pop.sentiment = Sentiment::from_parts(0.0, 0.8, 0.2, 0.0, 0.0);

        far.update_sentiments(None);
        near_pop.update_sentiments(None);

        near(far.sentiment.anger(), 0.68);
        near(far.sentiment.contentment(), 0.32);
        near(near_pop.sentiment.anger(), 0.17);
        near(near_pop.sentiment.contentment(), 0.83);
    }

    #[test]
    fn update_sentiments_pulls_again_when_the_cause_is_gone() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::from_parts(0.0, 0.0, 1.0, 0.0, 0.0);

        pop.update_sentiments(None);

        near(pop.sentiment.anger(), 0.85);
        near(pop.sentiment.contentment(), 0.15);
        assert_eq!(pop.sentiment.hope(), 0.0);
        assert_eq!(pop.sentiment.happiness(), 0.0);
        assert_eq!(pop.sentiment.fear(), 0.0);

        pop.update_sentiments(None);

        near(pop.sentiment.anger(), 0.7225);
        near(pop.sentiment.contentment(), 0.2775);
    }
}