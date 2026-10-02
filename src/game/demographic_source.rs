/// # Demographic Source
///
/// Which demographic a record is attached to.
///
/// The value is that demographic's id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DemographicSource {
    /// The pop's species.
    Species(usize),
    /// A culture.
    Culture(usize),
    /// A class.
    ///
    /// TODO: Class demographics are not implemented yet.
    Class(usize),
    /// A religion.
    Religion(usize),
}

impl DemographicSource {
    /// # Id
    ///
    /// The demographic id this source names.
    pub fn id(self) -> usize {
        match self {
            Self::Species(id) | Self::Culture(id) | Self::Class(id) | Self::Religion(id) => id,
        }
    }

    /// # Order Rank
    ///
    /// Sort key for desire ordering: Species, then Culture, then Class, then Religion.
    pub fn order_rank(self) -> u8 {
        match self {
            Self::Species(_) => 0,
            Self::Culture(_) => 1,
            Self::Class(_) => 2,
            Self::Religion(_) => 3,
        }
    }
}
