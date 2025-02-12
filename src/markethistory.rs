use std::collections::{HashMap, HashSet};

/// # Market History
/// 
/// Stores the data of yesterday for a market. Used by actors and 
/// processes around the system to make decisions in the local markets.
/// 
/// Should (eventually) include:
/// 
/// - AMV Prices for Wants, Classes, and Goods
/// - A Record of production, consumption, import, and export of goods.
/// - Additionally information, such as currencies and taxes.
pub struct MarketHistory {
    pub good_prices: HashMap<usize, f64>,
    pub class_prices: HashMap<usize, f64>,
    pub want_prices: HashMap<usize, f64>,

    pub good_production: HashMap<usize, f64>,
    pub good_consumption: HashMap<usize, f64>,
    pub good_import: HashMap<usize, f64>,
    pub good_export: HashMap<usize, f64>,
    pub good_for_sale: HashMap<usize, f64>,
    pub good_sold: HashMap<usize, f64>,

    pub currencies: HashSet<usize>,
    // Taxes
}