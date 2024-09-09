/// # Good
/// 
/// A good is something that is desireable or useful.
#[derive(Clone, Debug)]
pub struct Good {
    /// Unique id of the good.
    pub id: usize,
    /// The name of the good. (defaults to G(id))
    pub name: String,
    /// The durability of the good.
    /// If valued between 0.0 and 1.0 (inclusive) that represents how many will survive each day.
    /// 0.0 means all decay at the end of the day. 1.0 means none of them do. 
    /// Decayed goods round up to the nearest value.
    /// 
    /// If above 1.0 then the good is produced via "Durability", ie, whenever produced 
    /// by a process it multiplies the goods it produces (plus efficiency gain) times 
    /// this value, to produce units of durability. 
    /// From there, the number of these durability goods, divided by this value, and 
    /// rounded up produce the number of durability which decays each day.
    /// 
    /// These "Durability points" may be 'sold' freely (think of it as renting).
    /// 
    /// Goods with a durability of greater than 1 can be thought of as being capital,
    /// though this isn't a perfect fit.
    pub durability: f64,
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
    /// # Production Multiplier
    /// 
    /// Whenever a good is made, if it has a durability greater than 1, the good units
    /// get multiplied by this value. The multiplier is the durability of the good in
    /// question.
    /// 
    /// While durability above 1 should be a whole value, we ceiling it just in case.
    pub fn output_mult(&self) -> f64 {
        self.durability.ceil()
    }

    /// # Decay
    /// 
    /// Returns how many goods survive at the end of the day.
    /// 
    /// Always returns whole value, expects whole values, but accepts non-whole.
    pub fn decay(&self, quantity: f64) -> f64 {
        if self.durability <= 1.0 {
            (quantity * self.durability).floor()
        } else {
            let units = (quantity / self.durability).ceil();
            (quantity - units).floor()
        }
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
}