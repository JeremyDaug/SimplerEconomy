use std::collections::{HashMap, HashSet};



/// # Market
/// 
/// A coheisive economic unit in which people, production, and trade occurs.
pub struct Market {
    /// Unique Id of the market
    pub id: usize,
    /// The name of the market.
    pub name: String,
    /// Connections to other markets and the info attached to that connection.
    pub connections: HashMap<usize, MarketConnectionType>,
    /// The local info on goods.
    pub goods_info: HashMap<usize, GoodData>,

    /// Pops in this market.
    pub pops: HashSet<usize>,
    /// Jobs in this market.
    pub jobs: HashSet<usize>,
}

/// # Good Data
/// 
/// Data of a good in a market.
pub struct GoodData {
    /// Abstract Market Value, a helper which creates an understood comparable 
    /// value in the market. This is not a price, but should approximate it when
    /// money exists and is widely used.
    pub amv: f64,
    /// The deneral desireablitiy of the good is to everyone in the market.
    pub salability: f64,
}

pub enum MarketConnectionType {
    Land(f64),
    Sea(f64)
}