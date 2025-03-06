use std::{collections::{BTreeMap, HashMap, VecDeque}, mem::discriminant};

use itertools::Itertools;

use crate::{data::Data, desire::{Desire, DesireTag}, drow::DRow, household::Household, market::Market, species::Species};

use ordered_float::OrderedFloat;

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
    pub desires: VecDeque<Desire>,
    /// What property the pop owns and how they are using it.
    pub property: HashMap<usize, PropertyRecord>,
    /// What wants the pop currently has in their 
    pub wants: HashMap<usize, WantRecord>,
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
            desires: VecDeque::new(),
            property: HashMap::new(),
            wants: HashMap::new(),
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

    /// # Satisfy Desires
    /// 
    /// Takes the existing property of the pop and sorts it into it's desires.
    /// 
    /// There's no special prioritization, start at the bottom of desires, add to
    /// the first, and go from there. 
    pub fn satisfy_desires(&mut self, data: &Data) {
        // Move current desires into a working btreemap for easier organization and management.
        let mut working_desires: BTreeMap<OrderedFloat<f64>, Desire> = BTreeMap::new();
        for desire in self.desires.iter() {
            let start = OrderedFloat(desire.start);
            working_desires.insert(start, desire.clone());
        }
        // A holding space for desires that have been totally satisfied to simplify
        let mut finished: Vec<Desire> = vec![];
        loop {
            // Get current step and desire from the front of the working desires. If no next one, leave loop.
            let (current_step, mut current_desire) = 
            if let Some((current_step, current_desire)) = working_desires.pop_first() {
                (current_step, current_desire)
            } else {
                break;
            };
            // prep our shifted record for checking if we succeeded at satisfying the desire.
            let mut shifted = 0.0;
            match current_desire.item {
                crate::item::Item::Want(id) => {
                    // want is the most complicated, but follows a standard priority method.
                    // First, try to get wants from storage.
                    let mut shifted = 0.0;
                    if let Some(want_rec) = self.wants.get_mut(&id) {
                        let shift = want_rec.available().min(current_desire.amount - shifted);
                        want_rec.reserved += shift;
                        current_desire.satisfaction += shift;
                        shifted += shift;
                    }
                    if shifted != current_desire.amount {
                        // First try to get via ownership
                        let want = data.get_want(id);
                        // get the goods we can use for this.
                        for good in want.ownership_sources.iter().filter(|x| self.property.contains_key(x)) {
                            // with a good gotten, reserve as much as necessary to satisfy it.
                        }
                    }
                    // then try for use
                    // lastly consumption
                },
                crate::item::Item::Class(id) => {
                    let members = data.get_class(id);
                    let mut shifted = 0.0;
                    for member in members.iter() {
                        if let Some(rec) = self.property.get_mut(member) {
                            // get how much we can shift over, capping at the target sans already moved goods.
                            let shift = rec.available().min(current_desire.amount - shifted);
                            rec.reserved += shift;
                            current_desire.satisfaction += shift;
                            shifted += shift;
                        }
                        if shifted == current_desire.amount {
                            // if shifted in total enough to cover desire, break out of loop.
                            break;
                        }
                    }
                },
                crate::item::Item::Good(id) => {
                    // Good, so just find and insert
                    if let Some(rec) = self.property.get_mut(&id) {
                        // How much we can shift over.
                        let shift = rec.available().min(current_desire.amount);
                        shifted += shift; // add to shifted for later checking
                        rec.reserved += shift; // add to reserved.
                        current_desire.satisfaction += shift; // and to satisfaction.
                    }
                },
            }
            // If did not succeed at satisfying this time, or desire is fully satisfied, add to finished.
            if shifted < current_desire.amount || current_desire.is_fully_satisfied() {
                finished.push(current_desire);
            } else { // otherwise, put back into our desires to try and satisfy again. Putting to the next spot it woud do
                let next_step = current_desire.next_step(current_step.0);
                
            }
        }
    }

    /// # Update Desires Full
    /// 
    /// Call this on a pop that has it's demographic rows updated and needs
    /// it's desires updated to match. 
    /// 
    /// This will totally recalculate the desires of a pop from scratch, so only use 
    /// if the pop is new or there was a major change that a simple update would not cover.
    /// 
    /// NOTE: Didn't bother to test as it's most complex parts have been broken out and tested separately.
    pub fn update_desires_full(&mut self, data: &Data) {
        // Insert all desires into our vector, scaling to the appropriate tags of the 
        // desire. If they are the same desire (with different desire values) combine them.
        let mut desires: Vec<Desire> = vec![];
        for row in self.demo_breakdown.iter() {
            // species
            let species = data.get_species(row.species);
            Self::integrate_desires(&species.desires, row, &mut desires);
            // culture
            if let Some(culture_id) = row.culture {
                let culture = data.get_culture(culture_id);
                Self::integrate_desires(&culture.desires, row, &mut desires);
            }
            // Remaining sections go here.
        }
    }

    /// Helper for getting desires from a part of demographics into our total desires.
    /// 
    /// Takes in the desires of the source demographic part (Species.desires, culture.desires)
    /// Takes in the row for household information.
    /// 
    /// And it takes in, and modifies, the desires we are adding to and will eventually set
    /// self.desires to.
    pub(crate) fn integrate_desires(source_desires: &Vec<Desire>, row: &DRow, desires: &mut Vec<Desire>) {
        for desire in source_desires.iter() {
            println!("---");
            println!("Start: {}", desire.start);
            println!("Good: {}", desire.item.unwrap());
            println!("Amount: {}", desire.amount);
            // copy base over
            let mut new_des = desire.clone();
            // get multiplier
            Self::get_desire_multiplier(desire, row, &mut new_des);
            // with desire scaled properly, find if it already exists in our desires
            // desires are always sorted.
            let mut current = if let Some((est, _)) = desires.iter()
            .find_position(|x| x.start >= new_des.start) {
                // find the first one which is equal to or greater than our new destination.
                est
            } else { desires.len() }; // if none was found then it is either the last or only one.
            println!("First Pos: {}", current);
            // with first match found, try to find duplicates while walking up. 
            loop {
                if current >= desires.len() {
                    // if at or past the end, insert at the end and continue.
                    println!("Insert Position: {}", current);
                    desires.push(new_des);
                    break;
                } else if desires.get(current).unwrap().equals(&new_des) {
                    // if new_desire matches existing desire, add to it.
                    println!("Insert Position: {}", current);
                    desires.get_mut(current).unwrap().amount += new_des.amount;
                    break;
                } else if desires.get(current).unwrap().start > new_des.start {
                    // If the desire we're looking at is greater than our current, insert
                    println!("Insert Position: {}", current);
                    desires.insert(current, new_des);
                    break;
                }
                // If we haven't walked off the end just yet,
                // and we haven't found a match
                // AND we the current is still less than or equal to our new desires start
                // step up 1 and try again.
                println!("Current Start: {}", desires.get(current).unwrap().start);
                current += 1;
            }
        }
    }

    /// Helper for getting multiplier on desires based on tags. This is is used in
    /// multiple places and is likely to change in the future.
    pub(crate) fn get_desire_multiplier(desire: &Desire, row: &DRow, new_des: &mut Desire) {
        // get multiplier
        let mut multiplier = 0.0;
        for tag in desire.tags.iter() {
            if let DesireTag::HouseholdNeed = tag {
                debug_assert!(multiplier == 0.0, 
                    "Mulitpliper already set here, either duplicate tag found or another tag is HouseMemberNeed, which shouldn't be next to HouseholdNeed.");
                multiplier = row.household.count;
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
    }
}

/// Helper for pop property.
#[derive(Debug, Copy, Clone)]
pub struct PropertyRecord {
    /// How many units are owned by the pop right now.
    pub owned: f64,
    /// How many they want to keep at all times. This also covers
    /// reservations to satisfy desires.
    pub reserved: f64,
    /// How many they have used today to satisfy desires.
    pub expended: f64,
    /// How many were given up in trade.
    pub traded: f64,
    /// How many were offered, but not accepted.
    pub offered: f64,
}

impl PropertyRecord {
    pub fn new() -> Self {
        Self {
            owned: 0.0,
            reserved: 0.0,
            expended: 0.0,
            traded: 0.0,
            offered: 0.0,
        }
    }

    /// Available
    /// 
    /// How many goods are available to be used/expended.
    /// This is effectively the difference between owned and reserved.
    pub fn available(&self) -> f64 {
        self.owned - self.reserved
    }
}

/// # Want Record
/// 
/// Records want data for the pop, including how much is available today,
/// reserved wants,
#[derive(Debug, Clone)]
pub struct WantRecord {
    /// How much is currnetly owned.
    pub owned: f64,
    /// How much has been reserved for desires
    pub reserved: f64,
}

impl WantRecord {
    pub fn new() -> Self {
        Self::default()
    }
    
    fn available(&self) -> f64 {
        self.owned - self.reserved
    }
}

impl Default for WantRecord {
    fn default() -> Self {
        Self::new()
    }
}