use std::collections::HashMap;

use crate::game::{map::Map, market::Market};

/// # Map Data
/// 
/// Contains and manages the data for map and environmental data.
#[derive(Debug)]
pub struct MapData {
    pub map: Map,                     // Core hexgrid, tiles, navigation
    pub markets: HashMap<usize, Market>,             // Tile-linked or global markets
    // Secondary environmental: weather, pollution, resources regen, etc.
    pub environment: EnvironmentData,
}

/// # Environmental Data
/// 
/// Stores overarching environmental data, like 
#[derive(Debug)]
pub struct EnvironmentData { /* weather, resources, etc. */ }
