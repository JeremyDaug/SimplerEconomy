use std::collections::{HashMap, VecDeque};

use crate::data::Data;

/// # Job
/// 
/// Jobs are defined by what processes they do, the pops which do them, and where they
/// do it.
/// 
/// There are no businesses, instead there are just jobs. If/When firms are added,
/// jobs will be genericised to being just the processes involved.
#[derive(Debug, Clone)]
pub struct Job {
    /// The unique ID of the job.
    /// Currently, you can only have 1 job in each market.
    pub id: usize,
    /// The name of the job, doesn't need to be unique, but should be
    /// distinguishable.
    pub name: String,

    // Workforce
    /// The id of the pop who does work here.
    pub workers: usize,
    /// How much and in what goods workers are payed. These are reevaluated daily.
    /// 
    /// This is the wage paid per hour, so multiply by the time_purchase for total wage.
    pub wage: HashMap<usize, f64>,
    /// How much time the job purchases with it's wage.
    pub time_purchase: f64,

    /// The pop which owns the job, and thus gains any profits from it.
    pub owner: Option<usize>,

    /// Links to any Loans the job has taken out.
    pub lenders: Vec<usize>,

    /// WHat processes the job entails.
    /// Also orders them by when which it tries to do first, and
    /// which it prioritizes.
    pub process: Vec<usize>,

    /// The processes, and how many hours it wants to put into each process.
    pub target: HashMap<usize, f64>,

    /// Property owned by the job (and the owner technically)
    pub property: HashMap<usize, f64>,
    /// The currently available time the job has control over.
    pub time: f64,

    /// Limited length detailed history.
    /// 
    /// Records end of day property results. Should only store past 5-10 days.
    /// May expand later.
    pub property_history: VecDeque<HashMap<usize, f64>>,
    /// The AMV valuation of the job over previous time periods.
    /// 
    /// Currently should only hold 50 days worth. May expand later.
    pub amv_history: VecDeque<f64>,
}

impl Job {
    /// # Runs the work for the day, excluding buying and selling.
    pub fn workday(&mut self) {

    }

    /// # Do Work
    /// 
    /// Does the work of the day.
    /// 
    /// Goes through each process, doing what it can.
    /// 
    /// Records and collects results into Work Results.
    /// 
    /// returns updated work results
    pub fn do_work(&mut self, data: &Data, mut prev_results: WorkResults) -> WorkResults {
        let mut expended = HashMap::new();
        for p in self.process.iter() {
            // get time target capped at what time we have available.
            let time = self.target.get(p).unwrap().min(self.time);
            // get the process
            let proc = data.processes.get(p).unwrap();
            // do the process.
            let proc_results = proc.do_process(time, &self.property, &data);
            // with process results gotten, apply to work results and remove/use stuff.
            for (&good, &quant) in proc_results.used.iter() {
                expended.entry(good)
                    .and_modify(|x| *x += quant)
                    .or_insert(quant);
                self.property.entry(good)
                    .and_modify(|x| *x -= quant);
                prev_results.goods_used.entry(good)
                    .and_modify(|x| *x += quant)
                    .or_insert(quant);
            }
            // apply results of consumed goods
            for (&good, &quant) in proc_results.consumed.iter() {
                self.property.entry(good)
                    .and_modify(|x| *x -= quant);
                prev_results.goods_consumed
                    .entry(good)
                    .and_modify(|x| *x += quant)
                    .or_insert(quant);
            }
            // 

        }

        prev_results
    }
}

/// # Work results
/// 
/// The results of a job doing their work. 
/// 
/// Includes
/// - Input Costs in exchanged goods.
/// - Input costs in goods consumed and used.
/// - Wages paid out.
/// - Interest costs payed.
pub struct WorkResults {
    /// The cost to purchase input goods (and in what form).
    pub input_costs: HashMap<usize, f64>,
    /// Goods consumed in work.
    pub goods_consumed: HashMap<usize, f64>,
    /// Goods used but not consumed in work.
    pub goods_used: HashMap<usize, f64>,
    /// How much (and in what form) was wages paid out in total.
    pub wages_paid: HashMap<usize, f64>,
    /// How much (and what) was paid in interest.
    pub interest_paid: HashMap<usize, f64>,
    /// How much time did each process consume.
    pub process_time_success: HashMap<usize, f64>,
}