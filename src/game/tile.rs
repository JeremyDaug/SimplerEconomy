/// # Hex
/// 
/// Hex is our smallest unit of concrete spacial data. Everything beneath this is 
/// abstracted. 
#[derive(Debug, Clone)]
pub struct Tile {
    /// The region the tile is in.
    pub region: Option<usize>,
}

impl Tile {
    pub fn new(region: Option<usize>) -> Self {
        Self {
            region
        }
    }
}