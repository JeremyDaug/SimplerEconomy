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
    /// 
    /// Claims range from 0-4 inclusive, and slides up and down.
    /// 0, uninterested/no claim - No interest or desire in the tile.
    /// 1, minimal claim - Some interest and will be alerted of actions to take the 
    ///    claim.
    /// 2, Claim - High Interest, will likely actively defend and seek to claim.
    /// 3, Righteous Claim, Understood claim, in the process of gaining ownership, or 
    ///    has clear ownership claim.
    /// 4, Owned, Currently occupied the the claimant. reaching this status, locks out 
    ///    others from claiming it, reducing and capping their claim at 3.5.
    /// 
    /// This can be overridden by the occupier. Occupiers control and gain claims over
    /// normal rules, but should they lose their occupier status, the ownership will 
    /// either enter limbo to be negotiated over, or return to the current highest 
    /// claimant.
    pub claims: HashMap<usize, f32>,
    /// The current owner, regardless of claims.
    /// If this is set to empty, then it is either unowned or 
    /// owned by whoever has the highest claim on it.
    pub occupier: Option<usize>,
}

impl Tile {
    pub fn new(hex: Hex) -> Self {
        Self {
            hex,
            region: None,
            claims: HashMap::new(),
            occupier: None,
        }
    }

    /// Region fluent setter.
    pub fn in_region(mut self, region: usize) -> Self {
        self.region = Some(region);
        self
    }
}