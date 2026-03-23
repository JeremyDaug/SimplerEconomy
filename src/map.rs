use crate::hex::Hex;

/// # Map
/// 
/// This stores our map in greater detail. 
/// 
/// Stores hex data, and allows to organize and manipulate it.
/// 
/// It also deals with the location and organization of the hexes relative to each other.
/// 
/// The hex grid map is point donw, to create nice horizontal rows.
pub struct Map {
    /// Our hexes, stored in a simple hex. The HexCoord is the location (x, y) of the 
    /// hex in question.
    pub hexes: Vec<Vec<Hex>>,
    /// The Locations of Cities (Region Centers) on the map.
    /// 
    /// We add cities in order of creation and delete (or move) as needed.
    pub cities: Vec<HexCoord>,
    /// The Regions which are under each City. 
    /// 
    /// The regions idx should corrispond to the city's idx.
    pub regions: Vec<Vec<HexCoord>>, 
}

/// # Hex Coordinate
/// 
/// Helper for our map to store and deal with location and algorithms easier.
#[derive(Debug, Copy, Clone, Hash)]
pub struct HexCoord {
    x: i32,
    y: i32,
}

impl HexCoord {
    pub fn z(&self) -> i32 {
        - self.x - self.y
    }
}