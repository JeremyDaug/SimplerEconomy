use std::{mem::discriminant, num::{NonZero, NonZeroUsize}};

use crate::{household::HouseholdMember, item::Item};

/// # Desire
/// 
/// A desired good. All pops want 1 per household. Desires may be repeated.
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
    pub start: f64,
    /// The size of intervals (if any). Interval is always Positive.
    pub interval: Option<f64>,
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
            start,
            interval: None,
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

    /// # With Interval
    /// 
    /// Consuming setter for interval and steps.
    /// 
    /// # Panics
    /// 
    /// Panics if interval is not positive.
    pub fn with_interval(mut self, interval: f64, steps: Option<usize>) -> Self {
        assert!(interval > 1.0, "Interval must Greater than 1.0.");
        assert!(!interval.is_nan(), "Interval must be a number.");
        if let Some(_) = self.tags.iter().find(|x| discriminant(&DesireTag::LifeNeed(0.0)) == discriminant(x)) {
            assert!(steps.is_some(), "Desire has the LifeNeed tag. It must have a finite number of steps.");
        }
        self.interval = Some(interval);
        if let Some(steps) = steps { // If given a value, convert to Option<NonZeroUsize>
            self.steps = NonZero::new(steps);
        } else { // if no steps given, just set to None.
            self.steps = None;
        }
        self
    }

    /// # End
    /// 
    /// Gets the value of the final step it lands on. 
    /// 
    /// If it takes no steps, it returns the start.
    /// 
    /// If it takes steps, but does not end, it returns None.
    /// 
    /// TODO: Test this to ensure correctness.
    /// 
    /// ```
    /// use simpler_economy::item::Item;
    /// use simpler_economy::desire::Desire;
    /// let d = Desire::new(Item::Want(0), 1.0, 1.0);
    /// assert_eq!(d.end(), Some(1.0));
    /// let d = Desire::new(Item::Want(0), 1.0, 1.0)
    ///     .with_interval(2.0, Some(2));
    /// assert_eq!(d.end(), Some(4.0));
    /// let d = Desire::new(Item::Want(0), 1.0, 1.0)
    ///     .with_interval(2.0, None);
    /// assert_eq!(d.end(), None);
    /// ```
    pub fn end(&self) -> Option<f64> {
        if let Some(interval) = self.interval {
            if let Some(steps) = self.steps {
                Some(self.start * interval.powf(steps.get() as f64))
            } else { None }
        } else { Some(self.start) }
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
        if current < self.start {
            return Some(self.start);
        } else if let Some(end) = self.end() {
            if current >= end {
                return None;
            }
        }
        // base formula is Start * Interval ^ Step = Point
        // This solves for step. Log_Interval(Point / Step ) = Step
        let step = (current / self.start).log(self.interval.unwrap());
        // with step, round up
        let fin_step = step.ceil();
        // and recalculate the step.
        Some(self.start * self.interval.unwrap().powf(fin_step))
    }

    /// # Equals
    /// 
    /// A specific check for desires that ensures they are effectively the same.
    /// It checks that everything BUT amount is the same.
    pub fn equals(&self, other: &Desire) -> bool {
        if self.item == other.item &&
        self.start == other.start &&
        self.interval == other.interval &&
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

