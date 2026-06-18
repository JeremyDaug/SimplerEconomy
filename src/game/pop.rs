use std::collections::HashMap;

use crate::game::{desire::Desire, household::HouseholdDef};

#[derive(Debug, Clone)]
pub struct Pop {
    /// The unique ID of the pop. May drop this or replace it to simplify.
    /// 
    /// Should be stored in the market alone. ID may be allowed to be non-unique
    /// between markets, but unique within a market.
    pub id: usize,

    /// The ID of the pop's job. Each pop only has 1 right now.
    pub job: usize,

    ///  The property and details of the property
    pub property: HashMap<usize, PopPRow>,
    
    /// Desires of a pop, a consolidated and organized for satisfaction calculations.
    /// 
    /// Nested Vec of Vecs.
    /// 0: Basic Needs
    /// 1: Common Needs
    /// 2: Luxury Needs
    /// 
    /// When trying to satisfy desires, they will always try to fill all of basic needs 
    /// first, then common needs, then Luxury needs. Once it has filled up Luxury needs
    /// it will repeatedly fill up Luxury needs indefinitely, stoping only when it runs
    /// out of goods to satisfy the desires with.
    pub desires: Vec<Vec<Desire>>,

    /// The working desires of the pop, a flat structure that goes :
    /// Basic Needs -> Common Needs -> Luxury Needs. 
    /// 
    /// If a pop satisfies all of these, Luxury needs will be duplicated and added
    /// to the end. Repeat until they are unable to satisfy any more, or run out of
    /// useable trade goods.
    /// 
    /// When a working desire is done (either full or unable to be satisfied) it is
    /// returned to desires proper.
    pub working_desires: Vec<Desire>,

    /// The demographic breakdown of this pop.
    /// 
    /// This may be expanded to be a vector of Demographic Rows, to consolidate
    /// multiple pops of different cultures into one.
    pub demographics: DemoRow,
}

#[derive(Debug, Clone, Copy)]
pub struct DemoRow {
    /// The Number of households, floating point as growth storage.
    pub count: f64,
    /// The definition of this row's household. This is the sum of the baseline plus
    /// all other demographic effects.
    pub household: HouseholdDef,
    /// The species ID, currently should always be 0, which is default human.
    pub species: usize,
    /// The culture ID, 0 means none.
    pub culture: usize,
    /// The class ID, 0 means none.
    pub class: usize,
    /// The Religious ID, 0 means none.
    pub religion: usize,
}

impl Pop {
    /// # Update Desires
    /// 
    /// Called at the end of each day, after the population has changed in size due to 
    /// growth and migration, this updates the amount requested by desires to correctly 
    /// scale with the new population.
    pub fn update_desires(&mut self) {
        todo!()
    }

    /// # Next Shopping Trip
    /// 
    /// Used during the day, this decides what a pop will want to buy next. It does this
    /// 
    pub fn next_shopping_trip(&self) {
        todo!()
    }

    /// # Consume
    /// 
    /// Consumes goods from `property` to satisfy a pop's desires.
    /// 
    /// Goods should already be reserved and ready to be consumed, so do so.
    /// 
    /// - Basic (0) and Common (1) tiers are processed **once each**, in list order,
    ///   filling to the best of the pop's ability before moving to the next tier.
    /// - Luxury (2) desires are **repeatedly cycled** and overfilled as much as possible
    ///   until no further progress can be made with remaining goods.
    ///
    /// For desires with a bucket of goods, higher-efficiency goods are preferred.
    ///
    /// The results of the consumption is stored in the desires as satisfaction.
    ///
    /// Also mutates `self.property` (reduces `quantity`, increases `consumed`).
    /// 
    /// The function assumes that all desires are currently in `self.desires` and
    /// none are in `self.working_desires`.
    pub fn consume(&mut self) {
        let mut curr_tier = 0;
        let mut working_desires = vec![];
        
        
    }

    /// # Satisfy Tier
    /// 
    /// Takes a list of desires (presumably a tier) and tries to satisfy each desire
    /// in order.
    /// 
    /// Will consume desires for satisfaction.
    /// 
    /// Returns true if any of the desires was fully satisfied.
    pub(crate) fn satify_tier(&mut self, desires: &mut Vec<Desire>) -> bool {
        let mut success = false;
        for desire in desires.iter_mut() {
            let result = self.satisfy_one_desire(desire);
            if result >= 1.0 {
                success = true;
            }
        }
        success
    }

    /// # Satisfy One Desire
    /// 
    /// A helper which takes a single desire and tries to satisfy it to one level. It 
    /// returns it's sucess rate (0.0-1.0) of satisfying the level.
    pub(crate) fn satisfy_one_desire(&mut self, desire: &mut Desire) -> f64 {
        // Clone + sort by efficiency descending (best substitutes first)
        let mut targets = desire.target.clone();
        targets.sort_by(|a, b| b.efficiency.partial_cmp(&a.efficiency).unwrap_or(std::cmp::Ordering::Equal));

        let mut remaining = desire.amount;

        for target in targets.iter() {
            if remaining <=  0.0 {
                break;
            }
            // get the target good, or continue on to the next target.
            if let Some(row) = self.property.get_mut(&target.good) && row.quantity > 0.0 {
                // satisfaction_gained = goods_consumed * eff
                // => goods_needed = remaining / eff
                let needed = remaining / target.efficiency;
                let take = needed.min(row.quantity);

                // Perform the consumption
                row.quantity -= take;
                row.consumed += take;
                let sat_gained = take * target.efficiency;
                desire.satisfaction += sat_gained;
                remaining -= sat_gained;
            }
        }
        // return the current satisfaction rate (as a rate of 0.0 - 1.0)
        (desire.satisfaction / desire.amount) % 1.0 // double check this, I can never remember how Modulo works with floating point values.
    }
}

/// # Pop Property Row
/// 
/// A row of data attached to the property of a pop. This is used for fine tuned
/// property management and consumption details.
/// 
/// This currently only contains the current quantity, the amount reserved for 
/// consumption, and the target they want to reach.
#[derive(Debug, Clone, Copy)]
pub struct PopPRow {
    /// The total amount owned at the moment.
    pub quantity: f64,
    
    /// The Target amount the pop desires to have after all shopping is complete.
    pub target: f64,
    /// The amount that has already been earmarked for the pop's use today.
    pub reserved: f64,

    /// How many of this good was consumed yesterday for desires.
    pub consumed: f64,
}

impl PopPRow {
    pub fn new(quantity: f64) -> Self {
        Self {
            quantity,
            target: 0.0,
            reserved: 0.0,
            consumed: 0.0,
        }
    }

    pub fn available(&self) -> f64 {
        self.quantity - self.reserved
    }
}