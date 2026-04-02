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
    /// The claims on the tile across the board.
    pub claims: HashMap<usize, usize>,
    /// The current owner, regardless of claims.
    pub owner: Option<usize>,
}

impl Tile {
    pub fn new(hex: Hex) -> Self {
        Self {
            hex,
            region: None,
            claims: HashMap::new(),
            owner: None,
        }
    }

    /// Region fluent setter.
    pub fn in_region(mut self, region: usize) -> Self {
        self.region = Some(region);
        self
    }
}