use std::collections::{HashMap, HashSet};

use crate::world::World;



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
    /// A quick finder for those goods which have reached a Salability of 90% 
    /// or higher 
    pub monies: HashSet<usize>,

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
    pub fn market_day(&mut self, world: &mut World) {
        // setup time in all of our pops.
        for pop in self.pops.iter() {
            world.pops.get_mut(pop).expect("Pop not found.")
            .reset_time();
        }
        // Jobs purchase labor for the day.
        for job in self.jobs.iter() {
            world.jobs.get_mut(job).expect("Job not found.")
            .pay_workers(&mut world.pops, &self.goods_info);
        }
    }
    
    /// # Salibility AMV Modifier
    /// 
    /// This is the standard function which defines how a AMV is reduced
    /// for barter purposes when offered.
    /// 
    /// Current function is original (0.5 + sal / 2)
    pub fn salibility_amv_mod(sal: f64) -> f64 {
        0.5 + sal / 2.0
    }
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