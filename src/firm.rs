use std::collections::{HashMap, HashSet};

use crate::{data::Data, pop::Pop};


/// # Firm
/// 
/// A firm is the primary economic unit of the simulation. 
/// 
/// It handles all production and economic management of said production.
#[derive(Debug, Clone)]
pub struct Firm {
    /// Firm ID, should be unique across the entire game space.
    pub id: usize,

    /// The Name of the firm, likely to be auto-generated, may be altered by player.
    pub name: String,
    /// The ID of the market the firm is headquartered in.
    pub market: usize,

    // Firm Structural Connections
    /// The Firm which is above this one (if there is one).
    pub parent: Option<usize>,
    /// Children Firms of this one.
    pub children: HashSet<usize>,
    /// How many total current shares the firm has outstanding (should be equal to 
    /// total shares in workers).
    /// 
    /// These shares only apply to the workers and are always loss sharing.
    /// 
    /// Limited Liability shares, (Voting/Nonvoting and Dividend/Non-dividend) which 
    /// don't transfer debt are treated seperately from these as they go through a 
    /// financial firm/institution.
    pub shares: usize,

    /// The property of the firm.
    /// NOTE: Currently simplified to bare minimum.
    pub property: HashMap<usize, f64>,

    /// The work force of the firm (includes owners, owner/operators, worker-owners, 
    /// and equity (loss) sharing shareholders).
    pub workers: HashMap<usize, WorkerInfo>,
    /// The length of the workday for the firm. Helps define how many hours it takes from each
    /// worker. Sholud be the same length for all pops.
    /// 
    /// NOTE: This may be integrated into the WorkerInfo as a business may have mulitple shifts or different shifts.
    pub shift_length: f64,
    /// How many shifts the firm has in a given day. This value times shift_length 
    /// should not be greater than the number of hours in a market day.
    pub shifts: f64,
}
impl Firm {
    pub fn work_day_exchange(&self, data: &Data, pop: &mut Pop) -> 
    (HashMap<usize, f64>, HashMap<usize, f64>) {
        todo!()
    }
}

/// # Worker Info
/// 
/// Helper sturct to define data about our workers.
#[derive(Debug, Clone)]
pub struct WorkerInfo {
    /// The kind of worker they are.
    pub worker_type: WorkerType,
    /// What they are paid per unit day.
    /// 
    /// NOTE: This is effectively always an 'hourly' wage as the worker cannot decide to increase or reduce hours work from their side.
    /// NOTE: Currently, this is a daily wage, meaning it comes out every day before they work as opposed to a 'paycheck' which would come out once every few days or so, typically after the work has been done.
    pub wage: HashMap<usize, f64>,
    // TODO: include wage type, pay schedule, and wage complications here.
    
    /// What the firm is recieving from the pop each day.
    /// 
    /// This is always daily, at the start of the day.
    /// 
    /// Skills taken in this are not taken but instead copied from the pop in question.
    pub labors: HashMap<usize, f64>,

    /// How many shares (if any) this worker has to it's name.
    /// 
    /// Shares here are always loss sharing, meaning excess losess from the firm will be
    /// drawn from this worker in proportion with it's shares.
    pub shares: usize,

    /// How many individual workers the 
    pub worker_cap: usize,
}

/// # Worker Type
/// 
/// The types of workers for a firm.
#[derive(Debug, Clone)]
pub enum WorkerType {
    /// The ultimate owner of the firm. Excess profits and losses come from this 
    /// pop. They proved work like workers, but are distinguished from normal workers.
    /// 
    /// LLCs and similar incorporated firms don't have one of these.
    Owner,
    /// A Shareholder is someone who owns part of the business, but does not work it.
    /// 
    /// This is meant for direct private shareholders, public shareholders are 'owned' 
    /// by proxy via a Stock Market Firm.
    /// 
    /// These pops are loss sharing, and will be made to pay for losses like a worker.
    Shareholder,
    /// A worker of in the business. 
    Worker,
    /// A worker of the firm, but typically only on for a short time. Often
    /// hired only for a short time. Never has ownership.
    Contractor,
    /// A slave worker, owned/working for the firm.
    Slave,
}

/// # Unit
/// 
/// A Unit is a mobile collection of resources. and people.
/// 
/// Should include everything needed for motion, influence, power projection, and population management.
#[derive(Debug, Clone)]
pub struct Unit {

}

/// # Institution
/// 
/// An Institution is an organization of a civilization, and a branch of a Hall of 
/// Power.
#[derive(Debug, Clone)]
pub struct Institution {

}

/// # Hall of Power
/// 
/// A Hall of power is the lower branch of a Civ and represents the overarching 
/// components that makes up a civilization.
#[derive(Debug, Clone)]
pub struct HallofPower {

}

/// # Civilization
/// 
/// A Civilization is a player. It organizes and centralizes control of everything that is
/// underneath it.
#[derive(Debug, Clone)]
pub struct Civ {

}

/// # Territory
/// 
/// A territory is an individual Map Tile.
#[derive(Debug, Clone)]
pub struct Territory{

}