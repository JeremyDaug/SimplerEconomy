use std::collections::HashMap;

use crate::game::{
    desire::DemoDesire, effects::DemographicEffect, household::DemographicRates,
};

/// # Stratum
///
/// A demographic layer of one culture. It carries desires, effects, and
/// rate addends the same way a culture does.
///
/// `culture` is the culture this stratum derives from. Id `0` is empty,
/// the same as culture `0`, and is not stored on [`crate::game::factuals::Factuals`].
#[derive(Debug, Clone)]
pub struct Stratum {
    /// The unique ID of the stratum.
    pub id: usize,
    /// The name of the stratum.
    pub name: String,
    /// The culture this stratum derives from.
    ///
    /// `0` is the empty culture. A stored stratum names a real culture.
    pub culture: usize,
    /// The ID of the state this is connected to. If a stratum is not connected
    /// to any state, it is set to 0.
    pub state: usize,
    /// Demographic desires keyed by `DemoDesire.id` for O(1) lookup.
    ///
    /// Tier lives on each `DemoDesire`; amounts are scaled for 1 household.
    pub desires: HashMap<usize, DemoDesire>,
    /// The universal effects on people in this stratum.
    ///
    /// This is for effects that are not contingent on other factors, like desires.
    pub stratum_effects: Vec<DemographicEffect>,
    /// Rate addends for this stratum. Added after the culture's rates.
    ///
    /// Defaults to 0.
    pub stratum_demo_eff: DemographicRates,
    /// Added to the species work-time fraction. `0` leaves that value alone.
    ///
    /// The stacked result is clamped in
    /// [`crate::game::factuals::Factuals::work_time_fraction`].
    pub work_time_fraction: f64,
    /// When true, pops should refresh effective demographic rates this turn.
    ///
    /// TODO: Smoother multi-turn application of large rate swings if needed.
    pub household_changed: bool,
}

impl Stratum {
    /// # New
    ///
    /// A stratum with `id`, `name`, and the `culture` it derives from.
    ///
    /// State defaults to 0. Desires start empty. Rate and work-time addends
    /// start at 0.
    pub fn new(id: usize, name: impl Into<String>, culture: usize) -> Self {
        Self {
            id,
            name: name.into(),
            culture,
            state: 0,
            desires: HashMap::new(),
            stratum_effects: vec![],
            stratum_demo_eff: DemographicRates::zero(),
            work_time_fraction: 0.0,
            household_changed: false,
        }
    }

    /// # With Id
    ///
    /// Sets the stratum's unique ID and returns it.
    pub fn with_id(mut self, id: usize) -> Self {
        self.id = id;
        self
    }

    /// # With Name
    ///
    /// Sets the stratum's display name and returns it.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// # With Culture
    ///
    /// Sets the culture this stratum derives from and returns it.
    ///
    /// This does not edit that culture's list. [`crate::game::factuals::Factuals::add_stratum`]
    /// records the id there when the stratum is stored.
    pub fn with_culture(mut self, culture: usize) -> Self {
        self.culture = culture;
        self
    }

    /// # With State
    ///
    /// Sets the connected state ID and returns this stratum.
    ///
    /// `0` means none.
    pub fn with_state(mut self, state: usize) -> Self {
        self.state = state;
        self
    }

    /// # With Desire
    ///
    /// Adds a demographic desire keyed by its id and returns this stratum.
    ///
    /// Debug-asserts that the desire's tier is 0, 1, or 2. Panics if a
    /// desire with the same id already exists.
    pub fn with_desire(mut self, desire: DemoDesire) -> Self {
        debug_assert!(desire.tier <= 2, "Desire tier must be 0, 1, or 2.");
        let id = desire.id;
        if self.desires.insert(id, desire).is_some() {
            panic!("DemoDesire {id} already exists on stratum {}.", self.id);
        }
        self
    }

    /// # Find Desire
    ///
    /// The demographic desire stored under `desire_id`.
    ///
    /// Returns `None` when this stratum has no desire with that id.
    pub fn find_desire(&self, desire_id: usize) -> Option<&DemoDesire> {
        self.desires.get(&desire_id)
    }
}
