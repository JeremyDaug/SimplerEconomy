use std::collections::HashSet;

use crate::game::demographic_source::DemographicSource;

/// A set of processes a job can point at.
#[derive(Debug, Clone, PartialEq)]
pub struct Craft {
    /// Craft id. `0` is not a craft.
    pub id: usize,
    /// Display name.
    pub name: String,
    /// Demographic this craft is attached to. `None` is the open craft.
    pub origin: Option<DemographicSource>,
    /// Process ids. [`Self::apply_to`] appends any that are not already present.
    pub processes: Vec<usize>,
    /// Process ids [`Self::apply_to`] drops.
    pub remove: Vec<usize>,
    /// A multiplier to the complexity costs of the processes in the craft.
    ///
    /// The distance of a job from its base craft reduces this bonus' effect.
    pub complexity_modifier: f64,
}

impl Craft {
    /// # New
    ///
    /// A craft with `id` and `name`, no processes, nothing removed, an open
    /// origin, and a complexity modifier of 1.0.
    pub fn new(id: usize, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            origin: None,
            processes: Vec::new(),
            remove: Vec::new(),
            complexity_modifier: 1.0,
        }
    }

    /// # With Origin
    ///
    /// Sets [`Self::origin`].
    pub fn with_origin(mut self, origin: Option<DemographicSource>) -> Self {
        self.origin = origin;
        self
    }

    /// # With Process
    ///
    /// Appends `process` to [`Self::processes`].
    pub fn with_process(mut self, process: usize) -> Self {
        self.processes.push(process);
        self
    }

    /// # With Remove
    ///
    /// Appends `process` to [`Self::remove`].
    pub fn with_remove(mut self, process: usize) -> Self {
        self.remove.push(process);
        self
    }

    /// # With Complexity Modifier
    ///
    /// Sets the complexity modifier for the craft.
    pub fn with_complexity_modifier(mut self, modifier: f64) -> Self {
        self.complexity_modifier = modifier;
        self
    }

    /// # Apply To
    ///
    /// `processes` is the list so far.
    ///
    /// Ids in [`Self::remove`] are dropped. Ids in [`Self::processes`] are
    /// appended when they are not already present. Returns the new list, in
    /// that order.
    pub fn apply_to(&self, processes: &[usize]) -> Vec<usize> {
        let mut next: Vec<usize> = processes
            .iter()
            .copied()
            .filter(|id| !self.remove.contains(id))
            .collect();
        for id in &self.processes {
            if !next.contains(id) {
                next.push(*id);
            }
        }
        next
    }

    /// # Complexity Cost
    ///
    /// The complexity cost of our list of processes against this craft.
    /// Basen on the difference of processes, not the complexity of the processses
    /// themselves.
    ///
    /// `weight` is the full distance of one extra process, passed to
    /// [`Self::craft_distance`]. Distance adds to the modifier until the
    /// cost caps at 1.0.
    pub fn complexity_cost(&self, processes: &Vec<usize>, weight: f64) -> f64 {
        let distance = self.craft_distance(processes, weight);
        (self.complexity_modifier + distance).min(1.0)
    }

    /// # Craft Distance
    ///
    /// Returns the distance of `processes` from this craft's processes.
    ///
    /// `weight` is the full cost of one extra process. A missing baseline
    /// process costs half of `weight`. A process id counts once.
    ///
    /// `(missing * weight / 2) + (additional * weight)`.
    pub fn craft_distance(&self, processes: &Vec<usize>, weight: f64) -> f64 {
        let baseline: HashSet<usize> = self.processes.iter().copied().collect();
        let present: HashSet<usize> = processes.iter().copied().collect();
        let missing = baseline.iter().filter(|id| !present.contains(*id)).count();
        let additional = present.iter().filter(|id| !baseline.contains(*id)).count();
        (missing as f64 * weight * 0.5) + (additional as f64 * weight)
    }
}

#[cfg(test)]
mod craft {
    use super::Craft;
    use crate::game::demographic_source::DemographicSource;

    fn subsistence() -> Craft {
        Craft::new(1, "subsistence")
            .with_process(1)
            .with_process(2)
            .with_complexity_modifier(0.4)
    }

    #[test]
    fn distance_counts_missing_half_and_added_full() {
        let craft = subsistence();

        assert!((craft.craft_distance(&vec![], 0.1) - 0.1).abs() < 1e-12);
        assert!((craft.craft_distance(&vec![1], 0.1) - 0.05).abs() < 1e-12);
        assert!(craft.craft_distance(&vec![1, 2], 0.1).abs() < 1e-12);
        assert!(craft.craft_distance(&vec![1, 1, 2], 0.1).abs() < 1e-12);
        assert!((craft.craft_distance(&vec![1, 2, 3], 0.1) - 0.1).abs() < 1e-12);
        // Missing 2 is half of 0.1. The two 3s are one extra process.
        assert!((craft.craft_distance(&vec![1, 3, 3], 0.1) - 0.15).abs() < 1e-12);
        let repeated = Craft::new(1, "subsistence").with_process(2).with_process(2);
        assert!((repeated.craft_distance(&vec![], 0.1) - 0.05).abs() < 1e-12);
        assert!((craft.craft_distance(&vec![1], 0.2) - 0.1).abs() < 1e-12);
        assert!((craft.craft_distance(&vec![1, 2, 3], 0.2) - 0.2).abs() < 1e-12);
    }

    #[test]
    fn cost_adds_distance_until_it_caps_at_one() {
        let craft = subsistence();

        assert!((craft.complexity_cost(&vec![1, 2], 0.1) - 0.4).abs() < 1e-12);
        assert!((craft.complexity_cost(&vec![1, 2, 3], 0.1) - 0.5).abs() < 1e-12);

        let near = craft.with_complexity_modifier(0.95);
        assert!((near.complexity_cost(&vec![1, 2, 3], 0.1) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn apply_to_drops_removed_ids_then_appends_new_ones() {
        let open = subsistence();
        assert_eq!(open.origin, None);
        assert!(open.remove.is_empty());

        let attached = Craft::new(1, "subsistence")
            .with_origin(Some(DemographicSource::Culture(2)))
            .with_remove(2)
            .with_process(3);
        assert_eq!(attached.apply_to(&[1, 2]), vec![1, 3]);
        assert_eq!(attached.apply_to(&[1, 3]), vec![1, 3]);
        assert_eq!(attached.origin, Some(DemographicSource::Culture(2)));
    }
}
