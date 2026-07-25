use crate::game::factuals::Factuals;

pub use crate::game::effects::{EffectScope, InstitutionEffect};

/// # Institution
/// 
/// An Institution is an organization which is not purely focused on economic activity.
/// Institutions operate as as a collection of firms under an overriding directive and 
/// shared resource pool.
/// 
/// Institutions are semi-autonomous. They make their own decisions and property separate 
/// from the player, but players do have high level control of them. 
/// 
/// All Institutions include a list of firms they control and manage. 
/// 
/// Institutions do attempt to maintain at least minimum profitability, but will focus
/// on things beyond profit or loss as well, and often have access to sell unique goods
/// that cannot be found elsewhere like Charity, Piety, Unity, and other 'intangibles'.
/// 
/// Institutions can also oversee Cultures, Classes, and Religions.
/// 
/// ## Note: 
/// 
/// For now, Institutions only cover a few things expressly, more things will be added
/// later.
/// 1. Demographics (Culture, Class, and Religion. Possibly Species later as well).
/// 2. Branches of the State (Administration, Military, Legislature, Judiciary, etc)
/// 3. Special Organizations (Trade Leagues, Mercenary Forces, Academies, etc)
/// 
/// By the Alpha state I expect all Demographics and an example froth the others.
/// 
/// Beta should have mulitple examples of each.
/// 
/// ## Note 2 - Electric Boogaloo
/// 
/// Currently, this is just a very loose skeleton to hold up for other things.
/// 
/// It includes consolidated effects from the institution. A lot more work will be 
/// needed and good thought will need to be put into it's structure.
#[derive(Debug, Clone)]
pub struct Institution {
    /// The Unique Id of the Institution
    pub id: usize,
    /// The player who currently controls the institution.
    pub owner: Option<usize>,

    /// The Name of the institution.
    pub name: String,

    /// The collected bonuses and effects of the institution. 
    /// Most of these are applied to pops, but can be applied to others as well.
    /// See [`InstitutionEffect`] / [`EffectScope`] in `effects`.
    pub effects: Vec<InstitutionEffect>,
}

impl Institution {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            owner: None,
            name: "".into(),
            effects: vec![],
        }
    }

    /// End-of-day bookkeeping for this institution.
    /// Only external input is factuals.
    pub fn record_keeping(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Institution record keeping")
    }
}