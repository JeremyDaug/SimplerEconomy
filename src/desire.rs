use std::num::{NonZero, NonZeroUsize};

use crate::item::Item;

/// # Desire
/// 
/// A desired good. All pops want 1 per household. Desires may be repeated.
/// The usize contained in both is the specific item desired.
/// 
/// The start is used to define equvialence between desires of different levels.
/// 
/// 1 / start is the 'desireablity factor' of a good.
#[derive(Clone, Copy, Debug)]
pub struct Desire {
    /// The desired Item.
    pub item: Item,
    /// The starting value. Must be positive.
    pub start: f64,
    /// The size of intervals (if any). Interval is always Positive
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
    // TODO: Add this later.
    //pub tags: Vec<DesireTag>,
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
    pub fn new(item: Item, start: f64) -> Self {
        assert!(start > 0.0, "Start must have positive value.");
        Self {
            item,
            start,
            interval: None,
            steps: None,
        }
    }

    /// # With Interval
    /// 
    /// Consuming setter for interval and steps.
    /// 
    /// # Panics
    /// 
    /// Panics if interval is not positive.
    pub fn with_interval(mut self, interval: f64, steps: Option<usize>) -> Self {
        assert!(interval > 0.0, "Interval must be a positive value.");
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
    /// let d = Desire::new(Item::Want(0), 1.0);
    /// assert_eq!(d.end(), Some(1.0));
    /// let d = Desire::new(Item::Want(0), 1.0)
    ///     .with_interval(1.0, Some(10));
    /// assert_eq!(d.end(), Some(11.0));
    /// let d = Desire::new(Item::Want(0), 1.0)
    ///     .with_interval(1.0, None);
    /// assert_eq!(d.end(), None);
    /// ```
    pub fn end(self) -> Option<f64> {
        if let Some(interval) = self.interval {
            if let Some(steps) = self.steps {
                Some(self.start + steps.get() as f64 * interval)
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
    pub fn next_step(self, current:  f64) -> Option<f64> {
        if current < self.start {
            return Some(self.start);
        } else if let Some(end) = self.end() {
            if current >= end {
                return None;
            }
        }
        // Get the interval.
        let interval = self.interval.unwrap();
        // remove the start.
        let diff = current - self.start;
        // then see where you end up step wise, and round up.
        let steps = (diff / interval).ceil();
        // then add the interval and steps back to start for the destination.
        return Some(steps * interval + self.start);
    }
}