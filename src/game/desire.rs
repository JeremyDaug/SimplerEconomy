
/// # Desire
/// 
/// A Desire is things or groups of things that are desired by a pop.
/// 
/// 
#[derive(Debug, Clone)]
pub struct Desire {
    /// Useful Identifier which points back to where this desire comes from.
    pub source: DesireSource,

    /// The goods beings desired. If of length 1, then it's a specific good,
    /// if it's multiple, then it's a bucket.
    /// 
    /// The f64 part is the 'effeciency' of the good. Ignored if of length 1.
    pub target: Vec<(usize, f64)>,

    /// The amount of units needed 
    pub amount: f64,
}

/// # Desire Source
/// 
/// Where is the desire's definition derived from.
#[derive(Debug, Clone, Copy)]
pub enum DesireSource {
    /// Desire is sourced from the pop's biological needs.
    Species(usize),
    /// Desire is sourced from a Culture.
    Culture(usize),
    /// Desire is sourced from a class (Not currently used).
    Class(usize),
    /// Desire is sourced from a religion.
    Religion(usize),
}

impl DesireSource {
    /// # Unwrap
    /// 
    /// Gets the ID of the Desire Source.
    pub fn unwrap(&self) -> &usize {
        match self {
            DesireSource::Species(id) |
            DesireSource::Culture(id) |
            DesireSource::Class(id) |
            DesireSource::Religion(id) => id,
        }
    }
}