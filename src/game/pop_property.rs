//! Property rows, demographics row, and day-end records for pops.

use bevy::utils::default;
use circular_buffer::CircularBuffer;

use crate::game::config::pop_constants;
use crate::game::household::Household;
use crate::game::util::lerp;

/// Demographic breakdown of a pop (one row for now).
#[derive(Debug, Clone, Copy)]
pub struct DemoRow {
    /// Living household block (count, average composition, sex, labor, partnership).
    pub household: Household,
    /// Species ID; currently should always be 0 (default human).
    pub species: usize,
    /// Culture ID; 0 means none.
    pub culture: usize,
    /// Class ID; 0 means none.
    pub class: usize,
    /// Religion ID; 0 means none.
    pub religion: usize,
}

impl DemoRow {
    /// Household size × count.
    pub fn total_population(&self) -> f64 {
        self.household.total_count()
    }

    pub fn adult_pop(&self) -> f64 {
        self.household.total_adults()
    }

    pub fn elder_pop(&self) -> f64 {
        self.household.total_elders()
    }

    pub fn children_pop(&self) -> f64 {
        self.household.total_children()
    }

    pub fn labor(&self) -> f64 {
        self.household.total_labor()
    }
    
    pub(crate) fn count(&self) -> f64 {
        self.household.count
    }
}

/// Per-good property ledger for a pop.
#[derive(Debug, Clone, Copy, Default)]
pub struct PopPRow {
    /// Total amount owned. Not necessarily available.
    pub quantity: f64,

    /// Touchstone for how much of this good needs target for desires.
    /// Updated with population changes; may be removed later.
    pub desire_needs: f64,

    /// Shopping target after shopping (bulk planning).
    /// Goods that cannot trade should stay 0.0.
    /// 
    /// Ideally should be equal to saved + reserved after shopping, but not a hard 
    /// requirement.
    pub shop_target: f64,

    /// Wish-to-preserve between days (hoarding target). Not a hard fence on consume.
    pub save_target: f64,

    /// Earmarked for today's use; does not remove from quantity. Reset day-start.
    pub reserved: f64,

    /// Consumed for desires today; full decay at day end.
    pub consumed: f64,

    /// Used (not destroyed) for use-desires; returned to quantity at day end after decay.
    pub used: f64,
}

impl PopPRow {
    pub fn new(quantity: f64) -> Self {
        Self {
            quantity,
            ..default()
        }
    }

    pub fn with_target(mut self, target: f64) -> Self {
        self.shop_target = target;
        self
    }

    pub fn with_reserve(mut self, reserve: f64) -> Self {
        self.reserved = reserve;
        self
    }

    /// Sets the between-days save target. Not on-hand savings.
    pub fn with_save_target(mut self, save_target: f64) -> Self {
        self.save_target = save_target;
        self
    }

    pub fn with_consumed(mut self, consumed: f64) -> Self {
        self.consumed = consumed;
        self
    }

    pub fn with_used(mut self, used: f64) -> Self {
        self.used = used;
        self
    }

    pub fn with_desire_need(mut self, desire_needs: f64) -> Self {
        self.desire_needs = desire_needs;
        self
    }

    /// `quantity - shop_target` (negative ⇒ want to buy).
    pub fn exchange(&self) -> f64 {
        self.quantity - self.shop_target
    }

    /// `quantity - reserved` (unclaimed stock).
    pub fn available(&self) -> f64 {
        self.quantity - self.reserved
    }

    /// # Saved
    ///
    /// Actual saved units: `quantity - reserved` (floored at 0).
    /// Consume draws quantity and reserved together, so this stays valid after consume.
    /// Does not use `save_target`.
    pub fn saved(&self) -> f64 {
        (self.quantity - self.reserved.max(0.0)).max(0.0)
    }
}

/// # Pop Record
/// 
/// Pop day-end / process-satisfaction records, including living-standard history.
#[derive(Debug, Clone, PartialEq)]
pub struct PopRecords {
    /// Tier satisfactions [basic, common, luxury] after satisfaction boosts.
    /// Measured as a percentage of success of each desire summed together. 
    /// 
    /// So a tier sat of 3.0 means at worst, 3 desires of that tier were fully satisfied.
    /// Could be more desires at lower satisfaction or fewer with a satisfaction boost.
    /// 
    /// Filled in by the pop itself.
    pub tier_sat: [f64; 3],
    /// `sum(desire.satisfaction)` across all tiers.
    pub satisfaction_units_total: f64,
    /// Living Standard value today. 
    /// A weighted sum of `self.tier_sat` where
    /// `living_stardard = basic + 0.6*Common + 0.4*Luxury`
    /// 
    /// Formula subject to change.
    pub living_standard: f64,
    /// Standard of Living average (calculated daily, stored here for quick access).
    /// 
    /// May remove later.
    pub sol_avg: f64,
    /// The estimated rate of change over the past few days.
    /// 
    /// Estimated by EMA method.
    /// sol_avg 
    pub trend: f64,
    /// The history of the pop's standard of living. Covers `HISTORY_MAX` (currently
    /// 16 turns).
    pub sol_history: CircularBuffer<{ pop_constants::HISTORY_MAX }, f64>,

    // --- Census ---
    /// Household count today. Written in record_keeping.
    pub pop_size: f64,
    /// Household-count history, same length as sol_history.
    pub pop_history: CircularBuffer<{ pop_constants::HISTORY_MAX }, f64>,
    /// Households in minus households out today. Written in the migration phase.
    pub net_migration: f64,
    /// Household-count change from growth_phase (new - old). Not migration.
    /// Should never be >= current household count.
    pub previous_growth: f64,
    /// Labor available today (`Household::total_labor`).
    pub labor: f64,

    // --- Balance sheet ---
    /// AMV of on-hand property: `sum(quantity*price)` (missing prices => 1.0).
    /// 
    /// Filled in by the pop.
    pub wealth_amv: f64,
    /// Spendable / mobile wealth: `Sum(quantity * price * salability)`.
    /// Skips Untradeable goods. Per-household series goes in wealth_history.
    pub liquid_wealth: f64,
    /// Liquid wealth per household, same ring length as sol_history.
    pub wealth_history: CircularBuffer<{ pop_constants::HISTORY_MAX }, f64>,
    /// AMV of goods consumed and used today (last look before decay).
    pub consumption_amv: f64,
    /// AMV gained from wages today. 0.0 until market day pays.
    pub income_amv: f64,
    /// AMV of stock sitting against PopPRow.saved.
    pub saved_amv: f64,
    /// Shop success: AMV on-hand vs shop_target, typically 0.0..=1.0.
    pub shop_fill: f64,

    // --- Planning variables (rewritten in record_keeping, read next market day) ---
    /// Target share of liquid wealth to hold. Drives PopPRow.saved.
    pub savings_ratio: f64,
    /// Personal interest rate. Higher => consume now, demand more return to save/invest.
    pub time_preference: f64,
    /// Fear/greed planning variable in -1.0..=1.0. Not SentimentKind::Fear.
    /// Nudged from sentiment + SOL trend; lerped so one day cannot flip hoarding.
    pub risk_appetite: f64,
}

impl Default for PopRecords {
    fn default() -> Self {
        Self {
            tier_sat: [1.0, 1.0, 1.0],
            satisfaction_units_total: 0.0,
            living_standard: 1.0,
            sol_avg: 1.0,
            trend: 0.0,
            sol_history: CircularBuffer::new(),
            pop_size: 0.0,
            pop_history: CircularBuffer::new(),
            net_migration: 0.0,
            previous_growth: 0.0,
            labor: 0.0,
            wealth_amv: 0.0,
            liquid_wealth: 0.0,
            wealth_history: CircularBuffer::new(),
            consumption_amv: 0.0,
            income_amv: 0.0,
            saved_amv: 0.0,
            shop_fill: 1.0,
            savings_ratio: pop_constants::DEFAULT_SAVINGS_RATIO,
            time_preference: pop_constants::DEFAULT_TIME_PREFERENCE,
            risk_appetite: pop_constants::DEFAULT_RISK_APPETITE,
        }
    }
}

impl PopRecords {
    /// # Update Living Standard
    /// 
    /// Given the current `tier_sat`, update `self.living_standard` to 
    /// reflect the weighted sum of `tier_sat` values.
    /// 
    /// Current formula is `living_stardard = 3.0*basic + 1.5*Common + 1.0*Luxury`.
    pub fn update_living_standard(&mut self) {
        self.living_standard = 
            self.tier_sat[0] * pop_constants::SCORE_WEIGHT_BASIC +
            self.tier_sat[1] * pop_constants::SCORE_WEIGHT_COMMON +
            self.tier_sat[2] * pop_constants::SCORE_WEIGHT_LUXURY;
    }

    /// # Update Trend
    /// 
    /// Updates the record based on updated `tier_sat`, `satisfaction_units_total` and 
    /// `living_standard`.
    /// 
    /// Updates `sol_avg`, `trend`, and `sol_history` based on the current state of the pop.
    /// 
    /// Should be called during `process_satisfaciton` after `update_living_standard`.
    pub fn update_trend(&mut self) {
        // if first day just set the average and trend and move on.
        if self.sol_history.len() == 0 {
            self.sol_avg = self.living_standard;
            self.trend = 0.0;
            self.sol_history.push_back(self.living_standard);
            return;
        }
        // update the average using EMA method.
        let prev_avg = self.sol_avg;
        // weighted rolling average
        self.sol_avg = lerp(self.sol_avg, self.living_standard, pop_constants::ROLLING_AVG_WEIGHT);
        // update the trend based on the change in living standard and the average.
        self.trend = self.living_standard - prev_avg;
        // push the current living standard to the history.
        self.sol_history.push_back(self.living_standard);
    }

    /// Push today's pop_size onto pop_history.
    pub fn push_pop_history(&mut self) {
        self.pop_history.push_back(self.pop_size);
    }

    /// Push liquid wealth per household onto wealth_history.
    /// `0.0` when pop_size is 0.
    pub fn push_wealth_history(&mut self) {
        let per_household = if self.pop_size > 0.0 {
            self.liquid_wealth / self.pop_size
        } else {
            0.0
        };
        self.wealth_history.push_back(per_household);
    }
}

#[cfg(test)]
mod pop_records_should {
    use super::*;

    #[test]
    fn default_sets_planning_values_and_empty_rings() {
        let records = PopRecords::default();
        assert_eq!(records.savings_ratio, pop_constants::DEFAULT_SAVINGS_RATIO);
        assert_eq!(records.time_preference, pop_constants::DEFAULT_TIME_PREFERENCE);
        assert_eq!(records.risk_appetite, pop_constants::DEFAULT_RISK_APPETITE);
        assert_eq!(records.shop_fill, 1.0);
        assert_eq!(records.previous_growth, 0.0);
        assert_eq!(records.pop_history.len(), 0);
        assert_eq!(records.wealth_history.len(), 0);
        assert_eq!(records.sol_history.len(), 0);
    }

    #[test]
    fn push_pop_history_appends_pop_size() {
        let mut records = PopRecords::default();
        records.pop_size = 12.0;
        records.push_pop_history();
        assert_eq!(records.pop_history.len(), 1);
        assert_eq!(records.pop_history[0], 12.0);
    }

    #[test]
    fn push_wealth_history_stores_per_household_liquid() {
        let mut records = PopRecords::default();
        records.liquid_wealth = 20.0;
        records.pop_size = 10.0;
        records.push_wealth_history();
        assert_eq!(records.wealth_history.len(), 1);
        assert!((records.wealth_history[0] - 2.0).abs() < 1e-12);

        records.pop_size = 0.0;
        records.push_wealth_history();
        assert_eq!(records.wealth_history.len(), 2);
        assert_eq!(records.wealth_history[1], 0.0);
    }
}

#[cfg(test)]
mod pop_p_row_saved_should {
    use super::*;

    #[test]
    fn is_quantity_minus_reserved() {
        let row = PopPRow::new(10.0).with_reserve(4.0).with_save_target(99.0);
        assert_eq!(row.saved(), 6.0);
    }

    #[test]
    fn floors_at_zero_when_reserved_exceeds_quantity() {
        let row = PopPRow::new(3.0).with_reserve(5.0);
        assert_eq!(row.saved(), 0.0);
    }
}
