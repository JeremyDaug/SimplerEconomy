/// A baseline set of processes a job can point at.
#[derive(Debug, Clone, PartialEq)]
pub struct Craft {
    /// Craft id. `0` is not a craft.
    pub id: usize,
    /// Display name.
    pub name: String,
    /// Process ids in the baseline, in run order.
    pub processes: Vec<usize>,
    /// A multiplier to the complexity costs of the processes in the craft.
    ///
    /// The distance of a job from its base craft reduces this bonus' effect.
    pub complexity_modifier: f64,
}

/// A culture's or religion's change to one base craft.
#[derive(Debug, Clone, PartialEq)]
pub struct CulturalCraft {
    /// Base craft id this overlay belongs to.
    pub craft: usize,
    /// Process ids appended when they are not already in the list.
    pub add: Vec<usize>,
    /// Process ids dropped from the list this overlay is applied to.
    pub remove: Vec<usize>,
    /// Multiplied into the base craft's complexity modifier when this overlay applies.
    ///
    /// `1.0` leaves the base modifier unchanged.
    pub complexity_modifier: f64,
}

impl Craft {
    /// # New
    ///
    /// A craft with `id` and `name`, no processes, and a complexity modifier of 1.0.
    pub fn new(id: usize, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            processes: Vec::new(),
            complexity_modifier: 1.0,
        }
    }

    /// # With Process
    ///
    /// Appends `process` to the baseline.
    pub fn with_process(mut self, process: usize) -> Self {
        self.processes.push(process);
        self
    }

    /// # With Complexity Modifier
    ///
    /// Sets the complexity modifier for the craft.
    pub fn with_complexity_modifier(mut self, modifier: f64) -> Self {
        self.complexity_modifier = modifier;
        self
    }

    /// # With Overlay
    ///
    /// Applies one culture or religion change to this craft.
    ///
    /// `overlay` supplies the process changes and a modifier. Process ids
    /// follow [`CulturalCraft::apply`]. The modifier is multiplied into
    /// [`Self::complexity_modifier`]. Id and name stay. Returns this craft.
    pub fn with_overlay(mut self, overlay: &CulturalCraft) -> Self {
        self.processes = overlay.apply(&self.processes);
        self.complexity_modifier *= overlay.complexity_modifier;
        self
    }

    /// # Complexity Cost
    ///
    /// The complexity cost of `processes` against this craft.
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
        let mut missing = 0;
        for id in &self.processes {
            if !processes.contains(id) {
                missing += 1;
            }
        }
        let mut additional = 0;
        for id in processes {
            if !self.processes.contains(id) {
                additional += 1;
            }
        }
        (missing as f64 * weight * 0.5) + (additional as f64 * weight)
    }
}

impl CulturalCraft {
    /// # New
    ///
    /// An overlay for base craft `craft`, with nothing added or removed
    /// and a complexity modifier of 1.0.
    pub fn new(craft: usize) -> Self {
        Self {
            craft,
            add: Vec::new(),
            remove: Vec::new(),
            complexity_modifier: 1.0,
        }
    }

    /// # With Add
    ///
    /// Appends `process` to the processes this overlay adds.
    pub fn with_add(mut self, process: usize) -> Self {
        self.add.push(process);
        self
    }

    /// # With Remove
    ///
    /// Appends `process` to the processes this overlay drops.
    pub fn with_remove(mut self, process: usize) -> Self {
        self.remove.push(process);
        self
    }

    /// # With Complexity Modifier
    ///
    /// Sets the complexity modifier for this overlay.
    pub fn with_complexity_modifier(mut self, modifier: f64) -> Self {
        self.complexity_modifier = modifier;
        self
    }

    /// # Apply
    ///
    /// `processes` is the list so far.
    ///
    /// Ids in [`Self::remove`] are dropped. Ids in [`Self::add`] are appended
    /// when they are not already present. Returns the new list, in that order.
    pub fn apply(&self, processes: &[usize]) -> Vec<usize> {
        let mut next: Vec<usize> = processes
            .iter()
            .copied()
            .filter(|id| !self.remove.contains(id))
            .collect();
        for id in &self.add {
            if !next.contains(id) {
                next.push(*id);
            }
        }
        next
    }
}

#[cfg(test)]
mod craft {
    use super::{Craft, CulturalCraft};

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
    fn overlay_rewrites_processes_and_multiplies_its_modifier() {
        let plain = CulturalCraft::new(1);
        assert!((plain.complexity_modifier - 1.0).abs() < 1e-12);
        let kept = subsistence().with_overlay(&plain);
        assert!((kept.complexity_modifier - 0.4).abs() < 1e-12);

        let overlay = CulturalCraft::new(1)
            .with_remove(2)
            .with_add(3)
            .with_complexity_modifier(0.5);
        let craft = subsistence().with_overlay(&overlay);

        assert_eq!(craft.id, 1);
        assert_eq!(craft.name, "subsistence");
        assert_eq!(craft.processes, vec![1, 3]);
        assert!((craft.complexity_modifier - 0.2).abs() < 1e-12);
    }
}
