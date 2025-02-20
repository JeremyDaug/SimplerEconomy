/// # Demographic Row
/// 
/// A Demographic row is used by pops to define the amount of a population in a
/// pop group.
/// 
/// Has a column for each 'layer' of information a pop can have.
/// 
/// Currently, we only use one.
#[derive(Debug, Clone, Copy)]
pub struct DRow {
    /// How many households have this data.
    pub count: f64,
    /// All pops need a base species at all times.
    pub species: usize,
    /// Culture is currently the only additional layer of info for a pop.
    pub culture: Option<usize>,
}