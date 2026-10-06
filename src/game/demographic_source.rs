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
    /// A stratum of a culture. This is the economic subgroup, in place of a class.
    Stratum(usize),
    /// A religion.
    Religion(usize),
}

impl DemographicSource {
    /// # Id
    ///
    /// The demographic id this source names.
    pub fn id(self) -> usize {
        match self {
            Self::Species(id)
            | Self::Culture(id)
            | Self::Stratum(id)
            | Self::Religion(id) => id,
        }
    }

    /// # Order Rank
    ///
    /// Sort key for desire ordering: Species, then Culture, then Stratum, then Religion.
    pub fn order_rank(self) -> u8 {
        match self {
            Self::Species(_) => 0,
            Self::Culture(_) => 1,
            Self::Stratum(_) => 2,
            Self::Religion(_) => 3,
        }
    }
}
