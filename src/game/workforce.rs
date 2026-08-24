use std::collections::HashMap;

/// # Workforce
/// 
/// Storage for the workforce of a Firm, which pops at which wage and for how long each 
/// market day. 
/// 
/// Firms should encourage workforce pops to unify when possible to simplify things.
/// 
/// Firm does not actually care about size of a pop, but the pop will respond each day
/// with how much over/under work it is performing and giving the firm first dibs to
/// absorb the extra labor time.
/// 
/// If a firm doesn't directly manage the time balance of employees, instead letting 
/// them grow or shrink as wages and work hours demand. A tad, unrealistic perhaps,
/// but good enough until more complex labor contracts and rules can be added.
/// 
/// This will probably change when different kinds of wages and controls come in.
/// For now, this is a pure 'hourly wage' system, not a salary or contract.
/// 
/// ## Notes
/// 
/// For future purposes, Time Wage is time delimited, buying a specific amount of time
/// and letting workers manage their own size and population. Salary is worker limited,
/// defining how many people the workplace will hire, and dealing with hours second.
/// Salary gives more control to the firm over the population, but in return for more 
/// consistent wages per pop. Salaried has a soft cap on work hours.
/// 
/// Slavery operates as a special case of contract, giving a specific basket of goods in
/// return for work, but with still no control over time worked or workers included.
#[derive(Debug, Clone)]
pub struct Workforce {
    /// The Id of the pop this connects to.
    pub id: usize,
    /// What kind of contract the workforce is under.
    pub contract_type: WorkforceContractType,
    /// The number of workers. Lower number is the minimum number of workers,
    /// upper is the maximum. The upper 
    pub workers: (f64, f64),
    /// The hours (multiplier) applied to labor and possibly payment as well, if the
    /// worker is in the right contract type.
    pub hours: f64,
    /// The 'work unit' from the pop on. This is effectively the 'hourly work' done.
    /// If wage labor, this is multiplied by size, for the number of hours purchased 
    /// from the workers. For salary, this is the measure 
    pub labor: HashMap<usize, f64>,
    /// The payment for their work. This is either 'salaried' meaning it's everything,
    /// or 'waged' meaning it's per multiple of the expected labor.
    pub payment: HashMap<usize, f64>,
}

impl Workforce {
    pub fn empty() -> Self {
        Self {
            id: 0,
            contract_type: WorkforceContractType::Wage,
            workers: (0.0, 0.0),
            hours: 0.0,
            labor: HashMap::new(),
            payment: HashMap::new(),
        }
    }
}

/// # Workforce Contract Type
/// 
/// Defines how workers are paid.
/// 
/// Currently mostly placeholder.
#[derive(Debug, Clone)]
pub enum WorkforceContractType {
    /// Hourly wage, Pop is paid for unit
    Wage,
    /// Paid in profits, the value attached being the percent of AMV profits they take
    /// daily.
    Owner(f64),
}