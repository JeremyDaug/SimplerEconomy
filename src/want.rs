/// # Want
/// 
/// Wants are abstract desires, non-transferable by nature.
/// 
/// They are consumed/destroyed at the end of the market turn.
/// 
/// Wants can also have an effect on the owner, which are recorded here.
pub struct Want {
    /// The Unique id of the want.
    pub id: usize,
    /// Name of the want, should be unique.
    pub name: String,
    /// The additional effects of a want that are applied to the owner at day's end.
    pub effects: Vec<WantEffect>
}

impl Want {
    pub fn new(id: usize, name: String) -> Self {
        Self {
            id,
            name,
            effects: vec![],
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
}

pub enum WantEffect {

}