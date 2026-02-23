use std::{mem::discriminant, num::{NonZero, NonZeroUsize}};

use crate::{demandcurve::DemandCurve, household::HouseholdMember, item::Item};

/// # Desire
/// 
/// A desired good. All pops want self.amount per person. Desires may be repeated.
/// The usize contained in both is the specific item desired.
/// 
/// Starting Priority defines the priority of the good for buying. Lower values is
/// higher priority.
/// 
/// A step is defined as the satisfaction / amount. each step increases priority
/// as per the PriorityFn formula.
/// 
/// The Priority Function defines how each new step (satisfaction / amount) increases
/// the current priority value. This is a smooth function, so partial satisfaction still
/// creates a useful value.
#[derive(Clone, Debug)]
pub struct Desire {
    /// The desired Item.
    pub item: Item,
    /// The amount desired per 'step'.
    pub amount: f64,
    /// The starting priority of the desire. May be **any** value.
    pub starting_value: f64,
    /// The function which defines how Valuation changes over time.
    /// 
    /// Can smoothly define value based on 
    /// satisafction / amount = current priority step.
    pub demand_fn: DemandCurve,
    /// The number of steps that can be taken. Value should be Positive.
    /// 
    /// If None, then it has no cap and continues indefinitely.
    /// 
    /// If any value given, then it gives the number-1 steps of space.
    /// 
    /// Note: This means that if you give it Some(1), then it has 1 step, it's priority 
    /// value at the end will be priority_fn(1).
    /// 
    /// Note: Due to fence posting, the last priority that can be satisfied will be 
    /// step-1, not step. Step is the priority it doesn't go beyond.
    pub steps: Option<NonZeroUsize>,
    /// Tags and effects attached to this desire.
    /// 
    /// Tags also force additional rules on the desire in question.
    /// 
    /// Tags are sorted for easier use and duplicate checking.
    pub tags: Vec<DesireTag>,
    /// The amount of satisfaction the desire has currently.
    /// Only used for Pops.
    /// 
    /// Measured in units of Item supplied, not steps.
    pub satisfaction: f64,
}

impl Desire {
    /// # New
    /// 
    /// Creates a new basic Desire, allowing you to set
    /// the desired item and the starting priority.
    /// 
    /// Steps defaults to 1.
    /// 
    /// # Panics
    /// 
    /// If Start Priority is not a number or if amount is non-positive or not a number.
    pub fn new(item: Item, amount: f64, starting_value: f64, demand_fn: DemandCurve) -> Self {
        assert!(amount > 0.0, "Amount must be a positive value.");
        assert!(!amount.is_nan(), "Amount must be a number.");
        assert!(!starting_value.is_nan(), "Start must be a number.");
        Self {
            item,
            amount,
            starting_value,
            demand_fn,
            steps: NonZero::new(1),
            tags: vec![],
            satisfaction: 0.0,
        }
    }

    /// # With Tag
    /// 
    /// Inserts tag onto desire fluently.
    /// 
    /// # Panics
    /// 
    /// Asserts that LifeNeeds must be finite.
    /// 
    /// Asserts if tags can or can't be next to each other.
    pub fn with_tag(mut self, tag: DesireTag) -> Self {
        // ensure this desire has proper ending if we're addign a life need.
        match tag {
            DesireTag::LifeNeed(_) => {
                println!("steps: {}", self.steps.is_none());
                assert!(self.steps.is_some(), 
                "A Desire with the tag LifeNeed must have a finite number of steps.");
            },
            _ => {}
        }
        // check that tag can be put next to all exsiting tags.
        for t in self.tags.iter() {
            if let Err(msg) = t.safe_with(&tag) {
                assert!(false, "{}", msg);
            }
        }
        // insert into sorted place and check that there are no duplicates.
        match self.tags.binary_search_by(|p| p.partial_cmp(&tag)
            .expect("Tag somehow has a NaN value.")) 
        {
            Ok(_) => { assert!(false, "Tag already exists in Desire.")},
            Err(pos) => {
                self.tags.insert(pos, tag);
            }
        }
        self
    }

    /// # With Steps
    /// 
    /// Consuming setter for Number of Steps.
    /// 
    /// Putting in 0 steps means that it has no end.
    /// 
    /// Steps must be a positive integer value.
    /// 
    /// 1 step is the same as not setting at all.
    /// 
    /// ## Notes
    /// 
    /// Steps cannot be infinite if the desire is also a LifeNeed, will panic if given incorrectly.
    pub fn with_steps(mut self, steps: usize) -> Self {
        if let Some(_) = self.tags.iter().find(|x| discriminant(&DesireTag::LifeNeed(0.0)) == discriminant(x)) {
            assert!(steps > 0, "Desire has the LifeNeed tag. It must have a finite number of steps.");
        }
        if steps > 0 { // If given a value, convert to Option<NonZeroUsize>
            self.steps = NonZero::new(steps);
        } else { // if no steps given, just set to None.
            self.steps = None;
        }
        self
    }

    /// # Satsfied Up To
    /// 
    /// Given the current satisfaction of a desire, it returns the current prioirity it 
    /// reaches. 
    /// 
    /// Caps at the maximum number of steps (if any).
    /// 
    /// NOTE: Not tested as it's so simple.
    pub fn satisfied_steps(&self) -> f64 {
        self.satisfaction / self.amount
    }

    /// # End
    /// 
    /// Gets the upper priority bound of our prioirity curve.
    /// Note, this effectively gets the value of the exact last step it reached.
    /// IE, if a desire has 3 steps, starts at 3.0 and has a slope of -1.0, then
    /// then it has steps 3.0, 2.0, and 1.0, with an end of 0.0.
    /// 
    /// This helps us deal with partial or fractional steps.
    /// 
    /// If it takes steps, but does not end, it returns None.
    pub fn end(&self) -> Option<f64> {
        if let Some(steps) = self.steps {
            let steps = steps.get() as f64;
            Some(self.demand_fn.value(self.starting_value, steps))
        } else {
            None
        }
    }

    /// # On Step
    /// 
    /// Calculates which step the given value is on.
    /// 
    /// If outside of the interval, it returns None.
    pub fn on_step(&self, value: f64) -> Option<f64> {
        if value > self.starting_value {
            // if above starting value, can't be valid.
            return None;
        } else if let Some(end) = self.end() {
            if end < value {
                // if after end
                return None;
            }
        }
        Some(self.demand_fn.inverse(self.starting_value, value))
    }

    /// # Next Value
    /// 
    /// Gets the Next value of our desire based on satisfaction. If satisfied to 10, it 
    /// get's the value of 11. This does not take into account the ammount or partials satisfaction.
    /// insetad just assuming the next whole value is correct. IE, 5.5 returns the value of 6.
    /// 
    /// Assumes only 1 unit, is being satisfied.
    pub fn next_value(&self) -> f64 {
        let steps = self.satisfied_steps();
        self.demand_fn.value(self.starting_value, steps + 1.0)
    }

    /// # Current Value
    /// 
    /// Gets the total sum value of the desire, summing the value
    /// over each step.
    /// 
    /// This multiplies the value produced by the amount, meaning it scales with it's 
    /// 
    pub fn current_value(&self) -> f64 {
        todo!()
    }

    /// # Weight
    /// 
    /// Weight gives the value of these.
    /// 
    /// Weight is based on the range (current - start) / steps.
    /// 
    /// (Denser gives higher weight)
    /// 
    /// NOTE: Does not seem to be in use, not sure why I would use it right now. Will likely delete. Current prioritization does not need weight.
    pub fn weight(&self) -> f64 {
        let result =  self.satisfied_steps() / 
            (self.next_value() - self.starting_value);
        if result.is_nan() {
            0.0
        } else {
            result
        }
    }

    /// # Equals
    /// 
    /// A specific check for desires that ensures they are effectively the same.
    /// It checks that everything BUT amount and satisfaction is the same.
    /// 
    /// Amount and satisfaction can be different due to population size differences.
    pub fn equals(&self, other: &Desire) -> bool {
        if self.item == other.item &&
        self.starting_value == other.starting_value &&
        self.demand_fn.eq(&other.demand_fn) &&
        self.steps == other.steps { // if easy stuff is true, check tags.
            if self.tags.len() == other.tags.len() { // need same number
                for idx in 0..self.tags.len() {
                    // and same order.
                    if self.tags[idx] != other.tags[idx] {
                        return false;
                    }
                }
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    /// # Satisfied to Priority
    /// 
    /// What Priority level the desire has been satisfied to.
    /// 
    /// ## Note
    /// If desire has limited steps it will go to steps+1 as it's highest
    /// end point.
    pub fn satisfied_to_priority(&self) -> f64 {
        let step = self.satisfied_steps();
        self.demand_fn.value(self.starting_value, step)
    }
    
    /// # Get Priority
    /// 
    /// Gets the priority of the step given, if not a valid step it returns None.
    /// 
    /// This can be given fractional steps and will return properly.
    pub(crate) fn get_priority(&self, step: f64) -> Option<f64> {
        if step < 0.0 { // if negative value, return None
            None
        } else if let Some(max_steps) = self.steps { // if it has interval, check if in interval
            let max_steps = max_steps.get() as f64;
            if max_steps < step { // if above final step, return none.
                None
            } else {
                Some(self.demand_fn.value(self.starting_value, step))
            }
        } else { // if no end, run calc as normal.
            Some(self.demand_fn.value(self.starting_value, step))
        }
    }
    
    /// # Is Fully Satisfied
    /// 
    /// Checks if a desire has been fully satisfied.
    /// 
    /// IE, the amount of satisfaction is equal to
    /// self.amount * self.steps. 
    pub fn is_fully_satisfied(&self) -> bool {
        if let Some(step) = self.steps {
            let steps = step.get() as f64;
            steps == (self.satisfaction / self.amount)
        } else { // if no end, cannot be fully satisfied.
            false
        }
    }
    
    /// # Satisfied At
    /// 
    /// Checks if the desire is satisdied to a particular step or not.
    /// 
    /// If step is not valid, it returns false.
    pub(crate) fn satisfied_to_step(&self, step: f64) -> bool {
        if let Some(on_step) = self.on_step(step) {
            // if we have an equal or greater amount of satisfaction than 
            // the amount * steps, then it's satisfied at that level.
            if self.amount * on_step <= self.satisfaction {
                true
            } else {
                false
            }
        } else {
            false
        }
    }
}

/// # Desire Tag
/// 
/// Tags that define additional rules and modifiers to our desires. Adding additional
/// effects to a pop based on the desire being satisfied or not, or
/// modifying how it calculates it's need, such as limiting to a per-hosuehold
/// or per member of a household.
#[derive(Clone, Debug, PartialEq, PartialOrd)]
pub enum DesireTag {
    /// Desire is necissary for life. Not getting it massively 
    /// increases mortality per step not met. Value attached is 
    /// the increase to mortality (Current Mortality + value * missing_steps). 
    /// (1.0 is 100% mortality)
    /// 
    /// ### Additional Limitations
    /// 
    /// LifeNeed makes is so the desire cannot be infinite.
    /// 
    /// If it has an interval, it MUST have an end, otherwise the mortality gain is 
    /// always infinite.
    /// 
    /// It is advised to make sure that the number of steps * our mortality value don't 
    /// go above 1.0, though this could just be a way to create a number of mandatory
    /// steps before the remainder just reduce mortality to normal rates.
    LifeNeed(f64),
    /// Desire is needed on a 'per household' level, meaning that it uses the number of households rather
    /// than the number of people.
    /// 
    /// Exclusive with HouseholdMemberNeed.
    HouseholdNeed,
    /// The desire is needed based on the number of a particular member in the household.
    /// 
    /// This is exclusive with Household Need and itself.
    HouseMemberNeed(HouseholdMember),
}

impl DesireTag {
    /// # Life Need
    /// 
    /// creates a LifeNeed desire tag safely, ensuring it's both functional and not instantly lethal after 1 step.
    pub fn life_need(mortality: f64) -> DesireTag {
        assert!(mortality > 0.0 && mortality <= 1.0, "Mortality must be greater than 0.0, and no greater than 1.0.");
        DesireTag::LifeNeed(mortality)
    }

    /// # Safe With
    /// 
    /// Our enforcement checker to ensure two tags are safe next to each other.
    /// 
    /// Returns Ok() when it's safe. Returns Err(str) when not safe, the str is why it's invalid.
    pub fn safe_with(&self, other: &DesireTag) -> Result<(), &str> {
        // Currently, no tag can be next to itself, even if it's another version.
        if discriminant(self) == discriminant(other) {
            return Err("Same Tags, never safe.");
        }
        match self {
            DesireTag::LifeNeed(_) => {}, // safe next to all others.
            DesireTag::HouseholdNeed => {
                // Cannot be next to any HouseMemberNeed
                if let DesireTag::HouseMemberNeed(_) = other {
                    return Err("Household Need cannot be next to a HouseMemberNeed.");
                }
            },
            DesireTag::HouseMemberNeed(_) => {
                // cannot be next to householdneed
                if let DesireTag::HouseholdNeed = other {
                    return Err("HouseMemberNeed cannot be next to a HouseholdNeed.");
                }
            },
        }
        Ok(())
    }
}