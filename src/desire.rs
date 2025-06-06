use std::{mem::discriminant, num::{NonZero, NonZeroUsize}};

use crate::{household::HouseholdMember, item::Item};

/// # Desire
/// 
/// A desired good. All pops want self.amount per person. Desires may be repeated.
/// The usize contained in both is the specific item desired.
/// 
/// Starting Priority defines the priority of the good for buying. Lower values is
/// higher priority.
/// 
/// The 'value' of a desire is inversly proportional to the 'length' of it's priority curve.
/// The length when outside of it's range is 1 per level of priority (ie flat).
/// The further it is away from being flat, the lower it's relative value.
/// Lagrangian Path style (L - S) ~~Probably not actually Lagrangian done right. Sue me.~~
/// 
/// The curve between it's start and end steps is defined by PriorityFn.
/// 
/// A step is defined as the satisfaction / amount. each step increases priority
/// as per the PriorityFn formula.
#[derive(Clone, Debug)]
pub struct Desire {
    /// The desired Item.
    pub item: Item,
    /// The amount desired per tier.
    pub amount: f64,
    /// The starting priority of the desire. May be any value.
    pub start_priority: f64,
    /// The function which defines how Priority changes over steps.
    /// 
    /// Can smoothly define prioriyt based on 
    /// satisafction / amount = current priority step.
    pub priority_fn: PriorityFn,
    /// The number of steps that can be taken. Value should be Positive.
    /// 
    /// If None, then it has no cap and continues indefinitely.
    /// 
    /// If Some value, then it has an intervale of [0-steps].
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
    /// the desired item and the starting priority.
    /// 
    /// Steps defaults to 1.
    /// 
    /// # Panics
    /// 
    /// If Start Priority is not a number or if amount is non-positive or not a number.
    pub fn new(item: Item, amount: f64, start_priority: f64, priority_fn: PriorityFn) -> Self {
        assert!(amount > 0.0, "Amount must be a positive value.");
        assert!(!amount.is_nan(), "Amount must be a number.");
        assert!(!start_priority.is_nan(), "Start must be a number.");
        Self {
            item,
            amount,
            start_priority,
            priority_fn,
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
                assert!(self.steps.is_none(), 
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

    /// # With Step Factor
    /// 
    /// Consuming setter for Number of Steps.
    /// 
    /// 
    /// Putting in 0 steps means that it has no end.
    /// 
    /// Factor must be between 0.0 and 1.0 exclusive.
    /// Factor is the multiplier reduction to the value for each step.
    /// 
    /// # Panics
    /// 
    /// Factor must be a finite number between 0.0 and 1.0 exclusive.
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

    /// # Current Valuation
    /// 
    /// Gets the current value of the desire based on existing satisfaction.
    /// 
    /// Returns the numebr of steps satisfied and the total summation in that order.
    /// 
    /// Valuation is based on the arc length of the priority function across the 
    /// satisfied section subtracted from the number of steps.
    /// 
    /// This does not include the pre-priority distance.
    /// 
    /// (self.satisfaction / self.amount) - arc_length
    pub fn current_valuation(&self) -> (f64, f64) {
        let steps = if let Some(end) = self.steps {
            let cap = end.get() as f64;
            (self.satisfaction / self.amount).min(cap)
        } else {
            self.satisfaction / self.amount
        };
        let d_a = self.priority_fn.derivative(0.0);
        let d_b = self.priority_fn.derivative(steps);

        (steps, summation)
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
    /// Caps at the maximum number of steps (if any).
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

/// # Priority Functions
/// 
/// Defines how priority of a desire changes over time.
/// Priority always walks up in ascending order. Don't worry about odd phrasing.
/// 
/// Start is defined be Desire.
/// Step is included in the function as a key parameter
/// Growth in as additional growth factor, multiplying the
/// standard growth to be faster or more extreme.
/// 
/// step and growth ***must*** be positive values.
#[derive(Clone, Copy, Debug)]
pub enum PriorityFn {
    /// Linear Priority Function. 
    /// start + slope * n
    Linear{slope: f64},
    /// Quadratic Priority Function.
    /// start + (accel * n) ^ 2
    Quadratic{accel: f64},
    /// Exponential Priority Function
    /// (1.0 + step) ^ (growth * n) -1 + start
    Exponential{step: f64, growth: f64},
}

impl PriorityFn {
    /// # Linear
    /// 
    /// Safely creates linear PriorityFn.
    pub fn linear(slope: f64) -> Self {
        assert!(slope > 0.0, "Step must be a positive value!");
        Self::Linear{slope}
    }

    /// # Quadratic
    /// 
    /// Safely creates Quadratic PriorityFn.
    pub fn quadratic(step: f64) -> Self {
        assert!(step > 0.0, "Step must be a positive value!");
        Self::Quadratic{accel: step}
    }

    /// # Exponential
    /// 
    /// Safely creates Exponential PriorityFn.
    pub fn exponential(step: f64, growth: f64) -> Self {
        assert!(step > 0.0, "Step must be a positive value!");
        assert!(growth > 0.0, "Growth must be a positive value!");
        Self::Exponential{step, growth}
    }

    /// # Priority
    /// 
    /// Calculates and returns the current priority value of the desire.
    pub fn priority(&self, start: f64, n: f64) -> f64 {
        match self {
            PriorityFn::Linear { slope: step } => {
                start + step * n
            },
            PriorityFn::Quadratic { accel} => {
                start + (accel * n).powf(2.0)
            },
            PriorityFn::Exponential { step, growth } => {
                (1.0 + step).powf(growth * n) - 1.0 + start
            },
        }
    }

    /// # Derivative
    /// 
    /// Calculates the derivative/slope of the function at a particular point.
    pub fn derivative(&self, value: f64) -> f64 {
        match self {
            PriorityFn::Linear { slope } => {
                *slope
            },
            PriorityFn::Quadratic { accel } => {
                2.0 * accel * value
            },
            PriorityFn::Exponential { step, growth } => {
                growth * (1.0 + step).ln() * (1.0 * step).powf(growth * value)
            },
        }
    }

    /// # Arc Length
    /// 
    /// Arc Length Calculator.
    /// 
    /// Takes in the endpoint we are calculating to.
    /// 
    /// This uses a simple 8 step approximation (2 end points plus 6 evenly 
    /// spaced steps between), for quadratic and exponential formulas due to
    /// their bonkers arc length integrals.
    pub fn arc_length(&self, start: f64, end: f64) -> f64 {
        assert!(start < end, "Start must come before end!");
        match self {
            PriorityFn::Linear { slope } => {
                (1.0 + slope.powf(2.0)).sqrt() * end -
                (1.0 + slope.powf(2.0)).sqrt() * start
            },
            PriorityFn::Quadratic { accel } => {
                let diff = end - start;
                let step_size = diff / 8.0;
                todo!()
            },
            PriorityFn::Exponential { step, growth } => {
                todo!()
            },
        }
    }
}