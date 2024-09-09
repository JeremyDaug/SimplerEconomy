/// # Desire
/// 
/// A desired good. All pops want 1 per household. Desires may be repeated.
/// The usize contained in both is the specific item desired.
#[derive(Clone, Copy, Debug)]
pub enum Desire {
    /// Good is consumed at the end of the day, when satisfying the desire.
    Consume(usize),
    /// Good is not consumed by owner at the end of the day to satisfy desire.
    Own(usize),
    // TODO Non-Exclusive desire, for goods which are not consumed, but can also satisfy something else?
}

impl Desire {
    /// # Unwrap
    /// 
    /// Extracts the ID of our desired good.
    pub fn unwrap(&self) -> usize {
        match self {
            Desire::Consume(id) |
            Desire::Own(id) => *id,
        }
    }
}