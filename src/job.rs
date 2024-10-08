use std::collections::{HashMap, VecDeque};

use crate::{data::Data, market::GoodData, pop::Pop};

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
    /// 
    /// When paying out wages, a we will prioritize using these first, and will treat 
    /// them as being full AMV when offered. If there is a shortfall, the job will 
    /// supplement it with other goods, their amv modified by salability.
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
    /// When planning, how many days worth of goods it wants to have at the end
    /// of the day. Defaults to 2. Currently will not change, may need to change
    /// if more market volatility is introduced.
    pub excess_input_target: f64,

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
    /// # Pay Workers
    /// 
    /// Purchases labor from workers, and takes time appropriate to the
    /// wage selected. If unable to pay all, it instead pays what it can
    /// and takes it appropriately.
    /// 
    /// If the job has no owner, it instead takes all property from the pop,
    /// as it assumes that it will return everything at the end of the work day.
    /// 
    /// If job has an owner, it pays the agreed upon wage and takes time
    /// to match that. If unable to pay for all the time it wants, it pays everything it
    /// can and gets as much time as it can, with the time recieved reduced by the AMV
    /// missing from the total needed.
    /// 
    /// Workers do not negotiate here, barter for time occurs near the end of the day,
    /// if unable to pay for all time today, it only purchases as much as it can.
    pub fn pay_workers(&mut self, pops: &mut HashMap<usize, Pop>, 
    good_info: &HashMap<usize, GoodData>) {
        if let Some(_) = self.owner { // if owned
            // get the worker pop
            let pop = pops.get_mut(&self.workers).expect("Pop not found.");
            // get total time we'll try to get
            let mut time = self.time_purchase.min(pop.unused_time);
            // calculate the hourly AMV per hour based on specific wage.
            let hourly_amv = self.wage.iter()
                .map(|(good, amt)| {
                    amt * good_info.get(good).unwrap().amv
                }).sum::<f64>();
            let total_amv_cost = time * hourly_amv; // total AMV cost for the time we want.
            // get how much we have in actual wages
            let mut expending_amv = 0.0;
            let mut paycheck = HashMap::new();
            for (&good, amt) in self.wage.iter() {
                let good_data = good_info.get(&good).unwrap(); // get good data
                // insert what we have (up to what we should insert)
                paycheck.insert(good, (amt * time).min(*self.property.get(&good).unwrap_or(&0.0)));
                let expending = paycheck.get(&good).unwrap(); // how much we are/can expend
                expending_amv += expending * good_data.amv; // add to expending
                self.property.entry(good).and_modify(|x| *x -= expending); // remove from property.
            }
            if expending_amv < total_amv_cost { // if not enough AMV (not enough goods)
                // reduce time purchase to match how much we're able to purchase (round down).
                let reduction = expending_amv / total_amv_cost;
                time = (time * reduction).floor(); // reduce by the fractional difference,
            }
            // take the time from the pop and give them their wage.
            pop.unused_time -= time;
            for (good, amt) in paycheck {
                pop.property.entry(good)
                    .and_modify(|x| *x += amt)
                    .or_insert(amt);
            }
        } else { 
            // No owner, so pops are the job
            // Pops give everything to the job, then return all goods to the 
            // pop at the end of the work day.
            let pop = pops.get_mut(&self.workers).expect("Pop not found.");
            // move time over.
            self.time = pop.unused_time;
            pop.unused_time = 0.0;
            // move over property.
            let ids: Vec<usize> = pop.property.keys().cloned().collect();
            for good in ids {
                let amt = pop.property.remove(&good).unwrap();
                self.property.entry(good)
                    .and_modify(|x| *x += amt)
                    .or_insert(amt);
            }
        }
    }

    /// # Workday
    /// 
    /// Runs the work for the day, excluding buying and selling.
    /// 
    /// This 
    pub fn workday(&mut self) {
        // 
    }

    /// # Do Work
    /// 
    /// Does the work of the day.
    /// 
    /// Goes through each process, doing what it can.
    /// 
    /// Records and collects results into Work Results.
    /// 
    /// Returns updated work results
    pub fn do_work(&mut self, data: &Data, mut prev_results: WorkResults) -> WorkResults {
        for p in self.process.iter() {
            // get time target capped at what time we have available.
            let time = self.target.get(p).unwrap().min(self.time);
            // get the process
            let proc = data.processes.get(p).unwrap();
            // do the process.
            let proc_results = proc.do_process(time, &self.property, &data);
            // with process results gotten, apply to work results and remove/use stuff.
            for (&good, &quant) in proc_results.used.iter() {
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
            // record created goods
            for (&good, &quant) in proc_results.created.iter() {
                self.property.entry(good)
                    .and_modify(|x| *x += quant)
                    .or_insert(quant);
                prev_results.produced_goods
                    .entry(good)
                    .and_modify(|x| *x += quant);
            }
            // record time used
            prev_results.process_time_success.entry(*p)
                .and_modify(|x| *x += proc_results.time_used)
                .or_insert(proc_results.time_used);
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
    /// The goods produced 
    pub produced_goods: HashMap<usize, f64>,
}