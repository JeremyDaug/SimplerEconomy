use std::collections::HashMap;

use itertools::Itertools;

use crate::{data::Data, desire::Desire, market::Market, drow::DRow};

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

    /// How many households are in it. This is used as the 'base' labor time calculation.
    /// 
    /// You can think of this as the 'base' number of working adults. 
    /// 
    /// This should NEVER be larger than Population.
    pub workers: f64,
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
            workers: 0.0,
            population: 0.0,
            demo_breakdown: vec![],
            efficiency: 1.0,
            desires: vec![],
            property: HashMap::new(),
        }
    }

    /// # Add Demo
    /// 
    /// Adds a demographic row to the pop.
    /// 
    /// Also adds to the size of the pop.
    /// 
    /// This does not update desires. Do that separately.
    pub fn add_demo(mut self, demo: DRow) -> Self {
        self.workers += demo.count;
        self.demo_breakdown.push(demo);
        self
    }

    /// # Update Desires
    /// 
    /// Call this on a pop that has it's demographic rows updated.
    pub fn update_desires(&mut self, data: &Data) {
        // collect like parts into consolidated desires regardless of row.
        // species
        let species = self.demo_breakdown.iter()
            .map(|x| x.species).unique().collect_vec();
        for spec_id in species {
            let species_data = data.get_species(spec_id);
            // get how many of this species.
            let sum = self.demo_breakdown
                .iter().filter(|x| x.species == spec_id)
                .map(|x| x.count)
                .sum::<f64>();
            for desire in species_data.desires.iter() {

            }
        }
        // cultures
    }
}