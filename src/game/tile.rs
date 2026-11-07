use std::collections::HashMap;

use hexx::Hex;

/// # Hex
///
/// Hex is our smallest unit of concrete spacial data. Everything beneath this is
/// abstracted.
#[derive(Debug, Clone)]
pub struct Tile {
    /// The Hex location of the tile, effectively the tile's unique ID.
    pub hex: Hex,
    /// The region the tile is in.
    pub region: Option<usize>,
    pub claims: HashMap<usize, f32>,
    /// The current owner, regardless of claims.
    /// If this is set to empty, then it is either unowned or
    /// owned by whoever has the highest claim on it.
    pub occupier: Option<usize>,
}

impl Tile {
    /// # New
    ///
    /// A tile at `hex` with no region, no claims, and no occupier.
    pub fn new(hex: Hex) -> Self {
        Self {
            hex,
            region: None,
            claims: HashMap::new(),
            occupier: None,
        }
    }

    /// # In Region
    ///
    /// Sets the region and returns this tile.
    pub fn in_region(mut self, region: usize) -> Self {
        self.region = Some(region);
        self
    }
}
