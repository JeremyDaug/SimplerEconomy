use bevy::platform::collections::HashSet;
use hexx::Hex;

use super::tile::Tile;

/// # Region
/// 
/// A region is the consolidated information of a collection of tiles, as well as the 
/// local market tissue.
/// 
/// This is what is called on internal market activities and the like.
/// 
/// Regions hold 'modable' tiles, the tiles as they are right now, rather than the
/// starting state of them, which the map holds.
#[derive(Debug, Clone)]
pub struct Region {
    /// The ID of the region, this should match it's index in the map as well as
    /// the ID of the market which oversees the region.
    pub id: usize,
    /// The hex territory the region covers, stored as a hash set for easier finds.
    pub territory: HashSet<Hex>,
    /// The tiles in the region.
    /// 
    /// These are the active, and extant tiles as has been modified.
    /// 
    /// These can be copied up into the map periodically, but only 
    /// long term changes should do this.
    pub tiles: Vec<Vec<Tile>>,
}