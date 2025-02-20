use crate::desire::Desire;


/// # Culture
/// 
/// A common storage place for data used by pops. Currently only covers 
/// culture, species, and other factors are ignored and will need their own
/// storage most likely.
/// 
/// This currently only stores the desires of the pop.
pub struct Culture {
    /// The unique id of the culture.
    pub id: usize,
    /// The unique name of the culture.
    pub name: String,
    /// The desire track of the culture.
    pub desires: Vec<Desire>,
    // TODO: Culture Modifiers
    // TODO: Culture Tech Storage.
}

impl Culture {
    pub fn new(id: usize, name: String) -> Culture {
        Culture {
            id,
            name,
            desires: vec![],
        }
    }
}