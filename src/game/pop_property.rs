//! Property rows, demographics row, and day-end records for pops.

use bevy::utils::default;
use circular_buffer::CircularBuffer;

use crate::game::config::pop_constants;
use crate::game::household::HouseholdDef;
use crate::game::util::lerp;

/// Demographic breakdown of a pop (one row for now).
#[derive(Debug, Clone, Copy)]
pub struct DemoRow {
    /// Number of households (floating point for growth storage).
    ///
    /// Living pops always have `count ≥ 1.0`. After growth, if count would be
    /// `< 1.0` it is snapped to `0.0` (destroyed / pending cleanup). There is no
    /// stable living pop with `0 < count < 1`.
    pub count: f64,
    /// Household definition: baseline plus demographic modifiers.
    pub household: HouseholdDef,
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
        self.count * self.household.size()
    }

    pub fn adult_pop(&self) -> f64 {
        self.count * self.household.adults
    }

    pub fn elder_pop(&self) -> f64 {
        self.count * self.household.elders
    }

    pub fn children_pop(&self) -> f64 {
        self.count * self.household.children
    }

    pub fn labor(&self) -> f64 {
        self.count * self.household.labor()
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
    pub shop_target: f64,

    /// Wish-to-preserve between days (hoarding target). Not a hard fence on consume.
    pub saved: f64,

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

    pub fn with_saved(mut self, saved: f64) -> Self {
        self.saved = saved;
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
    /// AMV of on-hand property: `sum(quantity*price)` (missing prices => 1.0).
    /// 
    /// Filled in by the pop.
    pub wealth_amv: f64,
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
}

impl Default for PopRecords {
    fn default() -> Self {
        Self {
            tier_sat: [1.0, 1.0, 1.0],
            wealth_amv: 0.0,
            satisfaction_units_total: 0.0,
            living_standard: 1.0,
            sol_avg: 1.0,
            trend: 0.0,
            sol_history: CircularBuffer::new(),
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
}
