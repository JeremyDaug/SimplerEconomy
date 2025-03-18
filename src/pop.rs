use std::{collections::{BTreeMap, HashMap, VecDeque}, env::current_exe, mem::discriminant};

use itertools::Itertools;

use crate::{data::{self, Data}, desire::{Desire, DesireTag}, drow::DRow, household::Household, market::Market, species::Species};

use ordered_float::OrderedFloat;

use crate::constants::TIME_ID;

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
    pub fn combine_households(&mut self, _data: &Data) {
        self.households = Household::zeroed_household();
        for row in self.demo_breakdown.iter() {
            self.households.combine(&row.household);
        }
    }

    /// # Satisfy Next desire
    /// 
    /// Satisfies the next desire in working_desires.
    /// 
    /// This will reserve wants and goods for the desires.
    /// 
    /// If a desire is not satisfied, it returns that desire and the step 
    /// at which it failed to satisfy.
    pub(crate) fn satisfy_next_desire(&mut self, working_desires: &mut VecDeque<(f64, Desire)>, 
    data: &Data) -> Option<(f64, Desire)> {
        assert!(working_desires.len() > 0, "Working Desires cannot be empty.");
        // Get current step and desire from the front of the working desires. If no next one, leave loop.
        let (current_step, mut current_desire) = 
        if let Some((current_step, current_desire)) = working_desires.pop_front() {
            println!("Current Step: {}", current_step);
            (current_step, current_desire)
        } else {
            return None;
        };
        // prep our shifted record for checking if we succeeded at satisfying the desire.
        let mut shifted = 0.0;
        match current_desire.item {
            crate::item::Item::Want(id) => {
                println!("Getting Wants");
                // want is the most complicated, but follows a standard priority method.
                // First, try to get wants from storage.
                if let Some(want_rec) = self.wants.get_mut(&id) {
                    // get available want
                    let shift = want_rec.available().min(current_desire.amount - shifted);
                    if shift > 0.0 {
                        println!("Have want already, reserving.");
                        want_rec.reserved += shift; // shift
                        current_desire.satisfaction += shift;
                        shifted += shift;
                    }
                }
                // First try to get via ownership
                if shifted < current_desire.amount { // check if we need more.
                    let want = data.get_want(id);
                    // get the goods we can use for this.
                    for good in want.ownership_sources.iter() {
                        // with a good gotten, reserve as much as necessary to satisfy it.
                        if let Some(good_rec) = self.property.get_mut(good) {
                            // Get how many of the good we need to reserve for it.
                            let good_data = data.get_good(*good);
                            let eff = *good_data.own_wants.get(&id)
                                .expect("Want not found in good ownership effects.");
                            let target = (current_desire.amount - shifted) / eff;
                            let shift = target.min(good_rec.available());
                            if shift > 0.0 {
                                println!("Getting Ownership Source.");
                                // shift and reserve
                                shifted += shift * eff;
                                good_rec.reserved += shift;
                                current_desire.satisfaction += shift * eff;
                                // add the extra wants to expected for later uses.
                                for (&want, &eff) in good_data.own_wants.iter() { 
                                    // add the wants to expected.
                                    if let Some(rec) = self.wants.get_mut(&want) {
                                        rec.expected += eff * shift;
                                        if want == id {
                                            rec.reserved += eff * shift;
                                        }
                                    } else {
                                        let mut rec = WantRecord {
                                            owned: 0.0,
                                            reserved: 0.0,
                                            expected: eff * shift,
                                        };
                                        if want == id {
                                            rec.reserved += eff * shift;
                                        }
                                        self.wants.insert(want, rec);
                                    }
                                }
                            }
                        }
                        if shifted > current_desire.amount {
                            break;
                        }
                    }
                }
                // Then try for use if we still need more.
                if shifted < current_desire.amount { // then try for use
                    let want = data.get_want(id);
                    // get the goods we can use for this.
                    for good in want.use_sources.iter() {
                        // with a good gotten, reserve as much as necessary to satisfy it.
                        if self.property.contains_key(good) {
                            // get time and the good
                            let mut good_rec = self.property.remove(good).unwrap();
                            // Get how many of the good we need to reserve for it.
                            let good_data = data.get_good(*good);
                            // get efficiency of producing that want.
                            let eff = *good_data.use_wants.get(&id)
                                .expect("Want not found in good ownership effects.");
                            let mut target = (current_desire.amount - shifted) / eff;
                            // get time target
                            let time_target = good_data.use_time * target;
                            // get our available time, capped at our target.
                            let available_time = time_target
                                .min(self.property.get(&TIME_ID)
                                    .unwrap_or(&PropertyRecord::new(0.0)).available()
                                );
                            if available_time != time_target { // if available time is not enough
                                // reduce target by available time.
                                target = available_time / time_target * target;
                            }
                            // with target gotten and possibly corrected, do the shift
                            let shift = target.min(good_rec.available());
                            if shift > 0.0 {
                                // shift and reserve good and the want
                                shifted += shift * eff;
                                good_rec.reserved += shift;
                                current_desire.satisfaction += shift * eff;
                                // shift time as well
                                self.property.get_mut(&TIME_ID).unwrap()
                                    .reserved += shift * good_data.use_time;
                                // add the extra wants to expected for later uses.
                                for (&want, &eff) in good_data.use_wants.iter() { 
                                    // add the wants to expected.
                                    if let Some(rec) = self.wants.get_mut(&want) {
                                        rec.expected += eff * shift;
                                        if want == id {
                                            rec.reserved += eff * shift;
                                        }
                                    } else {
                                        let mut rec = WantRecord {
                                            owned: 0.0,
                                            reserved: 0.0,
                                            expected: eff * shift,
                                        };
                                        if want == id {
                                            rec.reserved += eff * shift;
                                        }
                                        self.wants.insert(want, rec);
                                    }
                                }
                            }
                            // put good_rec back in regardless of result
                            self.property.insert(*good, good_rec);
                        }
                        if shifted > current_desire.amount {
                            break;
                        }
                    }
                }
                if shifted < current_desire.amount { // lastly consumption
                    let want = data.get_want(id);
                    // get the goods we can consume for this.
                    for good in want.consumption_sources.iter() {
                        // with a good gotten, reserve as much as necessary to satisfy it.
                        if self.property.contains_key(good) {
                            // get time and the good
                            let mut good_rec = self.property.remove(good).unwrap();
                            // Get how many of the good we need to reserve for it.
                            let good_data = data.get_good(*good);
                            // get efficiency of producing that want.
                            let eff = *good_data.consumption_wants.get(&id)
                                .expect("Want not found in good ownership effects.");
                            let mut target = (current_desire.amount - shifted) / eff;
                            // get time target
                            let time_target = good_data.consumption_time * target;
                            // get our available time, capped at our target.
                            let available_time = time_target
                                .min(self.property.get(&TIME_ID)
                                    .unwrap_or(&PropertyRecord::new(0.0)).available()
                                );
                            if available_time != time_target { // if available time is not enough
                                // reduce target by available time.
                                target = available_time / time_target * target;
                            }
                            // with target gotten and possibly corrected, do the shift
                            let shift = target.min(good_rec.available());
                            if shift > 0.0 {
                                // shift and reserve good and the want
                                shifted += shift * eff;
                                good_rec.reserved += shift;
                                current_desire.satisfaction += shift * eff;
                                // shift time as well
                                self.property.get_mut(&TIME_ID).unwrap()
                                    .reserved += shift * good_data.consumption_time;
                                // add the extra wants to expected for later uses.
                                for (&want, &eff) in good_data.consumption_wants.iter() {
                                    // add the wants to expected.
                                    if let Some(rec) = self.wants.get_mut(&want) {
                                        rec.expected += eff * shift;
                                        if want == id {
                                            rec.reserved += eff * shift;
                                        }
                                    } else {
                                        let mut rec = WantRecord {
                                            owned: 0.0,
                                            reserved: 0.0,
                                            expected: eff * shift,
                                        };
                                        if want == id {
                                            rec.reserved += eff * shift;
                                        }
                                        self.wants.insert(want, rec);
                                    }
                                }
                            }
                            // put good_rec back in regardless of result
                            self.property.insert(*good, good_rec);
                        }
                        if shifted > current_desire.amount {
                            break;
                        }
                    }
                }
            },
            crate::item::Item::Class(id) => {
                // get members of the class
                let members = data.get_class(id);
                for member in members.iter().sorted() {
                    // if we have the member, use it.
                    if let Some(rec) = self.property.get_mut(member) {
                        // get how much we can shift over, capping at the target sans already moved goods.
                        let shift = if rec.available() == 0.0 {
                            continue;
                        } else {
                            rec.available().min(current_desire.amount - shifted)
                        };
                        rec.reserved += shift;
                        current_desire.satisfaction += shift;
                        shifted += shift;
                    }
                    if shifted == current_desire.amount {
                        // if shifted enough to cover desire, stop trying.
                        break;
                    }
                }
            },
            crate::item::Item::Good(id) => {
                println!("Satisfying Good: {}.", id);
                // Good, so just find and insert
                if let Some(rec) = self.property.get_mut(&id) {
                    println!("Has in property.");
                    // How much we can shift over.
                    let shift = rec.available().min(current_desire.amount);
                    println!("Shifting: {}", shift);
                    shifted += shift; // add to shifted for later checking
                    rec.reserved += shift; // add to reserved.
                    current_desire.satisfaction += shift; // and to satisfaction.
                    println!("Current Satisfaction: {}", current_desire.satisfaction);
                }
            },
        }
        // If did not succeed at satisfying this time, or desire is fully satisfied, add to finished.
        if shifted < current_desire.amount || current_desire.is_fully_satisfied() {
            println!("Finished with desire. SHifted: {}, desire_target: {}", shifted, current_desire.amount);
            println!("Fully Satisfied: {}", current_desire.is_fully_satisfied());
            return Some((current_step, current_desire));
        } else { // otherwise, put back into our desires to try and satisfy again. Putting to the next spot it woud do
            println!("Repeat Desire.");
            let next_step = current_desire.next_step(current_step)
                .expect("Next Step should exist, but seemingly does not. Investigate why.");
            Self::ordered_desire_insert(working_desires, current_desire, next_step);
            None
        }
    }

    /// # Satisfy until Incomplete
    /// 
    /// Satisfies desires until an desire is unable to satisfy itself.
    /// 
    /// The working desires starts with the next desire this will start with. So no need
    /// to give a starting vaule or desire.
    /// 
    /// Returns the desire that was incomplete and the tier at which it was incomplete.
    pub fn satisfy_until_incomplete(&mut self, working_desires: &mut VecDeque<(f64, Desire)>, 
    data: &Data) -> Option<(f64, Desire)> {
        loop {
            // satisfy the next desire
            if let Some(result) = self.satisfy_next_desire(working_desires, data) {
                // if we get a desire here, escape out. We're done.
                return Some(result);
            }
            // if didn't find anything to stop us, go to the next.
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
        println!("Satisfying Desires.");
        // Working desires, includes the current tier it's on, and the desire.
        let mut working_desires: VecDeque<(f64, Desire)> = VecDeque::new();
        for desire in self.desires.iter() { // initial list is always sorted, so just move over.
            working_desires.push_back((desire.start, desire.clone()));
        }
        // A holding space for desires that have been totally satisfied to simplify
        let mut finished: Vec<Desire> = vec![];
        loop {
            // satisfy the next desire.
            if let Some(result ) = self.satisfy_next_desire(&mut working_desires, data) {
                finished.push(result.1);
            }
            // if no more desires to work on, gtfo.
            if working_desires.len() == 0 {
                break;
            }
        }
        // after doing all satisfactions, put them back in.
        for des in finished {
            println!("Inserting Finished Desires.");
            let (idx, _) = self.desires.iter().find_position(|x| x.equals(&des)).expect("Could not find desire.");
            self.desires.get_mut(idx).unwrap().satisfaction = des.satisfaction;
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
    
    /// Helper function
    /// 
    /// Adds a desire at a given tier to our working desires vecdeque.
    /// 
    /// If multiples of the tier exist, it adds after all existing ones.
    fn ordered_desire_insert(working_desires: &mut VecDeque<(f64, Desire)>, desire: Desire, tier: f64) {
        for idx in 0..working_desires.len() {
            if tier < working_desires.get(idx).unwrap().0 {
                working_desires.insert(idx, (tier, desire));
                return;
            }
        }
        working_desires.push_back((tier, desire));
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
    // We don't use expected goods here as all consumption happens simultaniously.
    /// How many they have used today to satisfy desires.
    pub expended: f64,
    /// How many were given up in trade.
    pub traded: f64,
    /// How many were offered, but not accepted.
    pub offered: f64,
}

impl PropertyRecord {
    pub fn new(owned: f64) -> Self {
        Self {
            owned,
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
    /// How many we are expecting to get during consumption.
    /// 
    pub expected: f64,
}

impl WantRecord {
    pub fn new() -> Self {
        Self {
            owned: 0.0,
            reserved: 0.0,
            expected: 0.0,
        }
    }
    
    /// # Available 
    /// 
    /// How many wants are available for planning purposes.
    /// 
    /// Includes currently owned and expected and removes wants that are 
    /// already reserved.
    /// 
    /// As wants cannot be traded, this should be safe in all cases.
    fn available(&self) -> f64 {
        self.owned - self.reserved + self.expected
    }
}