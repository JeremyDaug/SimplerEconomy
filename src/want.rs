use std::f64::consts::{E, LN_2};

use crate::constants::MINIMUM_WANT_THRESHOLD;

/// # Want
/// 
/// Wants are abstract desires, non-transferable by nature.
/// 
/// They are consumed at the end of the market turn. 
/// 
/// If not consumed, they will decay over time.
/// 
/// Wants can also have an effect on the owner, which are recorded here.
pub struct Want {
    /// The Unique id of the want.
    pub id: usize,
    /// Name of the want, should be unique.
    pub name: String,
    /// The rate of decay, current_val * (1.0 - decay_rate) = next_val.
    /// 
    /// To keep trace amounts of wants from sticking around forever, 
    /// any want that is below 0.001 units is destroyed regardless.
    pub decay_rate: f64,
    /// The additional effects of a want that are applied to the owner at day's end.
    /// 
    /// This is before any decay, and so comes from having the want, not from
    /// the want decaying itself.
    pub effects: Vec<WantEffect>,
    /// All sources which produce the want via ownership.
    pub ownership_sources: Vec<usize>,
    /// Sources which produce via use.
    pub use_sources: Vec<usize>,
    /// Sources which produce via consumption.
    pub consumption_sources: Vec<usize>,
}

impl Want {
    /// # New
    /// 
    /// Sets name and ID of the want.
    /// 
    /// Decay Rate is set to 1.0, decays to nothing at day end.
    pub fn new(id: usize, name: String) -> Self {
        Self {
            id,
            name,
            decay_rate: 1.0,
            effects: vec![],
            ownership_sources: vec![],
            use_sources: vec![],
            consumption_sources: vec![],
        }
    }

    /// # With Effect
    /// 
    /// Add an effect to the list of effects.
    /// 
    /// Consumes Want.
    pub fn with_effect(mut self, effect: WantEffect) -> Self {
        self.effects.push(effect);
        self
    }

    /// # With Effects
    /// 
    /// Add effects to the list of effects.
    /// 
    /// Consumes Want.
    pub fn with_effects(mut self, effects: Vec<WantEffect>) -> Self {
        self.effects.extend(effects);
        self
    }

    /// # Decays By
    /// 
    /// How much decays each market turn.
    /// 
    /// 1.0 means it all decays each day.
    /// 0.0 means it never decays.
    pub fn decays_by(mut self, decay_rate: f64) -> Self {
        assert!(decay_rate >= 0.0 && decay_rate <= 1.0, "Decay must be between 0.0 and 1.0 inclusive");
        self.decay_rate = decay_rate;
        self
    }

    /// # With Half Life
    /// 
    /// Defines the decay rate as a measure of how long it takes to decay by
    /// % 50.
    pub fn with_half_life(mut self, half_life: f64) -> Self {
        assert!(half_life > 0.0, "Half life must be a positive value.");
        self.decay_rate = E.powf(LN_2 / half_life);
        self
    }

    /// # Decay
    /// 
    /// Decays the given (start) quantity into the outputted result
    /// quantity.
    /// 
    /// This implements the minimum cap of 0.001 units. If the value is less
    /// than that, we return 0.0.
    /// 
    /// Asserts that start must be a positive value.
    pub fn decay(&self, start: f64) -> f64 {
        assert!(start > 0.0, "Start value must be a positive value.");
        let result = start * (1.0 - self.decay_rate);
        if result < MINIMUM_WANT_THRESHOLD {
            0.0
        } else {
            result
        }
    }
}

pub enum WantEffect {

}