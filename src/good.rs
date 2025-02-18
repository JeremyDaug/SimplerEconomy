use std::collections::HashMap;

/// # Good
/// 
/// A good is something that is desireable or useful.
/// 
/// ## Mass Conservation Rules
/// 
/// A soft rule to preserve conservation of mass would be the following.
/// 
/// 1. The starting good and decay result should have equivalent masses.
/// 2. Classes should either universally enforce mass equivalence between goods in a 
///     class, or be able to record that a class enforces equivalence or not. If it
///     does enforce it, then that class of goods can be destroyed by a process 
///     without creating or destroying mass safely. If it doesn't enforce equivalent 
///     mass in the class (hah) then it should not be destroyed by a process, as that
///     would create or destroy total mass by the process.
#[derive(Clone, Debug)]
pub struct Good {
    /// Unique id of the good.
    pub id: usize,
    /// The name of the good. (defaults to G(id))
    pub name: String,
    /// The Variant Name of the good (if it is a variant.)
    pub variant_name: String,

    /// Wants produced by Consumption.
    pub consumption_wants: HashMap<usize, f64>,
    /// How long it takes to consume.
    pub consumption_time: f64,
    /// Wants produced by use.
    pub use_wants: HashMap<usize, f64>,
    /// How long it takes to use.
    pub use_time: f64,
    /// Wants produced by use.
    pub own_wants: HashMap<usize, f64>,

    /// What class of goods this belongs to. 
    /// 
    /// If it points to itself, then it is the example of the class.
    pub class: Option<usize>,

    /// The durability of the good.
    /// 
    /// This is the probability (0.0 to 1.0 inclusive) that the good decays.
    /// 
    /// 1.0 is always decays, 0.0 does not decay.
    /// 
    /// It always decays by this amount, even if it results in partial goods.
    pub decay_rate: f64,
    /// What good (if any) it decays to and at what multiplier (should conserve mass).
    /// 
    /// This is also what it is consumed into.
    pub decays_to: Option<(usize, f64)>,
    /// How big it is in "Human carriable" units. 1 Bulk = 1 person can carry it 
    /// and nothing else. Think of this as 1th of a cubic meter or something like that.
    pub bulk: f64,
    /// How heavy the good is in kg, used for transportation. Humans can carry 25 kg 
    /// with just their arms and no aids.
    pub mass: f64,
    /// Tags to give extra properties for goods. Typically defines how they interact
    /// with the wider market.
    pub tags: Vec<GoodTags>,
}

impl Good {
    /// # New
    /// 
    /// Creates a new good with the id, name and variant name it's given.
    /// 
    /// All other values are either empty or set to 0.0 
    pub fn new(id: usize, name: String, variant_name: String) -> Self {
        Good { 
            id, 
            name, 
            variant_name, 
            consumption_wants: HashMap::new(), 
            consumption_time: 0.0, 
            use_wants: HashMap::new(), 
            use_time: 0.0, 
            own_wants: HashMap::new(), 
            class: None, 
            decay_rate: 0.0, 
            decays_to: None, 
            bulk: 0.0, 
            mass: 0.0, 
            tags: vec![]
        }
    }

    /// # In class
    /// 
    /// Consuming setter for class.
    pub fn in_class(mut self, class: usize) -> Self {
        self.class = Some(class);
        self
    }

    /// # With Decay
    /// 
    /// Sets the decay values.
    /// 
    /// Consumes original.
    /// 
    /// 1.0 Is alwasy decays, 0.0 is never decays.
    /// 
    /// # Panics
    /// 
    /// If Decay Rate is not between 0.0 and 1.0 inclusive.
    pub fn with_decay_rate(mut self, decay_rate: f64, ) -> Self {
        assert!(0.0 <= decay_rate && decay_rate <= 1.0, "decay_rate must be between 0.0 and 1.0 inclusive.");
        self.decay_rate = decay_rate;
        self
    }

    /// # Decays To
    /// 
    /// Sets what it decays to and at what efficiency.
    /// 
    /// 1.0 in = decay_eff out
    /// 
    /// # Warning
    /// 
    /// This does not check that 'decays_to' is a real good, and may result in error.
    /// 
    /// # Panics
    /// 
    /// decay_eff must be a positive value.
    pub fn decays_to(mut self, decays_to: usize, decay_eff: f64) -> Self {
        assert!(decay_eff > 0.0, "Decay Efficiency must be a Positive value.");
        self.decays_to = Some((decays_to, decay_eff));
        self
    }

    /// # With Consumption
    /// 
    /// Sets consumption values. Consumes original.
    /// 
    /// # Panics
    /// 
    /// Time must be non-negative.
    pub fn with_consumption(mut self, time: f64, wants: HashMap<usize, f64>) -> Self {
        assert!(time >= 0.0,"time cannot be negative!");

        self.consumption_time = time;
        self.consumption_wants = wants;
        self
    }

    /// # With Use
    /// 
    /// Sets Use values. Consumes original.
    /// 
    /// # Panics
    /// 
    /// Time must be non-negative.
    pub fn with_uses(mut self, time: f64, wants: HashMap<usize, f64>) -> Self {
        assert!(time >= 0.0, "time cannot be negative!");

        self.use_time = time;
        self.use_wants = wants;
        self
    }

    /// # With Ownership
    /// 
    /// Sets Ownership value. Consumes original.
    pub fn with_ownership(mut self, wants: HashMap<usize, f64>) -> Self {
        self.own_wants = wants;
        
        self
    }

    /// # With Bulk
    /// 
    /// Sets the bulk, consumes the original.
    /// 
    /// # Panics
    /// 
    /// Bulk must be Non-Negative.
    pub fn with_bulk(mut self, bulk: f64) -> Self {
        assert!(bulk >= 0.0, "Bulk cannot be negative!");
        self.bulk = bulk;
        self
    }

    /// # With Mass
    /// 
    /// Sets the mass and consumes the original.
    /// 
    /// # Panics
    /// 
    /// Mass must be non-negative.
    pub fn with_mass(mut self, mass: f64) -> Self {
        assert!(mass >= 0.0, "Mass cannot be negative!");
        self.mass = mass;
        self
    }

    /// # With Tags
    /// 
    /// Adds tags to the good. Consumes original.
    pub fn with_tags(mut self, tags: Vec<GoodTags>) -> Self {
        self.tags.extend(tags);
        self
    }
}

impl PartialEq for Good {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[derive(Debug, Clone)]
pub enum GoodTags {
    /// Good cannot be moved between markets. They also typically have no mass or 
    /// bulk either.
    Immobile,
    /// Good cannot be bought or sold between pops. They also typically have 
    /// no mass or bulk either.
    Nonexchangeable,
    /// Good is always consumed at the end of the day. Decays into nothing.
    Service,
    /// Good can be used for storage.
    Storage { bulk: f64, mass: f64 }
}