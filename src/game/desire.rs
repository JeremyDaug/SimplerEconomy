
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
    pub target: Vec<(usize, f64)>
}

/// # Desire Source
/// 
/// Where is the desire's definition derived from.
#[derive(Debug, Clone, Copy)]
pub enum DesireSource {
    Species(usize),
    Culture(usize),
    Class(usize),
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