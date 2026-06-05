use std::collections::HashMap;

use hexx::Hex;

use crate::game::market::Market;

/// # Firm 
/// 
/// A firm is the smallest unit of business. It deals with Production and local economic
/// calculation
#[derive(Debug, Clone)]
pub struct Firm {
    /// Unique Id for the Firm.
    pub id: usize,
    /// The unique name of the firm. If a child of another firm, this is it's 
    /// regional/sub name.
    pub name: String,

    /// Which market this firm is attached to and operating in.
    pub market: usize,
    /// The Specific Hex Tile the firm is centered in, for market splits and joins.
    pub location: Hex,

    /// The Parent firm to this firm (if it has one).
    pub parent: Option<usize>,
    /// The Children Firm for 
    pub children: Vec<usize>,
    /// The organizational level of the firm. 0 is lowest. if part of a larger Company
    /// it may be higher.
    /// 
    /// Firms of an organizational level can only control within 1 level of difference.
    pub level: usize,
    /// What kind of organization scheme the firm is operating under. Can only be
    /// changed by the highest level
    pub org_system: FirmOrganization,

    /// The Property owned by the firm. In some cases, this can be shared with the owner
    /// if it's an especially small business, but for most purposes, this is separate 
    /// and distinct.
    pub property: HashMap<usize, FirmPRow>,

    /// The details of the processes and work the firm will do.
    /// 
    /// Production lines are ordered by priority, those first in the list get run
    /// first. This should be noted for production lines that feed into each other.
    pub production_line: Vec<ProductionLine>,

    /// Information on who own's the firm, profits and risk distribution, and other
    /// such information.
    pub owners: Owners,

    /// Information on the workers, how many there are, how much they're payed, what 
    /// they transer over and back, and similar information.
    pub workforce: Workforce,

    /// Contracts are long term deals that the firm has, typically buy or sell orders
    /// to other firms, but it also forms a secondary source of labor in contactors,
    /// as well as connecting to institutions and states for access to their stuff.
    pub contracts: Contract,
}
// 
/// # Firm Organization
/// 
/// This is a set of categories that define Firm and Sub-firm actions, mostly internally.
/// 
/// It defines how Bureaucratic loads are distributed, modifies base Costs of Scale,
/// defines financial and resource sharing, and strategic level decision making.
#[derive(Debug, Clone)]
pub enum FirmOrganization {
    /// A special status to represent a large number of singular businesses, like 
    /// subsistance farms.
    /// 
    /// Disorganized Firms have no Costs of Scale, but also little control over
    /// their prices, often competing with themselves to their own detriment.
    /// 
    /// They also cannot be part of a larger Company. They can be dismantled or 
    /// bought out, but no more.
    Disorganized,
    /// A small business that owns a single building and does very few things.
    SmallBusiness,
    /// A group of smaller businesses that, while nominally independent and 
    /// separately owned, they work together closely for various purposes. 
    /// 
    /// A limitation of this structure is that businesses under it are generally small
    /// and financially independent, allowing them to go bankrupt or break away when
    /// it's convenient for them.
    /// 
    /// While they are independent at the sub-firm level, they have an easy time forming
    /// contracts with each other, giving preferential treatment to those in the
    /// assossiation.
    BusinessAssossiation,
    /// A business that's large and singularly owned, may have many different parts,
    /// but they are all at the attention of the head and can be organized to it's end.
    PrivateCompany,
    /// A Business that is owne
    PublicCorporation,
    Conglomerate,
    DecentralizedFirm,
}

#[derive(Debug, Clone)]
pub struct Workforce {

}

#[derive(Debug, Clone)]
pub struct Contract {}

/// # Owners
/// 
/// Owners defines how a firm is owned, who owns it, profit and loss distribution,
/// and other factors, like some high level business logic.
/// 
/// Currently a placeholder.
#[derive(Debug, Clone)]
pub struct Owners {

}

/// # Production Line
/// 
/// A Production line is a process and the information around it. This includes
/// targets, and input restrictions.
#[derive(Debug, Clone)]
pub struct ProductionLine {
    /// The process being run.
    pub process: usize,
    /// The target being sought. If None, then the firm wants to do as many as possible.
    pub target: Option<usize>,
    /// What goods are going to go into the process. Used to restrict optional inputs.
    pub inputs: Vec<usize>,
    /// A record of the average productivity (amv out / amv in) of the process.
    pub historical_productivity: f64,
}

/// # Firm Property Row
/// 
/// A row of property data for a Firm. Includes data for management, oversight, and 
/// targeting for both purchasing and use in production.
#[derive(Debug, Clone)]
pub struct FirmPRow {
    /// The amount currently owned.
    pub quantity: f64,
    /// The average ownership of the good over the last 30 days.
    pub rolling_average: f64,
    /// The target amount the firm wants to have after shopping.
    /// This should be before production processes.
    pub target: f64,
    /// If trading the good, this is the amount that the firm will never willingly 
    /// part with.
    pub reserve: f64,

    /// The average cost to get these good so far. Updated after each purchase and
    /// productive process.
    /// Used for value production efficiency calculations.
    pub average_cost: f64,
}