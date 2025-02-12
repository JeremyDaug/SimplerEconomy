/// # Item
/// 
/// Item is an enum for pointing to a want, class, or specific good.
#[derive(Clone, Copy, Debug, Hash)]
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

    /// # Is Class
    /// 
    /// If the item is a class.
    pub fn is_class(&self) -> bool {
        match self {
            Item::Class(_) => true,
            Item::Want(_) |
            Item::Good(_) => false,
        }
    }

    /// # Is Good
    /// 
    /// If the item is a good.
    pub fn is_good(&self) -> bool {
        match self {
            Item::Good(_) => true,
            Item::Class(_) |
            Item::Want(_) => false,
        }
    }
}

/// # Product
/// 
/// A product is specifically a class or good.
/// 
/// This always points to a good of some kind or another.
#[derive(Clone, Copy, Debug, Hash)]
pub enum Product {
    Class(usize),
    Good(usize)
}

impl Product {
    pub fn unwrap(&self) -> usize {
        match self {
            Product::Class(id) => *id,
            Product::Good(id) => *id,
        }
    }

    /// # Is Class
    /// 
    /// If the item is a class.
    pub fn is_class(&self) -> bool {
        match self {
            Product::Class(_) => true,
            Product::Good(_) => false,
        }
    }

    /// # Is Good
    /// 
    /// If the item is a good.
    pub fn is_good(&self) -> bool {
        match self {
            Product::Good(_) => true,
            Product::Class(_) => false,
        }
    }
}