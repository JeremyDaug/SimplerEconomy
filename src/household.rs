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
/// ## Household Changes
/// 
/// Households are 'created' at the species level and represents the 'natural' form
/// the species tends to take without any additional aid or needs.
/// 
/// Eventually, we may allow for greater variety and uniqueness in Households, but for 
/// now, this is fixed in place with pre-defined roles.
/// 
/// ## Modifiers
/// 
/// Higher levels of pop categories can alter the makeup of the household, adding 
/// or removing elders and children.
/// 
/// Additionally, some wants/goods have passive effects on a household if kept long enough.
/// Relativeyl high amounts of healthcare increase Elders for example.
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

    /// # Base Household
    /// 
    /// Returns the base house information of the household with count 0.0.
    pub fn base_household(&self) -> Self {
        self.mult(0.0)
    }

    /// # New
    /// 
    /// Creates new households. 
    /// 
    /// Household is the size, and adults, children, and elders are the ratios.
    /// 
    /// Be sure that the final size is as you expect.
    pub fn new(households: f64, adults: f64, children: f64, elders: f64) -> Self {
        Self::new_household(adults, children, elders).mult(households)
    }

    /// # Default Households
    /// 
    /// Creates household with the given number of households
    /// 
    /// Assumes the default makeup of household (2:0.5:2.5 Adult:Elder:Child ratio).
    pub fn default_households(households: f64) -> Self {
        Self::default().mult(households)
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

    /// # Add Households
    /// 
    /// Adds to the count of households.
    /// 
    /// Does not change anything else.
    pub fn add_households(&self, size: f64) -> Self {
        let mut result = self.clone();
        result.count += size;
        result
    }
}

// Household unit functions, kept separate to maintain sanity.
impl Household {
    /// # Add House Units
    /// 
    /// Adds together two household details. Ignoring size entirely.
    /// 
    /// This adds together the household information, accumulating into a final household.
    /// This is used to sum a pop row's changes and effects.
    /// 
    /// Species + culture + etc = our output.
    /// 
    /// # Panics
    /// 
    /// This panics if the results of adding doesn't work. Components should add up to the 
    /// household size.
    pub fn add_house_units(&self, other: &Household) -> Self {
        let result = Self {
            count: 0.0,
            household_size: self.household_size + other.household_size,
            adults: self.adults + other.adults,
            elders: self.elders + other.elders,
            children: self.children + other.children,
        };
        debug_assert!(result.household_size == (result.adults + result.elders + result.children),
            "Other has added to household incorrectly somehow.");
        result
    }

    /// # New Household
    /// 
    /// Creates a new household, the result is 1 household returned.
    pub fn new_household(adults: f64, children: f64, elders: f64) -> Self {
        Self {
            count: 1.0,
            household_size: adults + children + elders,
            adults,
            elders,
            children,
        }
    }

    /// # New Household Mod
    /// 
    /// Creates a new household, with no count.
    pub fn new_household_mod(adults: f64, children: f64, elders: f64) -> Self {
        Self {
            count: 0.0,
            household_size: adults + children + elders,
            adults,
            elders,
            children,
        }
    }

    /// # Change Household Size
    /// 
    /// Safely changes household size without altering population ratios between
    /// the components, adding the size to the households.
    pub fn change_household_size(&self, size: f64) -> Self {
        assert!(self.household_size + size > 0.0, 
            "Household size should not be reduced at or below 0.0.");
        // Get copy to return
        let mut result = self.clone();
        result.household_size += size;
        // Get ratios of the original to copy over.
        let adult_r = self.adults / self.household_size;
        let child_r = self.children / self.household_size;
        let elder_r = self.elders / self.household_size;
        result.adults = result.household_size * adult_r;
        result.children = result.household_size * child_r;
        result.elders = result.household_size * elder_r;
        result
    }

    /// # Add Elders
    /// 
    /// Adds elders to the basic household, this does alter the size of the household.
    pub fn add_elders(&self, elders: f64) -> Self {
        assert!(self.elders + elders > 0.0, "Elders cannot reduce self.adults below 0.0.");
        assert!(self.household_size + elders > 0.0, "Elders cannot reduce household size below 0.0.");

        let mut res = self.clone();
        res.household_size += elders;
        res.elders += elders;
        res
    }

    /// # Add Children
    /// 
    /// Adds Children to the basic household, this does alter the size of the household.
    pub fn add_children(&self, children: f64) -> Self {
        assert!(self.children + children > 0.0, "Children cannot reduce self.adults below 0.0.");
        assert!(self.household_size + children > 0.0, "Children cannot reduce household size below 0.0.");

        let mut res = self.clone();
        res.household_size += children;
        res.children += children;
        res
    }

    /// # Add Adult
    /// 
    /// Adds adults to the basic household, this does alter the size of the household.
    pub fn add_adults(&self, adults: f64) -> Self {
        assert!(self.adults + adults > 0.0, "Adult cannot reduce self.adults below 0.0.");
        assert!(self.household_size + adults > 0.0, "Adult cannot reduce household size below 0.0.");

        let mut res = self.clone();
        res.household_size += adults;
        res.adults += adults;
        res
    }

    /// # Alter Ratios
    /// 
    /// Safely moves pops from one component to another without changing size.
    /// 
    /// # Panics
    /// 
    /// Enforces the household_size is maintained (adult + child + elder = 0) and 
    /// that resulting adult, child, and elder values are positive.
    pub fn alter_ratios(&self, adult: f64, child: f64, elder: f64) -> Self {
        assert!((adult + child + elder == 0.0),  "Parameters must sum to 0.0.");
        assert!(self.adults + adult > 0.0, "Adult cannot be larger than self.adults.");
        assert!(self.children + child > 0.0, "Child cannot be larger than self.children.");
        assert!(self.elders + elder > 0.0, "Elders cannot be larger than self.elders.");
        let mut res = self.clone();
        res.adults += adult;
        res.children += child;
        res.elders += elder;
        res
    }

    /// # Is Unit
    /// 
    /// Checks to see if this household is a 'unit' household of size 1.
    pub fn is_unit(&self) -> bool {
        self.count == 1.0
    }

    /// # Is real household
    /// 
    /// Households that are real have a positive count. Anything else means 
    /// it's not properly calibrated for actual use as a part of Pop.
    pub fn is_real_household(&self) -> bool {
        self.count > 0.0
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
}

impl default::Default for Household {
    /// # Household Default
    /// 
    /// 1 household of household size 5, 2 adults, 0.5 elders, and 2.5 children.
    fn default() -> Self {
        Self { 
            count: 1.0,
            household_size: 5.0, 
            adults: 2.0, 
            elders: 0.5, 
            children: 2.5 
        }
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