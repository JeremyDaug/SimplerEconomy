use crate::game::desire::Desire;

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
    /// The desires of the culture, organized by tier. 
    /// Basic, Common, and Luxury. Basic should be uncommon.
    /// 
    /// The desires here are all scaled to the needs of 1 household.
    pub desires: Vec<Vec<Desire>>,
}