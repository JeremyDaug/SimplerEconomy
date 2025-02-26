use std::default;

/// # Household
/// 
/// This stores the number and makeup of household(s).
/// 
/// The count is how many households of this mold we are grouping together.
/// 
/// The components: household_size, adults, children, and elders; are define what a
/// singular unit of this household looks like.
/// 
/// The Total Size of a household is the maximum number of people in a household, and
/// represents a hypothetical 'maximum' time a household has for 'productive' workers.
/// 
/// Each individual in a household multiplies the base desires of the species, so if a 
/// species needs 1 unit of food to survive each day it needs 1 per member of the household.
/// Desires can also be focused on particular members, like per child or per elder desires.
/// Lastly, some desires are measured on a per-household basis, and so regardless of size, they
/// only need one.
/// 
/// A portion of this household is working, representing the amount of labor available to
/// the household. This labor is then broken up by the wider pop into labor and leisure, 
/// or more accurately, resource gathering and resource using.
/// 
/// A household is also broken up into 3 categories.
/// - Adults, normal, capable of full work, but give no additional bonuses.
/// - Children, 0.3 Labor efficiency, but add +1.0% Birthrate (per 400 days) per child.
/// - Elders, 0.5 Labor Efficiency, but add +1.0% Mortality (per 400 Days) per Elder, and
/// add 1.0 Research Point. May later include skill learning and preservation bonuses.
/// 
/// # Notes
/// 
/// Households are **Not** meant to change after being created, only adding households together
/// should change them. All changes in household should come from HoueholdMod.
/// 
/// ## Default Household
/// 
/// The base household that is used as the 'default' for V1.0 is made of 
/// 
/// 2 adults, 0.5 Elders and 2.5 Children, this gives 3.0 labor (2.0 Adults, 
/// 0.25 from Elders, 0.75 From Children).
/// 
/// This should give about 3.0 Labor at maximum capacity.
/// - Adults give 1.0 each.
/// - Elders give 0.5 each.
/// - Children give 0.3 each.
/// 
/// At this level the net growth of the pop normalizes to 2.0%
/// 
/// This comes from the +1.0% birthrate from each child and the +1.0% mortality
/// for each elder.
/// 
/// ## Household Modifiers
/// 
/// Higher levels of pop categories can alter the makeup of the household, changing 
/// size and ratio.
/// 
/// Modifiers are created by summing the values of households across a pop's demographics.
/// As such, values are not required to be positive.
#[derive(Clone, Copy, Debug)]
pub struct Household {
    /// The number of households grouped together like this.
    pub count: f64,

    /// The size of an individual household summed together.
    /// 
    /// This should equal to adults + elders + children.
    /// 
    /// To get total size of a group of households, multiply this by count.
    pub household_size: f64,
    /// How many adults are in a household.
    pub adults: f64,
    /// How many members of the household are elders.
    pub elders: f64,
    /// How many of the household are children.
    pub children: f64,
}

impl Household {
    /// # Growth
    /// 
    /// The growth expected of the household given current values.
    pub fn growth(&self) -> f64 {
        self.count * self.growth_rate()
    }

    /// # Growth Rate
    /// 
    /// The net growth of the household.
    pub fn growth_rate(&self) -> f64 {
        self.birth_rate() - self.mortality()
    }

    /// # Mortality
    /// 
    /// The mortality Rate of the household.
    pub fn mortality(&self) -> f64 {
        self.elders * 0.1
    }

    /// # Birth Rate
    /// 
    /// Produces the birthrate of the household.
    /// 
    /// 1.0 per child.
    pub fn birth_rate(&self) -> f64 {
        self.children * 0.1
    }

    /// # Labor 
    /// 
    /// The amount of labor this household produces each day.
    /// 
    /// # Defaults
    /// 
    /// Adults give 1.0.
    /// Children give 0.3.
    /// Elders give 0.5.
    pub fn labor(&self) -> f64 {
        self.count * (self.adults + self.children * 0.3 + self.elders * 0.5)
    }

    /// # Combine Households
    /// 
    /// Takes multiple households and combines them into one. Counts are added
    /// all other parts are averaged. This should result in a clean final household
    /// for more general use.
    /// 
    /// # Note
    /// 
    /// Households of size 0 do not get added due to the weighted averages used.
    pub fn combine_households(households: &Vec<Household>) -> Self {
        let final_count = households.iter().map(|x| x.count).sum::<f64>();
        let mut housesizeacc = 0.0;
        let mut adultacc = 0.0;
        let mut childacc = 0.0;
        let mut elderacc = 0.0;
        for h in households.iter() {
            housesizeacc += h.household_size * h.count;
            adultacc += h.adults * h.count;
            childacc += h.children * h.count;
            elderacc += h.elders * h.count;
        }
        Self {
            count: final_count,
            household_size: housesizeacc / final_count,
            adults: adultacc / final_count,
            elders: elderacc / final_count,
            children: childacc / final_count,
        }
    }

    /// # Combine
    /// 
    /// Takes two households and combines them into a new household.
    /// 
    /// This means that their counts are added while household size, adults, 
    /// and elders are averaged.
    /// 
    /// # Note
    /// 
    /// If adding a household with a count of 0, it results in no change.
    pub fn combine(&self, other: &Self) -> Self {
        let sum = self.count + other.count;
        let household_size = (self.household_size * self.count 
            + other.household_size * other.count)
            / sum;
        let adults = (self.adults * self.count + other.adults * other.count)
            / sum;
        let children = (self.children * self.count + other.children * other.count)
            / sum;
        let elders = (self.elders * self.count + other.elders * other.count)
            / sum;
        Self {
            count: sum, // counts added
            household_size, // weighted averages for the rest.
            adults,
            elders,
            children,
        }
    }

    /// # New
    /// 
    /// Creates new households. Sets both ratios, size, and number of household.
    /// 
    /// Be sure that the final size is as you expect.
    pub fn new(households: f64, adults: f64, children: f64, elders: f64) -> Self {
        Self::new_household(adults, children, elders).mult(households)
    }

    /// # Add Count
    /// 
    /// Adds to the count of households. Does not check if household count is positive.
    pub fn add_count(&self, count: f64) -> Self {
        Self {
            count: self.count + count,
            household_size: self.household_size,
            adults: self.adults,
            elders: self.elders,
            children: self.children,
        }
    }

    /// # Multiplier
    /// 
    /// Muitplies the the count of a household.
    pub fn mult(&self, scalar: f64) -> Self {
        Self {
            count: self.count * scalar,
            household_size: self.household_size,
            adults: self.adults,
            elders: self.elders,
            children: self.children,
        }
    }

    /// # Zeroed Household
    /// 
    /// Gets a household with all zero values.
    /// Useful for making 'modifier' households.
    pub fn zeroed_household() -> Self {
        Self {
            count: 0.0,
            household_size: 0.0,
            adults: 0.0,
            elders: 0.0,
            children: 0.0,
        }
    }

    /// # Modify Household
    /// 
    /// Adds the household modifier to the household. If applied to a household
    /// with a count it will change the size of the population.
    /// 
    /// # Notes
    /// 
    /// This ensures that household size and adults bottom out at 1.0, while 
    /// elders and children bottom out at 0.0.
    pub fn modify_household(&self, house_mod: HouseholdMod) -> Self {
        Self {
            count: self.count,
            household_size: (self.household_size + house_mod.net_change()).max(1.0),
            adults: (self.adults + house_mod.adults).max(1.0),
            elders: (self.elders + house_mod.elders).max(0.0),
            children: (self.children + house_mod.children).max(0.0),
        }
    }

    /// # Add Mods
    /// 
    /// Adds multiple modifiers to the household.
    /// 
    /// # Notes
    /// 
    /// This adds and restricts only after all household modifiers are applied.
    pub fn add_mods(&self, modifiers: Vec<HouseholdMod>) -> Self {
        let mut final_mod = HouseholdMod::zero();
        for modifier in modifiers.iter() {
            final_mod = final_mod.add_modifiers(*modifier);
        }
        Household::zeroed_household().modify_household(final_mod)
    }
}

/// # Household Member
/// 
/// Used in various places to denote and reference the subcomponents
/// of a household.
/// 
/// Currently only covers our built in members. If members are made generic and
/// moddable, then this may need to be just removed outright.
#[derive(Clone, Copy, Debug)]
pub enum HouseholdMember {
    Adult,
    Child,
    Elder,
}

/// # Household Modifier
/// 
/// Household Modifier is used to record changes in a household.
/// 
/// This is to simplify and streamline some of the logic of consolidation and
/// modifying households in a pop's Demographic Factors.
/// 
/// There is no protections or limitations on what is allowed. Households 
/// cap to ensure a household isn't empty, this doesn't.
#[derive(Debug, Copy, Clone)]
pub struct HouseholdMod {
    pub adults: f64,
    pub elders: f64,
    pub children: f64
}

impl HouseholdMod {
    /// # Add Modifiers
    /// 
    /// Adds two modifiers together to produce a combined modifiers.
    pub fn add_modifiers(&self, other: Self) -> Self {
        let mut result = self.clone();
        result.adults += other.adults;
        result.elders += other.elders;
        result.children += other.children;
        result
    }

    /// # Net Change
    /// 
    /// The net changes in the size of a household.
    pub fn net_change(&self) -> f64 {
        self.adults + self.elders + self.children
    }

    /// # Default Household
    /// 
    /// Gives a household mod that is the same as our default:
    /// 
    /// 2 adults, 0.5 elders, 2.5 children.
    pub fn default_household() -> Self {
        Self {
            adults: 2.0,
            elders: 0.5,
            children: 2.5,
        }
    }

    pub fn zero() -> Self {
        Self {
            adults: 0.0,
            elders: 0.0,
            children: 0.0,
        }
    }
}