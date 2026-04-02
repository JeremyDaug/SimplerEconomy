use std::collections::HashMap;

use super::pop::Pop;

/// # Market
/// 
/// The market holds the actors and acts on stuff. It is what abstracts physical details
/// from the region away and consolidates it into the goods that can be bought, sold,
/// and traded.
/// 
/// Things which can't be moved out of a region can't be moved out of a market, and so on.
#[derive(Debug, Clone)]
pub struct Market {
    /// The unique ID of the market, should match the ID of the region it represents.
    pub id: usize,
    /// The pops in the market.
    pub pops: HashMap<usize, Pop>,
}