use std::{collections::{HashMap, HashSet}, fmt::format};

use crate::good::Good;

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
    pub good_records: HashMap<usize, GoodRecord>,
    pub class_prices: HashMap<usize, f64>,
    pub want_prices: HashMap<usize, f64>,
                       
    pub currencies: HashSet<usize>,
    // Taxes
}

impl MarketHistory {
    pub fn new() -> Self {
        Self { 
            good_records: HashMap::new(), 
            class_prices: HashMap::new(), 
            want_prices: HashMap::new(), 
            currencies: HashSet::new(),
        }
    }

    pub fn with_good_record(mut self,good: usize, record: GoodRecord) -> Self {
        self.good_records.insert(good, record);
        self
    }
    
    /// # Get Record
    ///  
    /// Record shorthand
    pub fn get_record(&self, good: usize) -> &GoodRecord {
        self.good_records.get(&good)
              .expect(
            format!("Good '{}' was not found in market history records.", good).as_str()
        )


    }
}


pub struct GoodRecord {
    /// AMV price
    pub price: f64,
    /// Quantity Produced
    pub production: f64,
    /// Quantity Consumed
    pub consumption: f64,
    /// Quantity Imported into market.
    pub import: f64,
    /// Quantity Exported into market.
    pub export: f64,
    /// Quantity put up for sale.
     pub for_sale: f64,
    /// Quantity 
    pub sold: f64,
}

impl GoodRecord {
    pub fn new() -> Self {
        GoodRecord {
            price: 0.0,
            production: 0.0,
            consumption: 0.0,
            import: 0.0,
            export: 0.0,
            for_sale: 0.0,
            sold: 0.0,
        }
    }

    /// # With Price
    /// 
    /// Sets the price fluently.
    /// 
    /// # Note
    /// 
    /// Currently does not enforce a positive price. May change that, may not.
    pub fn with_price(mut self, price: f64) -> Self {
        self.price = price;
        self
    }                                  


}



 

  