use std::{mem::discriminant, num::{NonZero, NonZeroUsize}};

use crate::{household::HouseholdMember, item::Item};

/// # Desire
/// 
/// A desired good. All pops want 1 per person. Desires may be repeated.
/// The usize contained in both is the specific item desired.
/// 
/// The start is used to define equvialence between desires of different levels.
/// 
/// 1 / start is the 'desireablity factor' of a good. 
/// 
/// The interval, is the fixed ration between one step of a good and another.
/// This means that for a desire that has a 3/1 ratio between two steps, this 
/// valuation ratio persists between each step.
#[derive(Clone, Debug)]
pub struct Desire {
    /// The desired Item.
    pub item: Item,
    /// The amount desired per tier.
    pub amount: f64,
    /// The starting value. Must be positive.
    pub start_value: f64,
    /// The size of intervals (if any). Interval is always and between 0.0 and 1.0.
    pub reduction_factor: Option<f64>,
    /// The number of steps that can be taken.
    /// 
    /// Value should be Positive.
    /// 
    /// If no interval, then this is not used.
    /// 
    /// If there is an interval and no Steps, then it can go on indefinitely.
    /// 
    /// Cannot be zero, as that should just be a Desire with no interval at all.
    pub steps: Option<NonZeroUsize>,
    /// Tags and effects attached to this desire.
    /// 
    /// Tags also force additional rules on the desire in question.
    /// 
    /// To ensure make similarity checking easier, tags are sorted.
    pub tags: Vec<DesireTag>,
    /// The amount of satisfaciton the desire has currently.
    /// Only used for Pops.
    pub satisfaction: f64,
}

impl Desire {
    /// # New
    /// 
    /// Creates a new basic Desire, allowing you to set
    /// the desired item and the starting point.
    /// 
    /// # Panics
    /// 
    /// If the start Interval is not positive, it panics.
    pub fn new(item: Item, amount: f64, start: f64) -> Self {
        assert!(start > 0.0, "Start must have positive value.");
        assert!(amount > 0.0, "Amount must be a positive value.");
        assert!(!amount.is_nan(), "Amount must be a number.");
        assert!(!start.is_nan(), "Start must be a number.");
        Self {
            item,
            amount,
            start_value: start,
            reduction_factor: None,
            steps: None,
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
                assert!(self.reduction_factor.is_none() || (self.reduction_factor.is_some() && self.steps.is_some()), 
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

    /// # With Interval
    /// 
    /// Consuming setter for interval and steps.
    /// 
    /// Putting in 0 steps means that it has no end.
    /// 
    /// # Panics
    /// 
    /// Panics if interval is not positive.
    pub fn with_interval(mut self, interval: f64, steps: usize) -> Self {
        assert!(0.0 < interval && interval < 1.0, "Interval must be between 0.0 and 1.0, exclusive.");
        assert!(interval.is_finite(), "Interval must be a finite number.");
        if let Some(_) = self.tags.iter().find(|x| discriminant(&DesireTag::LifeNeed(0.0)) == discriminant(x)) {
            assert!(steps > 0, "Desire has the LifeNeed tag. It must have a finite number of steps.");
        }
        self.reduction_factor = Some(interval);
        if steps > 0 { // If given a value, convert to Option<NonZeroUsize>
            self.steps = NonZero::new(steps);
        } else { // if no steps given, just set to None.
            self.steps = None;
        }
        self
    }

    /// # Current Valuation
    /// 
    /// Gets the current value of the desire based on existing satisfaction.
    /// 
    /// Returns the numebr of steps satisfied and the total summation.
    /// 
    /// This is a step valuation function, where each full amount of satisfaction is
    /// linear in adding value, and then it steps to the next value in line.
    pub fn current_valuation(&self) -> (f64, f64) {
        let mut fin_steps = 0.0;
        let mut summation = 0.0;
        if let Some(factor) = self.reduction_factor {
            let normalized_sat = (self.satisfaction / self.amount);
            //println!("Normalized Satisfaction: {}", normalized_sat);
            fin_steps = normalized_sat.floor();
            let steps = fin_steps as i32;
            // whole steps
            for step in 0..steps {
                let val = self.start_value * factor.powi(step) * self.amount;
                //println!("Step Val: {}", val);
                summation += self.start_value * factor.powi(step) * self.amount;
            }
            summation += self.start_value * factor.powi(steps) * self.amount 
                * (normalized_sat - normalized_sat.floor());
            //println!("Summation: {}", summation);
            //println!("Steps: {}", fin_steps);
        } else {
            summation = self.start_value * self.satisfaction;
            fin_steps = (self.satisfaction / self.amount).floor();
            //println!("Summation: {}", summation);
            //println!("Steps: {}", fin_steps);
        }
        (fin_steps, summation)
    }

    /// # End
    /// 
    /// Gets the value of the final value it lands on.
    /// 
    /// If it takes no steps, it returns the start.
    /// 
    /// If it takes steps, but does not end, it returns None.
    /// 
    /// TODO: Test this to ensure correctness.
    pub fn end(&self) -> Option<f64> {
        if let Some(interval) = self.reduction_factor {
            if let Some(steps) = self.steps {
                Some(self.start_value * interval.powf(steps.get() as f64))
            } else { None }
        } else { Some(self.start_value) }
    }

    /// # Next Step
    /// 
    /// Given the current value, get the next valid value this desire steps on.
    /// 
    /// If current value is equal to a step, it gets the next step.
    /// 
    /// If before the start, it returns start.
    /// 
    /// If after the End, it returns None.
    /// 
    /// TODO: Test this to ensure correctness.
    pub fn next_step(&self, current:  f64) -> Option<f64> {
        assert!(current > 0.0, "Current must be a positive value.");
        if current > self.start_value {
            return Some(self.start_value);
        } else if let Some(end) = self.end() {
            if current <= end {
                return None;
            }
        }
        // base formula is Start * Interval ^ Step = Point
        // This solves for step. Log_Interval(Point / Step ) = Step
        let step = (current / self.start_value).log(self.reduction_factor.unwrap());
        // with step, round up
        let mut fin_step = step.ceil();
        if fin_step == step {
            fin_step += 1.0;
        }
        // and recalculate the step.
        Some(self.start_value * self.reduction_factor.unwrap().powf(fin_step))
    }

    /// # Equals
    /// 
    /// A specific check for desires that ensures they are effectively the same.
    /// It checks that everything BUT amount and satisfaction is the same.
    pub fn equals(&self, other: &Desire) -> bool {
        if self.item == other.item &&
        self.start_value == other.start_value &&
        self.reduction_factor == other.reduction_factor &&
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

    /// # On Step
    /// 
    /// Calculates which step a value is on.
    /// 
    /// IE, in the formula start * Interval ^ N, this returns N.
    /// 
    /// If outside of the interval, it returns None.
    /// 
    /// If N is a decimal value, then it's not a whole step we're on.
    pub fn on_step(&self, val: f64) -> Option<f64> {
        assert!(val > 0.0, "Val must be a positive number.");
        if val < self.start_value {
            return None;
        } else if let Some(end) = self.end() {
            if val >= end {
                return None;
            }
        }
        // base formula is Start * Interval ^ Step = Point
        // This solves for step. Log_Interval(Point / Step ) = Step
        Some((val / self.start_value).log(self.reduction_factor.unwrap()))
    }

    /// # Satsfied Up To
    /// 
    /// Given the current satisfaction of a desire, it returns the step it 
    /// reaches. Rounds down. Fractional satisfaciton returns the step it is
    /// currently on.
    /// 
    /// Caps at the maximum nubre of steps (if any).
    pub fn satisfied_up_to(&self) -> f64 {
        (self.satisfaction / self.amount).floor()
    }

    /// # Satisfied to Value
    /// 
    /// What value level the desire has been satisfied to.
    /// 
    /// This is equivalent to finding the steps, flooring it, then
    /// applying the interval that many times.
    pub fn satisfied_to_value(&self) -> f64 {
        let mut step = self.satisfied_up_to();
        if let Some(interval) = self.reduction_factor {
            if let Some(steps) = self.steps {
                step = step.min(steps.get() as f64);
            }
            self.start_value * interval.powf(step)
        } else { self.start_value }
    }
    
    /// # Get Step
    /// 
    /// Gets the value of the step given, if not a valid step it returns None.
    /// 
    /// This can be given fractional steps and will return properly.
    /// 
    /// self.start * self.interval.powf(step)
    pub(crate) fn get_step(&self, step: f64) -> Option<f64> {
        if step < 0.0 { // if negative value, just return None, no step should be negative.
            None
        } else if let Some(interval) = self.reduction_factor { // if it has interval, check if in interval
            if let Some(max_steps) = self.steps { // if we have a max number of steps.
                if (max_steps.get() as f64) < step { // and we're above that max step
                    None
                } else { // if within that number of steps, get step
                    Some(self.start_value * interval.powf(step))
                }
            } else { // If no end step
                Some(self.start_value * interval.powf(step))
            }
        } else if step == 0.0 { // if no interval, only step 0.0 returns value.
            Some(self.start_value)
        } else { // all other cases are always None.
            None
        }
    }
    
    /// # Is Fully Satisfied
    /// 
    /// Checks if a desire has been fully satisfied.
    /// 
    /// IE, the amount of satisfaction is equal to
    /// self.amount * self.steps. 
    pub fn is_fully_satisfied(&self) -> bool {
        if let Some(_) = self.reduction_factor { // if we have an interval
            if let Some(steps) = self.steps { // and are finite
                return ((steps.get() as f64) * self.amount) == self.satisfaction;
            } else { // if not finite, can never be satisfied
                return false;
            }
        }
        // if no interval amount needs to be equal to satisfaction.
        return self.amount == self.satisfaction;
    }
    
    /// # Satisfied At
    /// 
    /// Checks if the current value is fully satisfied or not.
    /// 
    /// If step is not valid, it returns false.
    pub(crate) fn satisfied_at(&self, current_value: f64) -> bool {
        if let Some(step) = self.on_step(current_value) {
            // if we have an equal or greater amount of satisfaction than 
            // the amount * steps, then it's satisfied at that level.
            if self.amount * step <= self.satisfaction {
                true
            } else {
                false
            }
        } else {
            false
        }
    }
}

#[derive(Clone, Debug, PartialEq, PartialOrd)]
pub enum DesireTag {
    /// Desire is necissary for life. Not getting it massively 
    /// increases mortality per tier not met. Value attached is 
    /// the increase to mortality. (1.0 is 100% mortality)
    /// 
    /// # Additional Limitations
    /// 
    /// LifeNeed makes is so the desire cannot be infinite.
    /// 
    /// If it has an interval, it MUST have an end, otherwise the mortality gain is always infinite.
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

