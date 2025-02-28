use std::{collections::HashMap, mem::discriminant};

use itertools::Itertools;

use crate::{data::Data, desire::{Desire, DesireTag}, drow::DRow, household::Household, market::Market};

/// # Pop
/// 
/// A number of households grouped together into one unit.
#[derive(Debug, Clone)]
pub struct Pop {
    /// Unique Id of the pop.
    pub id: usize,
    /// The Id of the market our Pop is in
    pub market: usize,
    /// The ID of the firm the pop is working at.
    pub firm: usize,

    /// This is the collated households of the pop group, a the results of adding all
    /// demograpchic data together.
    pub households: Household,
    /// The actual population of the pop. This defines how many actual people are in this
    /// pop. How many mouths there are to feed and satisfy.
    /// 
    /// Eventually, this will likely be broken up into various sub-components, 
    /// Children, Adults, and Elders being the baseline. Species may alter the structure.
    /// 
    /// This uses fractional units to track population growth between turns.
    pub population: f64,
    /// Demographic Breakdown of the pop.
    /// This allows us to consolidate multiple categories of pop into a singular
    /// pop group. Pops have these enforced down into particular limitations,
    /// though this comes at the cost of increased processing and complexity.
    /// If different groups are in the same pop, then we assume they are being paid the same.
    /// If you want a pop to be paid differently, keep them separate.
    /// 
    /// The sum of each DRow should be equal to the size of the pop.
    /// 
    /// Fractional values in the breakdown represent stored up population growth.
    /// Fractions are dropped 
    /// 
    /// This is sorted by household count, largest to smallest.
    pub demo_breakdown: Vec<DRow>,
    /// How many days worth of work a single household in the group does.
    pub efficiency: f64,
    /// The consolidated desires of the pop, formed out of the consolidated desires of the pop.
    pub desires: Vec<Desire>,
    /// What property the pop owns today.
    pub property: HashMap<usize, f64>,
}

impl Pop {
    pub fn new(id: usize, market: usize, firm: usize) -> Self {
        Pop {
            id,
            market,
            firm,
            households: Household::zeroed_household(),
            population: 0.0,
            demo_breakdown: vec![],
            efficiency: 1.0,
            desires: vec![],
            property: HashMap::new(),
        }
    }

    /// # Add Demo
    /// 
    /// Adds a demographic row to the pop. Does not combine households.
    /// 
    /// This does not update desires. Do that separately.
    pub fn add_demo(mut self, demo: DRow) -> Self {
        match self.demo_breakdown.binary_search_by(|x| x.household.count.total_cmp(&demo.household.count)) {
            Ok(pos) | 
            Err(pos) => self.demo_breakdown.insert(pos, demo),
        }
        self
    }

    /// # Include Demo
    /// 
    /// Adds a demographic row to the demographic breakdown in proper ordering.
    pub fn include_demo(&mut self, demo: DRow) {
        match self.demo_breakdown.binary_search_by(|x| x.household.count.total_cmp(&demo.household.count)) {
            Ok(pos) | 
            Err(pos) => self.demo_breakdown.insert(pos, demo),
        }
    }

    /// # Combine Households
    /// 
    /// Combines the households of the pop and combines them into one in the pop.
    /// 
    /// Does not handle Demographic Desires.
    pub fn combine_households(&mut self, data: &Data) {
        self.households = Household::zeroed_household();
        for row in self.demo_breakdown.iter() {
            self.households.combine(&row.household);
        }
    }

    /// # Update Desires
    /// 
    /// Call this on a pop that has it's demographic rows updated and needs
    /// it's desires updated to match.
    pub fn update_desires(&mut self, data: &Data) {
        // Insert all desires into our vector, scaling to the appropriate tags of the 
        // desire. If they are the same desire (with different desire values) combine them.
        let mut desires: Vec<Desire> = vec![];
        for row in self.demo_breakdown.iter() {
            // species
            let species = data.get_species(row.species);
            for desire in species.desires.iter() {
                // copy base over
                let mut new_des = desire.clone();
                // get multiplier
                let mut multiplier = 0.0;
                for tag in desire.tags.iter() {
                    if let DesireTag::HouseholdNeed = tag {
                        debug_assert!(multiplier == 0.0, 
                            "Mulitpliper already set here, either duplicate tag found or another tag is HouseMemberNeed, which shouldn't be next to HouseholdNeed.");
                        multiplier = row.household.population();
                    } else if let DesireTag::HouseMemberNeed(member) = tag {
                        debug_assert!(multiplier == 0.0, 
                            "Mulitpliper already set here, either duplicate tag found or another tag is HouseMemberNeed, which shouldn't be next to HouseholdNeed.");
                        match member {
                            crate::household::HouseholdMember::Adult => multiplier = row.household.total_adults(),
                            crate::household::HouseholdMember::Child => multiplier = row.household.total_children(),
                            crate::household::HouseholdMember::Elder => multiplier = row.household.total_elders(),
                        }
                    }
                }
                // If no other tag sets Multiplier, then set to total population.
                if multiplier == 0.0 {
                    multiplier = row.household.population();
                }
                // multiply the desrie amount by the multiplier.
                new_des.amount = new_des.amount * multiplier;
                // with desire scaled properly, find if it already exists in our desires
                // desires are always sorted.
                
            }
        }
    }
}