use std::collections::HashMap;

use crate::game::{desire::DemoDesire, effects::DemographicEffect, household::DemographicRates};

/// # Culture
/// 
/// The culture of a pop. Defines the common and luxury needs as well as secondary
/// benefits of the culture.
/// 
/// Culture is highly maleable to the player it's attached to.
#[derive(Debug, Clone)]
pub struct Culture {
    /// The unique ID of the culture.
    pub id: usize,
    /// The name of the culture.
    pub name: String,
    /// The ID of the state this is connected to. If a culture is not connected to any
    /// state, it is set to 0.
    pub state: usize,
    /// Demographic desires keyed by `DemoDesire.id` for O(1) lookup.
    /// 
    /// Tier lives on each `DemoDesire`; amounts are scaled for 1 household.
    pub desires: HashMap<usize, DemoDesire>,
    /// The universal effects of Culture on a people with this culture.
    /// 
    /// This is for effects that are not contingent on other factors like desires.
    /// For example, Flat birth/mortality rates, per household culture/research 
    /// generation, household size or efficiency changes, and so on.
    pub culture_effects: Vec<DemographicEffect>,
    /// The Demogarphic effects on a species. Is added to the baseline of a pop's species.
    pub culture_demo_eff: DemographicRates,
    /// A helper flag to mark when a culture has changed, and so pop_households should
    /// also be updated. 
    /// 
    /// TODO: This and the effects of applying a household change should be smoother.
    /// Instead of snapping into place, it should apply in smooth phases to keep massive
    /// population swings from occurring. The current mechanism to keep things more 
    /// smooth is from the data design level. Only allowing small changes to apply with 
    /// each change and forcing those changes to be spread out over time.
    /// The ideal desire would be to have th transitionary effect take place over a few
    /// turns and possibly requiring population growth/shrink to make up the changes
    /// appropriately. That latter part is likely too complicated for our needs.
    pub household_changed: bool,
}

impl Culture {
    /// # New
    /// 
    /// Creates a culture with the given id and name.
    /// State defaults to 0 (no state). Desires start empty.
    pub fn new(id: usize, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            state: 0,
            desires: HashMap::new(),
            culture_effects: vec![],
            culture_demo_eff: DemographicRates::zero(),
            household_changed: false,
        }
    }

    /// Sets the culture's unique ID.
    pub fn with_id(mut self, id: usize) -> Self {
        self.id = id;
        self
    }

    /// Sets the culture's display name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets the connected state ID (0 means none).
    pub fn with_state(mut self, state: usize) -> Self {
        self.state = state;
        self
    }

    /// Adds a demographic desire keyed by its id.
    /// Debug-asserts that the desire's tier is 0, 1, or 2.
    /// Panics if a desire with the same id already exists.
    pub fn with_desire(mut self, desire: DemoDesire) -> Self {
        debug_assert!(desire.tier <= 2, "Desire tier must be 0, 1, or 2.");
        let id = desire.id;
        if self.desires.insert(id, desire).is_some() {
            panic!("DemoDesire {id} already exists on culture {}.", self.id);
        }
        self
    }

    /// Finds a demo desire by id.
    pub fn find_desire(&self, desire_id: usize) -> Option<&DemoDesire> {
        self.desires.get(&desire_id)
    }
}
