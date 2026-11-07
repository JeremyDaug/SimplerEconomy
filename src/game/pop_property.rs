//! Property rows, the demographic row, and the pop's record placeholder.

use crate::game::household::Household;

/// Demographic breakdown of a pop (one row for now).
#[derive(Debug, Clone, Copy)]
pub struct DemoRow {
    /// Living household block (count, average composition, sex, labor, partnership).
    pub household: Household,
    /// Species ID. `0` is the default human.
    pub species: usize,
    /// Culture ID. `0` means none.
    pub culture: usize,
}

/// # Pop Property Row
/// 
/// Per-good property ledger for a pop.
/// 
/// Covers general property information.
/// Does not include currency savings, which are tracked separately and are removed from here
/// once they are saved.
#[derive(Debug, Clone, Copy, Default)]
pub struct PopPRow {
    // Budgeting values
    /// Total amount currently owned.
    pub quantity: f64,
    /// Units earmarked for today's uses.
    pub reserved: f64,

    // Desire Values
    /// Fresh units inside `quantity`.
    ///
    /// [`crate::game::pop::Pop::decay_goods`] leaves this portion in place.
    pub fresh: f64,
    /// Units lost to rot tonight.
    ///
    /// [`crate::game::pop::Pop::decay_goods`] writes this.
    /// [`crate::game::pop::Pop::reset_day`] clears it.
    pub lost: f64,
    /// Consumed today. Removed from `quantity` when recorded, destroyed at decay.
    pub consumed: f64,

    // Process values
    /// Units a process put out today.
    pub produced: f64,
    /// Units a process destroyed today. Capital set aside is `used`, not this.
    ///
    /// [`crate::game::pop::Pop::reset_day`] clears it.
    pub spent: f64,
    /// Used and not destroyed. Returned to `quantity` at day end, then decayed.
    pub used: f64,
}

/// Day-end records for a pop.
///
/// The field stays on [`crate::game::pop::Pop`]. What it records is not chosen yet.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PopRecords {
    /// The total satisfaction for the pop.
    ///
    /// This value should never be negative, but may be +inf.
    ///
    /// If it is +inf, then the pop has no desires and is always satisfied.
    pub satisfaction: f64,
}

impl DemoRow {
    /// # Total Population
    ///
    /// People in this row, from [`Household::total_count`].
    pub fn total_population(&self) -> f64 {
        self.household.total_count()
    }

    /// # Adult Pop
    ///
    /// Adults in this row, from [`Household::total_adults`].
    pub fn adult_pop(&self) -> f64 {
        self.household.total_adults()
    }

    /// # Elder Pop
    ///
    /// Elders in this row, from [`Household::total_elders`].
    pub fn elder_pop(&self) -> f64 {
        self.household.total_elders()
    }

    /// # Children Pop
    ///
    /// Children in this row, from [`Household::total_children`].
    pub fn children_pop(&self) -> f64 {
        self.household.total_children()
    }

    /// # Labor
    ///
    /// Labor in this row, from [`Household::total_labor`].
    pub fn labor(&self) -> f64 {
        self.household.total_labor()
    }
}

impl PopPRow {
    /// # New
    ///
    /// A row with this quantity and every other ledger at 0.
    pub fn new(quantity: f64) -> Self {
        Self {
            quantity,
            ..Self::default()
        }
    }

    /// # With Reserve
    ///
    /// Sets units earmarked and returns this row.
    pub fn with_reserve(mut self, reserve: f64) -> Self {
        self.reserved = reserve;
        self
    }

    /// # With Fresh
    ///
    /// Sets fresh units and returns this row.
    pub fn with_fresh(mut self, fresh: f64) -> Self {
        self.fresh = fresh;
        self
    }

    /// # With Lost
    ///
    /// Sets units lost and returns this row.
    pub fn with_lost(mut self, lost: f64) -> Self {
        self.lost = lost;
        self
    }

    /// # With Consumed
    ///
    /// Sets units consumed and returns this row.
    pub fn with_consumed(mut self, consumed: f64) -> Self {
        self.consumed = consumed;
        self
    }

    /// # With Produced
    ///
    /// Sets units produced and returns this row.
    pub fn with_produced(mut self, produced: f64) -> Self {
        self.produced = produced;
        self
    }

    /// # With Spent
    ///
    /// Sets units spent and returns this row.
    pub fn with_spent(mut self, spent: f64) -> Self {
        self.spent = spent;
        self
    }

    /// # With Used
    ///
    /// Sets units used and returns this row.
    pub fn with_used(mut self, used: f64) -> Self {
        self.used = used;
        self
    }

    /// # Available
    ///
    /// On-hand units not earmarked.
    pub fn available(&self) -> f64 {
        self.quantity - self.reserved
    }

    /// # Spend Aged First
    ///
    /// Consumes our goods, spending old before fresh.
    ///
    /// Negative units are ignored, and overdrawing stops at 0.0.
    pub fn spend_aged_first(&mut self, units: f64) {
        if units <= 0.0 {
            return;
        }
        let aged = (self.quantity - self.fresh).max(0.0);
        let from_fresh = (units - aged).max(0.0);
        self.fresh = (self.fresh - from_fresh).max(0.0);
    }

    /// # Fresh Share
    ///
    /// Fraction of `quantity` that is fresh.
    ///
    /// `fresh` is clamped into `0..=quantity`, then divided by `quantity`.
    /// Returns `0` when `quantity` is `0`.
    pub fn fresh_share(&self) -> f64 {
        let quantity = self.quantity.max(0.0);
        if quantity == 0.0 {
            return 0.0;
        }
        self.fresh.max(0.0).min(quantity) / quantity
    }
}
