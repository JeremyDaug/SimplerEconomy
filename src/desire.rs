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
/// The Priority Function defines how each new step (satisfaction / amount) increases
/// the current priority value. This is a smooth function, so partial satisfaction still
/// creates a useful value.
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

    /// # Current Priority
    /// 
    /// Gets the current prioriyt of our desire based on satisfaction.
    pub fn current_priority(&self) -> f64 {
        let steps = self.satisfied_steps();
        self.priority_fn.priority(self.start_priority, steps)
    }

    /// # Weight
    /// 
    /// Weight gives the value of these.
    /// 
    /// Weight is based on the range (current - start) / steps.
    /// 
    /// (Denser gives higher weight)
    pub fn weight(&self) -> f64 {
        self.satisfied_steps() / (self.current_priority() - self.start_priority)
    }

    /// # End
    /// 
    /// Gets the upper priority bound of our prioirity curve.
    /// 
    /// If it takes steps, but does not end, it returns None.
    /// 
    /// TODO: Test this to ensure correctness.
    pub fn end(&self) -> Option<f64> {
        if let Some(steps) = self.steps {
            let steps = steps.get() as f64;
            Some(self.priority_fn.priority(self.start_priority, steps))
        } else {
            None
        }
    }

    /// # Current Valuation
    /// 
    /// Gets the current value of the desire based on existing satisfaction.
    /// 
    /// Returns the numebr of steps satisfied and the valuation in that order.
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
        let valuation = steps - self.priority_fn.arc_length(self.start_priority, 
            steps + self.start_priority);

        (steps, valuation)
    }

    /// # Expected Value
    /// 
    /// Given the current satisfaction and given amount of additional satisfaction,
    /// get the value of the next interval.
    /// 
    /// If sat_change reduces satisfaction below 0, it caps the lost value at the 
    /// current value.
    /// 
    /// TODO: Test this to ensure correctness.
    pub fn expected_value(&self, sat_change:  f64) -> f64 {
        // if reduces below current satisfaction, just get current valuation.
        if sat_change <= -self.satisfaction {
            return -self.current_valuation().1;
        }
        // cap at maximum steps
        let starting_steps = self.satisfied_steps();
        let steps_adding = if let Some(max_steps) = self.steps {
            (sat_change / self.amount).min(max_steps.get() as f64 - starting_steps)
        } else {
            sat_change / self.amount
        };
        let sign = sat_change.signum();
        // from here, get the arc length of between current and steps added.
        (steps_adding.abs() - self.priority_fn.arc_length(self.satisfied_steps(), 
        steps_adding + starting_steps)) * sign
    }

    /// # Equals
    /// 
    /// A specific check for desires that ensures they are effectively the same.
    /// It checks that everything BUT amount and satisfaction is the same.
    pub fn equals(&self, other: &Desire) -> bool {
        if self.item == other.item &&
        self.start_priority == other.start_priority &&
        self.priority_fn.eq(&other.priority_fn) &&
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
    /// Calculates which step the given priority is on.
    /// 
    /// If outside of the interval, it returns None.
    pub fn on_step(&self, priority: f64) -> Option<f64> {
        if priority < self.start_priority {
            // if before start
            return None;
        } else if let Some(end) = self.end() {
            if end < priority {
                // if after end
                return None;
            }
        }
        Some(self.priority_fn.inverse(self.start_priority, priority))
    }

    /// # Satsfied Up To
    /// 
    /// Given the current satisfaction of a desire, it returns the current prioirity it 
    /// reaches. 
    /// 
    /// Caps at the maximum number of steps (if any).
    pub fn satisfied_steps(&self) -> f64 {
        self.satisfaction / self.amount
    }

    /// # Satisfied to Priority
    /// 
    /// What Priority level the desire has been satisfied to.
    pub fn satisfied_to_priority(&self) -> f64 {
        let step = self.satisfied_steps();
        self.priority_fn.priority(self.start_priority, step)
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
                Some(self.priority_fn.priority(self.start_priority, step))
            }
        } else { // if no end, run calc as normal.
            Some(self.priority_fn.priority(self.start_priority, step))
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
    /// Checks if the desire is satisdief to a particular step or not.
    /// 
    /// If step is not valid, it returns false.
    pub(crate) fn satisfied_to_step(&self, current_value: f64) -> bool {
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
#[derive(Clone, Copy, Debug, PartialEq)]
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
    /// Calculates and returns the current priority value of the desire
    /// 
    /// F(n) + start.
    /// 
    /// n is the step the function is currently on..
    pub fn priority(&self, start: f64, n: f64) -> f64 {
        match self {
            PriorityFn::Linear { slope } => {
                start + slope * n
            },
            PriorityFn::Quadratic { accel} => {
                start + (accel * n).powf(2.0)
            },
            PriorityFn::Exponential { step, growth } => {
                (1.0 + step).powf(growth * n) - 1.0 + start
            },
        }
    }

    /// # Inverse
    /// 
    /// Given a priority, it returns the step it's on.
    pub fn inverse(&self, start: f64, priority: f64) -> f64 {
        match self {
            PriorityFn::Linear { slope } => {
                (priority - start) / slope
            },
            PriorityFn::Quadratic { accel } => {
                (priority - start).sqrt() / accel
            },
            PriorityFn::Exponential { step, growth } => {
                (priority + 1.0 - start).log(1.0 + step) / growth
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
        if start == end { // if start and end are the same, then there's no length, jsut leave.
            return 0.0;
        }
        match self {
            PriorityFn::Linear { slope } => {
                let diffx = end - start;
                let diffy = slope * diffx;
                ((diffx).powf(2.0) + (diffy).powf(2.0)).sqrt()
            },
            PriorityFn::Quadratic { .. } => {
                let diff = end - start; // get distance between start and endof the interval
                let step_size = diff / 8.0; // divide it up
                let mut acc = 0.0; // distance accumulator
                for cl in 0..8 { // step 7 times (8 points)
                    // get our end point steps
                    let lower_step = cl as f64 * step_size;
                    let upper_step = (cl + 1) as f64 * step_size;
                    // get our end point ys.
                    let lowery = self.priority(start, lower_step);
                    let uppery = self.priority(start, upper_step);
                    // add distance to our accumulator
                    acc += (step_size.powf(2.0) + (uppery - lowery).powf(2.0)).sqrt();
                }
                acc
            },
            PriorityFn::Exponential { .. } => {
                let diff = end - start; // get distance between start and endof the interval
                let step_size = diff / 8.0; // divide it up
                let mut acc = 0.0; // distance accumulator
                for cl in 0..8 { // step 7 times (8 points)
                    // get our end point steps
                    let lower_step = cl as f64 * step_size;
                    let upper_step = (cl + 1) as f64 * step_size;
                    // get our end point ys.
                    let lowery = self.priority(start, lower_step);
                    let uppery = self.priority(start, upper_step);
                    // add distance to our accumulator
                    acc += (step_size.powf(2.0) + (uppery - lowery).powf(2.0)).sqrt();
                }
                acc
            },
        }
    }
}