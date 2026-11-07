#![allow(dead_code)]

use std::collections::HashMap;

use crate::game::factuals::Factuals;
use crate::game::scalingfactor::ScalingFactor;
use crate::game::{
    desire::Desire,
    household::Household,
    job::Job,
    sentiment::Sentiment,
};

pub use crate::game::effects::PopEffect;
pub use crate::game::pop_property::{DemoRow, PopPRow, PopRecords};

/// A population slice: one household block, its goods, and its desires.
#[derive(Debug, Clone)]
pub struct Pop {
    pub id: usize,

    /// This pop's cottage work. Each pop keeps its own plan.
    pub job: Job,

    /// Goods on hand.
    pub property: HashMap<usize, PopPRow>,
    /// The currency savings of the pop. Anything in here is removed from [`Self::property`].
    /// It should not be counted for trade or use, and should be returned to property 
    /// after market phase so it can still get used/decayed properly.
    savings: HashMap<usize, f64>,

    /// Desires grouped by tier: 0 basic, 1 common, 2 luxury.
    pub desires: Vec<Vec<Desire>>,
    /// Where a pop is in satisfying it's desires.
    desire_cursor: Option<SatisfyCursor>,

    /// Species and culture for this slice.
    pub demographics: DemoRow,

    /// Share of the market population ceiling, in people. `None` leaves this pop out.
    pub population_cap: Option<f64>,

    /// Net households gained from internal migration this day.
    ///
    /// Negative is households that left. [`Market::internal_migration`]
    /// writes it and clears it at the start of that pass. A pop that
    /// stays is `0`. This is the household count, not people.
    pub migration: f64,

    /// Same-day effects waiting for growth or decay.
    pub stored_effects: Vec<PopEffect>,

    /// Mood shares. [`Self::growth_phase`] scales births per woman from these.
    pub sentiment: Sentiment,

    /// Day-end records. The struct is a placeholder.
    pub records: PopRecords,
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
        Self {
            id,
            job: Job::none(),
            property: HashMap::new(),
            savings: HashMap::new(),
            desires: vec![vec![]; 3],
            desire_cursor: None,
            demographics: DemoRow {
                household: Household::new(),
                species: 0,
                culture: 0,
            },
            population_cap: None,
            migration: 0.0,
            stored_effects: vec![],
            sentiment: Sentiment::new(),
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
    /// A count above one is that count. Anything else is one household.
    pub(crate) fn household_count(&self) -> f64 {
        let count = self.demographics.household.count;
        if count.is_finite() && count > 1.0 {
            count
        } else {
            1.0
        }
    }

    /// # Has Desire For
    ///
    /// True when a desire target lists `good` at a positive efficiency.
    pub(crate) fn has_desire_for(&self, good: usize) -> bool {
        self.desires.iter().flatten().any(|desire| {
            desire
                .target
                .iter()
                .any(|target| target.good == good && target.efficiency > 0.0)
        })
    }

    /// # Makes Good
    ///
    /// True when a job line outputs `good`.
    ///
    /// `factuals` supplies the process. A missing process does not count.
    /// An output of zero does not count.
    pub(crate) fn makes_good(&self, factuals: &Factuals, good: usize) -> bool {
        self.job.lines.iter().any(|line| {
            factuals.get_process(line.process).is_some_and(|process| {
                process
                    .outputs
                    .iter()
                    .any(|output| output.good == good && output.amount > 0.0)
            })
        })
    }

    /// # Takes Good
    ///
    /// True when a job line inputs `good`.
    ///
    /// `factuals` supplies the process. A missing process does not count.
    /// An input of zero does not count.
    pub(crate) fn takes_good(&self, factuals: &Factuals, good: usize) -> bool {
        self.job.lines.iter().any(|line| {
            factuals.get_process(line.process).is_some_and(|process| {
                process
                    .inputs
                    .iter()
                    .any(|input| input.good == good && input.amount > 0.0)
            })
        })
    }
}