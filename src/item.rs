/// # Item
/// 
/// Item is an enum for pointing to a want, class, or specific good.
#[derive(Clone, Copy, Debug)]
pub enum Item {
    Want(usize),
    Class(usize),
    Good(usize)
}

impl Item {
    /// # Unwrap
    /// 
    /// Gets the ID in the item.
    pub fn unwrap(&self) -> usize {
        match self {
            Item::Want(id) => *id,
            Item::Class(id) => *id,
            Item::Good(id) => *id,
        }
    }

    /// # Is Want
    /// 
    /// If the item is a want.
    pub fn is_want(&self) -> bool {
        match self {
            Item::Want(_) => true,
            Item::Class(_) |
            Item::Good(_) => false,
        }
    }

    /// # Is Want
    /// 
    /// If the item is a want.
    pub fn is_class(&self) -> bool {
        match self {
            Item::Class(_) => true,
            Item::Want(_) |
            Item::Good(_) => false,
        }
    }

    /// # Is Want
    /// 
    /// If the item is a want.
    pub fn is_good(&self) -> bool {
        match self {
            Item::Good(_) => true,
            Item::Class(_) |
            Item::Want(_) => false,
        }
    }
}