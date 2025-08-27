use std::collections::{HashMap, HashSet};

use itertools::Itertools;

use crate::{data::Data, world::World};



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
    /// A quick finder for those goods which have reached a Salability greater 
    /// or equal to MONEY_SALABILITY_THRESHOLD.
    pub monies: HashSet<usize>,
    /// When looking at goods to offer, this is the order buyers should use in
    /// this market.
    pub good_trade_priority: Vec<usize>,

    /// Pops in this market.
    pub pops: HashSet<usize>,
    /// Jobs in this market.
    pub jobs: HashSet<usize>,
    /// Merchants are a special subset of jobs which focus on making money by
    /// buying and selling goods both within the market and abroad.
    /// 
    /// This may be one merchant job for an entire market or multiple, one 
    /// for each connection the market has, this is to be determined.
    pub merchants: HashSet<usize>,
}

impl Market {
    /// # Market Day
    /// 
    /// The market day is called and covers all basic internal actions of the
    /// market.
    /// 
    /// The market day goes through the following steps.
    /// 
    /// 0. Make Time, update all pops with their current available time.
    /// 1. Purchase labor, and pay wages.
    /// 2. Jobs do their work, producing goods for sale.
    /// 3. Sell phase, all pops and jobs say if they are selling, what they are
    ///    selling, and at what price (jobs set price, pops just make offers).
    ///    If a 
    /// 4. Buy Phase, all pops and jobs go around, trying to purchase the goods
    ///    they need.
    /// 5. Consumption phase. Pops consume and get their satisfaction.
    /// 6. Job Recalculation, they measure their success for the day and try to
    ///    grow or shrink, this includes new labor targets they want to reach.
    /// 6. Pop Migration, starving pops die, hungry pops open up for migration
    ///    internal and external, also handle pop growth here.
    /// 7. Job Hiring, With their targets updated, and excess pops available
    ///    for hire, alter hiring and wages to try and meet their labor needs.
    /// 
    /// After the market day, comes the inter-market day, which is when trade
    /// and inter-market migration occurs.
    pub fn market_day(&mut self, world: &mut World, data: &Data) {
        // setup time in all of our pops.
        for pop in self.pops.iter() {
        }
        // Jobs purchase labor for the day.
        for job in self.jobs.iter() {
        }
        // jobs do their work
        // set up selling across the 
    }

    

    /// # Good Trade Priority
    /// 
    /// Recalculates good trade order by our currently defined method.
    /// 
    /// Our current method, Monies followed by ID order... Because I'm 
    /// feeling lazy right now.
    /// 
    /// TODO improve this to be more interesting. It should prioritize monies and salable goods.
    pub fn update_good_trade_priority(&mut self, _data: &Data) {
        self.good_trade_priority.clear();
        // all monies go first in ID order
        for good in self.monies.iter()
            .sorted() {
            self.good_trade_priority.push(*good);
        }
        // all other ids go in next, in id order.
        for good in self.goods_info.keys()
            .filter(|x| !self.monies.contains(&x))
            .sorted() {
            self.good_trade_priority.push(*good);
        }
    }

    /// # Get Good Info
    /// 
    /// A quick helper to get a good's market information.
    /// 
    /// # Panics
    /// 
    /// If good is not found, it will panic. A good being sought should 
    /// ALWAYS exist.
    pub fn get_good_info(&self, good: &usize) -> &GoodData {
        self.goods_info.get(good)
            .expect(format!("Good '{}' not found!", *good).as_str())
    }
    
    /// # Salibility AMV Modifier
    /// 
    /// This is the standard function which defines how a AMV is reduced
    /// for barter purposes when offered.
    /// 
    /// Current function is original (0.5 + sal / 2)
    pub fn salibility_amv_mod(sal: f64) -> f64 {
        let result = 0.5 + sal * 0.5;
        debug_assert!(result <= 1.0, "Salability given is greater than 1.0");
        result
    }
    
    /// # Good Trade Priority
    /// 
    /// A getter for good trade priority. 
    /// 
    /// # Panics
    /// 
    /// This panics when the length of self.good_trade_priority and self.goods_info
    /// are not the same, meaning that one or the other has changed and 
    /// self.update_good_trade_priority() should be run.
    pub fn get_good_trade_priority(&self) -> &[usize] {
        debug_assert!(self.good_trade_priority.len() == self.goods_info.len(), 
            "Market '{}' good - trade priority mismatch. Did you remember to update good_trade_priority?", self.id);
        &self.good_trade_priority
    }
}

/// # Good Data
/// 
/// Data of a good in a market.
/// 
/// TODO Expand to include an AMV history and/or average/rolling change over time and/or volatility.
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