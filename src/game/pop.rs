use std::collections::HashMap;

use bevy::utils::default;

use crate::game::{desire::{Desire, DesireTargetType}, household::HouseholdDef};

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

    /// # Process Satisfaction
    /// 
    /// This extracts satisfaction data, applies effects from desires, and recalculates
    /// the mood of a pop for political and future needs.
    pub fn process_satisfaction(&mut self) -> () {
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
        
        // first do basic desires, only one pass needed.
        working_desires = self.desires.remove(0); // pop off front
        self.satisfy_tier(&mut working_desires); // satisfy them
        self.desires.insert(0, working_desires); // put back

        // second do common needs, only one pass needed.
        working_desires = self.desires.remove(1); // pop off
        self.satisfy_tier(&mut working_desires); // satisfy
        self.desires.insert(1, working_desires); // put back

        // Last is Luxury Needs, do until we produce no more satisfaction.
        let mut iter_target = 1.0;
        working_desires = self.desires.remove(2); // pop off
        let mut ordered_desires = vec![];
        loop {// loop over desires
            // satisfy the current working desires
            self.satisfy_tier(&mut working_desires);
            // remove any desires not fully satisfied.
            let mut idx = 0;
            loop {
                if idx >= working_desires.len() { break; } // break out if we walk off the end.
                if working_desires[idx].tiers_satisfied() < iter_target {
                    // if not satisfied to our target, move to ordered_desires
                    ordered_desires.push(working_desires.remove(idx));
                } else {
                    // otherwise, increment idx by one and go on
                    idx += 1;
                }
            }
            // if nothing to go onto next time, break out.
            if working_desires.len() == 0 {
                break;
            } else { iter_target += 1.0; } // otherwise increment target and go again.
        } 
        // sasitsfacions done, reorganize
        ordered_desires.sort_by(|a, b| a.idx.cmp(&b.idx));
        self.desires.insert(2, ordered_desires); // put back
    }

    /// # Satisfy Tier
    /// 
    /// Takes a list of desires (presumably a tier) and tries to satisfy each desire
    /// in order.
    /// 
    /// Will consume desires for satisfaction.
    /// 
    /// Returns the highest success rate, useful for checking if any desire reached the 
    /// next full level.
    pub fn satisfy_tier(&mut self, desires: &mut Vec<Desire>) -> f64 {
        let mut success: f64 = 0.0;
        for desire in desires.iter_mut() {
            let result = self.satisfy_one_desire(desire);
            success = success.max(result);
        }
        success
    }

    /// # Satisfy One Desire
    /// 
    /// A helper which takes a single desire and tries to satisfy it to one level. It 
    /// returns final satisfaction level.
    pub(crate) fn satisfy_one_desire(&mut self, desire: &mut Desire) -> f64 {
        // Clone + sort by efficiency descending (best substitutes first)
        let mut targets = desire.target.clone();
        targets.sort_by(|a, b| b.efficiency.partial_cmp(&a.efficiency)
            .unwrap_or(std::cmp::Ordering::Equal));

        let mut remaining = desire.amount;

        for target in targets.iter() {
            if remaining <=  0.0 {
                break;
            }
            // get the target good, or continue on to the next target.
            if let Some(row) = self.property.get_mut(&target.good) && row.consumeable() > 0.0 {
                // remaining (Capped at the cap amount of the desire) divided by 
                // efficiency is how much is needed.
                let needed = remaining.min(desire.amount * target.cap) / target.efficiency;
                let take = needed.min(row.consumeable());

                // remove from quantity and reserve.
                row.quantity -= take;
                row.reserved -= take;
                match target.desire_type {
                    DesireTargetType::Consume => {
                        // shift to consumed.
                        row.consumed += take;
                        let sat_gained = take * target.efficiency;
                        desire.satisfaction += sat_gained;
                        remaining -= sat_gained;
                    },
                    DesireTargetType::Use => {
                        row.used += take;
                        let sat_gained = take * target.efficiency;
                        desire.satisfaction += sat_gained;
                        remaining -= sat_gained;
                    },
                }
            }
        }
        // The current satisfaction rate.
        desire.satisfaction / desire.amount
    }
}

/// # Pop Property Row
/// 
/// A row of data attached to the property of a pop. This is used for fine tuned
/// property management and consumption details.
/// 
/// This currently only contains the current quantity, the amount reserved for 
/// consumption, and the target they want to reach.
#[derive(Debug, Clone, Copy, Default)]
pub struct PopPRow {
    /// The total amount owned at the moment.
    pub quantity: f64,
    
    /// The Target amount the pop desires to have after all shopping is complete.
    /// Simplifies purchases into bulk purchases.
    /// 
    /// Should target roughly 1.0-1.2x of the desire at minimum. When a pop has elevated
    /// savings, fear, or similar 'hodl' moods activated, this target is pushed up.
    /// Supply volatility can also push it up to ensure 
    pub target: f64,

    /// The amount that has already been earmarked for the pop's use today. Used 
    /// to 'prepare' for consumption. Does not remove from quantity.
    pub reserved: f64,
    /// The amount that we wish to preserve between days.
    pub saved: f64,

    /// How many of this good was consumed for today's desires.
    /// All goods here are decayed at the end of the day.
    pub consumed: f64,
    /// Goods that were used for use desires, but not consumed, are stored here
    /// until the end of the day. At day's end, goods here decay normally, and are
    /// returned to total quantity.
    pub used: f64,
}

impl PopPRow {
    pub fn new(quantity: f64) -> Self {
        Self {
            quantity,
            ..default()
        }
    }

    /// # Exchange
    /// 
    /// `quantity` - `target`.
    /// 
    /// This gives the difference between the target to reach and the amount a pop owns.
    /// 
    /// Negative values are how much they will want to buy. Positive values are how many
    /// they are willing to offer or sell.
    pub fn exchange(&self) -> f64 {
        self.quantity - self.target
    }

    /// # Consumeable
    /// 
    /// `quantity` - `saved`.
    /// 
    /// Gives the difference between Current quantity and the amount a pop wants to
    /// save between days.
    /// 
    /// Should only be negative after decay has occurred.
    /// 
    /// Useful for picking out goods for consumption without overconsuming them.
    /// 
    /// This is typically a fraction of the target, and is modified along with target 
    /// and 
    pub fn consumeable(&self) -> f64 {
        self.quantity - self.saved
    }

    /// # Available
    /// 
    /// `quantity` - `reserved`.
    /// 
    /// This gives the amount of goods a pop has that have yet to be claimed by 
    /// another desire. Should always be non-negative.
    pub fn available(&self) -> f64 {
        self.quantity - self.reserved
    }
}