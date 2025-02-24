/// # Household
/// 
/// This defines the makeup of a household. Each component representing a 
/// different functional aspect of the household.
/// 
/// The Total Size of a household is the maximum number of people in a household, and
/// represents a hypothetical 'maximum' time a household has for 'productive' work.
/// 
/// A portion of this household is working, representing the amount of labor available to
/// the household. This labor is then broken up by the wider pop into labor and leisure, 
/// or more accurately, resource gathering and resource using.
/// 
/// A household is also broken up into 3 categories.
/// - Adults, normal, capable of full work, but give no additional bonuses.
/// - Children, more children equals faster birthrate, but they have lower labor output.
/// - Elders, reduces decay and increases growth of skills, produces research, but 
/// lower labor output and higher mortality.
/// 
/// ## Default Household
/// 
/// The base household that is used as the 'default' for V1.0 is made of 
/// 
/// 2 adults, 0.5 Elders and 2.5 Children.
/// 
/// This should give about 3.0 Labor at maximum capacity.
/// 
/// Adults give 1.0 each.
/// Elders give 0.75 each.
/// Children give 0.3 each.
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
pub struct Household {
    /// The total size of the household representing the average number of
    /// individuals within it.
    /// 
    /// You can think of this as the maximum amount of time 
    /// (total_size * day_length) a household could produce
    /// were everyone in it an adult working full time.
    pub total_size: f64,

    /// How many members of the household are elders.
    pub elders: f64,
    /// How many of the household are children.
    pub children: f64,
}

impl Household {
    
}