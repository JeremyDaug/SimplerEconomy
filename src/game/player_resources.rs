//! # Player Resources
//!
//! Non-good stocks players spend and accumulate: culture points, research,
//! legitimacy, authority, and faith. Used as a **daily yield bag** (pop extract
//! returns this) and as the **pool** stored on [`crate::game::state::State`].
//!
//! These are not goods. They never live in property rows.

/// # Player Resources
///
/// Named fields for the vanilla non-good player stocks. Adding a sixth vanilla
/// resource is a new field here (default `0.0`) plus matching effect arms.
///
/// A catch-all `PlayerResource` effect arm (id + amount) is likely later if
/// more stocks appear; this bag would then grow an `extra` map. Not added yet.
///
/// Negatives are allowed. There is no floor at 0. Going below zero applies
/// increasing penalties to the player and pops in their territory (state /
/// orchestrator, not this type).
///
/// Daily flow on the player's pool: accumulate from pops and firms, spend on
/// actions, then decay. **Culture** and **research** do not decay. **Faith**,
/// **legitimacy**, and **authority** do, which behaves like a rest value without
/// extra recovery for being far below rest.
///
/// TODO: Age-band scaling of *yields* (elders extra culture/research, children
/// less legitimacy). Household members currently count equally.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlayerResources {
    pub culture: f64,
    pub research: f64,
    pub legitimacy: f64,
    pub authority: f64,
    pub faith: f64,
}

impl PlayerResources {
    /// # New
    ///
    /// All stocks start at `0.0`.
    pub fn new() -> Self {
        Self::default()
    }

    /// True when every stock is exactly `0.0`.
    pub fn is_zero(&self) -> bool {
        self.culture == 0.0
            && self.research == 0.0
            && self.legitimacy == 0.0
            && self.authority == 0.0
            && self.faith == 0.0
    }

    /// Sets culture points.
    pub fn with_culture(mut self, culture: f64) -> Self {
        debug_assert!(culture.is_finite(), "culture must be finite");
        self.culture = culture;
        self
    }

    /// Sets research points.
    pub fn with_research(mut self, research: f64) -> Self {
        debug_assert!(research.is_finite(), "research must be finite");
        self.research = research;
        self
    }

    /// Sets legitimacy.
    pub fn with_legitimacy(mut self, legitimacy: f64) -> Self {
        debug_assert!(legitimacy.is_finite(), "legitimacy must be finite");
        self.legitimacy = legitimacy;
        self
    }

    /// Sets authority.
    pub fn with_authority(mut self, authority: f64) -> Self {
        debug_assert!(authority.is_finite(), "authority must be finite");
        self.authority = authority;
        self
    }

    /// Sets faith.
    pub fn with_faith(mut self, faith: f64) -> Self {
        debug_assert!(faith.is_finite(), "faith must be finite");
        self.faith = faith;
        self
    }
}

impl std::ops::Add for PlayerResources {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        debug_assert!(
            self.culture.is_finite()
                && self.research.is_finite()
                && self.legitimacy.is_finite()
                && self.authority.is_finite()
                && self.faith.is_finite()
                && rhs.culture.is_finite()
                && rhs.research.is_finite()
                && rhs.legitimacy.is_finite()
                && rhs.authority.is_finite()
                && rhs.faith.is_finite(),
            "player resource amounts must be finite"
        );
        Self {
            culture: self.culture + rhs.culture,
            research: self.research + rhs.research,
            legitimacy: self.legitimacy + rhs.legitimacy,
            authority: self.authority + rhs.authority,
            faith: self.faith + rhs.faith,
        }
    }
}

impl std::ops::AddAssign for PlayerResources {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

#[cfg(test)]
mod player_resources_should {
    use super::*;

    #[test]
    fn new_is_zero() {
        let bag = PlayerResources::new();
        assert!(bag.is_zero());
    }

    #[test]
    fn with_setters_and_add_combine_fields() {
        let a = PlayerResources::new()
            .with_culture(2.0)
            .with_research(1.0)
            .with_legitimacy(-0.5);
        let b = PlayerResources::new()
            .with_culture(3.0)
            .with_faith(4.0)
            .with_authority(0.25);
        let sum = a + b;
        assert!((sum.culture - 5.0).abs() < 1e-12);
        assert!((sum.research - 1.0).abs() < 1e-12);
        assert!((sum.legitimacy + 0.5).abs() < 1e-12);
        assert!((sum.authority - 0.25).abs() < 1e-12);
        assert!((sum.faith - 4.0).abs() < 1e-12);
        assert!(!sum.is_zero());
    }
}
