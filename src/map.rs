use bevy::platform::collections::HashMap;
use itertools::Itertools;

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
#[derive(Debug, Clone)]
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

    /// A helper to quickly find the city at a hex location.
    pub city_idcs: HashMap<HexCoord, usize>,
}

impl Map {
    /// # New
    /// 
    /// Creates a new map with the given parameters.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            hexes: vec![vec![Hex::new(); height]; width],
            cities: vec![],
            regions: vec![],
            city_idcs: HashMap::new(),
        }
    }

    /// # Add City
    /// 
    /// Adds a city to our cities and regions safely.
    /// 
    /// Returns true if successful, falso if a city already exists at that location.
    pub fn add_city(&mut self, city: HexCoord) -> bool {
        if self.cities.iter().any(|x| *x == city) {
            // if coordinate is already taken, return out
            false
        } else {
            self.cities.push(city);
            self.regions.push(vec![]);
            true
        }
    }

    pub fn find_city(&mut self, city: HexCoord) -> Option<usize> {
        if let Some((idx, _)) = self.cities.iter().find_position(|x| **x == city) {
            Some(idx)
        } else {
            None
        }
    }
}

/// # Hex Coordinate
/// 
/// Helper for our map to store and deal with location and algorithms easier.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct HexCoord {
    x: usize,
    y: usize,
}

impl HexCoord {
    /// # Z
    /// 
    /// A helper function, should return the value needed for x + y - z to equal 0.
    /// 
    /// We chose to do it this way to make things a bit easier to plug into our 2d array.
    pub fn z(&self) -> usize {
        self.x + self.y
    }
}