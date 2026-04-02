use std::ops::Rem;

use bevy::platform::collections::{HashMap, HashSet};
use hexx::Hex;
use itertools::Itertools;
use super::tile::Tile;

/// # Map
/// 
/// This stores our map in greater detail. 
/// 
/// Stores hex data, and allows to organize and manipulate it.
/// 
/// It also deals with the location and organization of the hexes relative to each other.
/// 
/// The hex grid map is point donw, to create nice horizontal rows.
/// 
/// This is an Axial grid with pointy top. Each row is offset by half to the right 
/// from the row below, while hexes directly to the left and right
/// 
/// ## Wrapping
/// 
/// The map is allowed to wrap (or not) in both the vertical or horizontal directions.
/// If neither the map is flat and has corners.
/// If it only wraps in one, it's cylyndrical.
/// If in both, it's torroidal.
#[derive(Debug, Clone)]
pub struct Map {
    /// The number of rows.
    pub height: usize,
    /// Number of Columns.
    pub width: usize,

    /// Vertical Wrapping.
    pub vwrap: bool,
    /// Horizontal Wrapping.
    pub hwrap: bool,

    /// Our hexes, stored in a simple hex. The HexCoord is the location (x, y) of the 
    /// hex in question.
    pub tiles: Vec<Vec<Tile>>,
    /// The regions which currently exist.
    /// 
    /// First hex in each region is the current capital of the region.
    /// 
    /// For quick region lookup, look at the Tile's region rather than doing an 
    /// iterative search.
    pub regions: Vec<Vec<Hex>>,
}

impl Map {
    /// # New
    /// 
    /// Creates a new map with the given parameters.
    /// 
    /// Assumes no wrapping.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            vwrap: false,
            hwrap: false,
            tiles: vec![vec![Tile::new(None); height]; width],
            regions: vec![],
        }
    }

    /// Sets vwrap to true.
    pub fn with_vwrap(mut self) -> Self {
        self.vwrap = true;
        self
    }

    /// Sets hwrap to true.
    pub fn with_hwrap(mut self) -> Self {
        self.hwrap = true;
        self
    }

    /// # Wrap
    /// 
    /// Takes a hex and wraps it to within our height/width bounds.
    /// 
    /// If hex is outside our bounds and we don't wrap, we return None as a safety mechanism.
    pub fn wrap(&self, hex: Hex) -> Option<Hex> {
        let mut res = hex.clone();
        if hex.x < 0 || hex.x >= self.width as i32 {
            if !self.hwrap {
                return None;
            } else {
                res.x = hex.x.rem_euclid(self.width as i32);
            }
        }

        if hex.y < 0 || hex.y >= self.height as i32 {
            if !self.vwrap {
                return None;
            } else {
                res.y = hex.y.rem_euclid(self.height as i32);
            }
        }

        Some(res)
    }

    /// # Get Region
    /// 
    /// Gets the region a hex is in. Returns the idx of the region.
    /// 
    /// If the hex is not in a region, it returns None.
    /// 
    /// Tiles keep track of this, so we can just ask them.
    pub fn get_region(&self, hex: Hex) -> Option<usize> {
        if let Some(tile) = self.get_tile(hex) {
            tile.region
        } else {
            None
        }
    }

    /// # Find City
    /// 
    /// Given a hex, find the city tile which oversees it.
    /// 
    /// Returns the hex of the region's capital, or None if not in a region.
    pub fn find_city(&mut self, hex: Hex) -> Option<Hex> {
        if let Some(region) = self.get_region(hex) {
            Some(self.regions[region][0])
        } else {
            None
        }
    }

    /// # Get Cities
    /// 
    /// Gets all of the cities in the map. 
    /// 
    /// Cities are stored in the first spot of a region, their location in this
    /// list is the same location in the region's list.
    pub fn get_cities(&self) -> Vec<Hex> {
        self.regions.iter().map(|x| x[0]).collect_vec()
    }

    /// # Get Tile
    /// 
    /// Gets the tile at the given hex, wrapping as needed.
    /// 
    /// Can return None and does not return mutable tile.
    pub fn get_tile(&self, hex: Hex) -> Option<&Tile> {
        if let Some(res) = self.wrap(hex) {
            return Some(&self.tiles[res.x as usize][res.y as usize]);
        }
        None
    }

    /// Internal getter for tiles. Skips safety checks, does NOT wrap the hex and returns
    /// the Tile. Use **Cautiously!**
    fn unsafe_get_tile(&self, hex: Hex) -> &Tile {
        &self.tiles[hex.x as usize][hex.y as usize]
    }

    /// Internal get tile mut helper. Unsafe, and does NOT wrap the hex so use 
    /// **cautiously**.
    fn unsafe_get_tile_mut(&mut self, hex: Hex) -> &mut Tile {
        &mut self.tiles[hex.x as usize][hex.y as usize]
    }

    /// # Get Region Neigbors
    /// 
    /// Gets all hexes that are adjacent to the region id selected.
    /// 
    /// This includes any wrapping of the map.
    pub fn get_region_neighbors(&self, region: usize) -> Vec<Hex> {
        let mut coverage = HashSet::new();

        // get region and neighbors
        let region = &self.regions[region];
        for hex in region.iter() {
            coverage.insert(*hex);
            for neighbors in hex.all_neighbors() {
                coverage.insert(neighbors);
            }
        }
        // remove region to leave neighbors
        for hex in region.iter() {
            coverage.remove(hex);
        }
        // then wrap coverage to within our bounds, or cull those outside of it.
        let mut result = vec![];
        for hex in coverage.iter() {
            if let Some(wrapped) = self.wrap(*hex) {
                result.push(wrapped);
            }
        }

        result
    }

    /// # Is region Neighbor
    /// 
    /// Checks that a hex is adjacent to an existing region.
    /// 
    /// This is slightly simplified from get_region_neighbors, instead just
    /// looking for any hex in the selected region which is of distance 1 away and 
    /// not already in the region.
    /// 
    /// This will panic if region is out of bounds of our current regions list.
    pub fn is_region_neighbor(&self, region: usize, hex: Hex) -> bool {
        // check that we can get the tile,
        if let Some(tile) = self.get_tile(hex) {
            if let Some(r) = tile.region && r == region{
                // if in the targeted region, gtfo.
                return false;
            }
            // get distance to our region
            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
            // TODO! Deal with this here!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!1
            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
            let wrapped = self.wrap(hex).unwrap() + Hex::new(self.width as i32, self.height as i32);
            for loc in self.regions[region].iter() {
                // if distance 1, it must be adjacent 
                // (ignore wrapping as that was checked above)
                if loc.distance_to(hex) == 1 {
                    return true;
                }
                // include wrapped to deal with multiples
                if loc.distance_to(wrapped) == 1 {
                    return true;
                }
            }
            return false;
        }
        false
    }

    /// # Add region
    /// 
    /// Adds a new region to our map. This new region starts as just the capital of the
    /// region.
    /// 
    /// Returns true if successful, false if the hex is already in a region.
    /// 
    /// Note:
    /// 
    /// This can safely add values outside of the map, it just modulo's them to
    /// within the map. Do be aware of that. Reminder that the last row/column is 
    /// height/width - 1.
    /// 
    /// To create a region from an existing region, you'll want to do a split and move
    /// instead.
    pub fn add_city(&mut self, city: Hex) -> bool {
        // restrict to our bounds
        if let Some(city) = self.wrap(city) {
            // check that the city hex is not in a region.
            if let Some(_) = self.get_region(city) {
                false
            } else {
                // if not, already in a region, it's safe to add.
                // add to tile
                self.unsafe_get_tile_mut(city).region = Some(self.regions.len());
                // then to region.
                self.regions.push(vec![city]);
                true
            }
        } else {
            false
        }
    }

    /// # Add to Region
    /// 
    /// Adds a hex to the selected region.
    /// 
    /// Returns true if successful, false otherwise.
    /// 
    /// Regions must be contiguous, including wrapping.
    /// 
    /// This does not move hexes from one region to another.
    /// 
    /// This can panic if the region does not exist.
    pub fn add_to_region(&mut self, region: usize, hex: Hex) -> bool {
        // check it is a tile
        if let Some(tile) = self.get_tile(hex) {
            println!("{:?}", hex);
            if let Some(_) = tile.region {
                // if in any region, gtfo
                return false;
            }
            
            if self.is_region_neighbor(region, hex) {
                // if a valid neighbor, add to the region
                println!("{:?}", hex);
                let wrapped = self.wrap(hex).unwrap();
                self.regions[region].push(wrapped);
                self.unsafe_get_tile_mut(wrapped).region = Some(region);
                return true;
            }
            // otherwise, return false.
            false
        } else {
            // if cannot be wrapped, we cannot add.
            false
        }
    }
}