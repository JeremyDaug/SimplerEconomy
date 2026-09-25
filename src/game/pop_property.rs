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
    /// Class ID. `0` means none. Class is not a type yet.
    pub class: usize,
    /// Religion ID. `0` means none.
    pub religion: usize,
}

impl DemoRow {
    /// Household size × count.
    pub fn total_population(&self) -> f64 {
        self.household.total_count()
    }

    pub fn adult_pop(&self) -> f64 {
        self.household.total_adults()
    }

    pub fn elder_pop(&self) -> f64 {
        self.household.total_elders()
    }

    pub fn children_pop(&self) -> f64 {
        self.household.total_children()
    }

    pub fn labor(&self) -> f64 {
        self.household.total_labor()
    }
}

/// Per-good property ledger for a pop.
#[derive(Debug, Clone, Copy, Default)]
pub struct PopPRow {
    /// Total amount currently owned.
    pub quantity: f64,
    /// Units earmarked for today's uses. `quantity - reserved` is free stock.
    pub reserved: f64,
    /// Units output by a process today. Not decayed today.
    pub process_output: f64,
    /// Consumed today. Removed from `quantity` when recorded, destroyed at decay.
    pub consumed: f64,
    /// Used and not destroyed. Returned to `quantity` at day end, then decayed.
    pub used: f64,
}

impl PopPRow {
    pub fn new(quantity: f64) -> Self {
        Self {
            quantity,
            ..Self::default()
        }
    }

    pub fn with_reserve(mut self, reserve: f64) -> Self {
        self.reserved = reserve;
        self
    }

    pub fn with_consumed(mut self, consumed: f64) -> Self {
        self.consumed = consumed;
        self
    }

    pub fn with_used(mut self, used: f64) -> Self {
        self.used = used;
        self
    }

    /// On-hand units not earmarked.
    pub fn available(&self) -> f64 {
        self.quantity - self.reserved
    }
}

/// Day-end records for a pop.
///
/// The field stays on [`crate::game::pop::Pop`]. What it records is not chosen yet.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PopRecords {}
