use std::{collections::{HashMap, VecDeque}, iter};

use itertools::Itertools;

use crate::{data::Data, market::{GoodData, Market}, pop::Pop};

const EXCESS_INPUT_MIN: f64 = 2.0;

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

    /// What processes the job entails.
    /// Also orders them by when which it tries to do first, and
    /// which it prioritizes.
    pub process: Vec<usize>,

    /// The processes, and how many hours it wants to put into each process.
    pub target: HashMap<usize, f64>,
    /// When planning for savings between days, it will want a defacto minimum
    /// set by EXCESS_INPUT_MIN above. This is the cap on total resources it will
    /// want to keep in either goods or resources. Any excess above this target
    /// will be given over to the owner, regardless of the dividend.
    /// 
    /// EXCESS_INPUT_MIN is the lower bound of this savings. If unable to reach
    /// the minimum bound, it will reduce the dividend until it's above.
    pub excess_input_max: f64,
    /// This is the dividend which it defaults to sending to the owner when between
    /// EXCESS_INPUT_MIN and self.excess_input_max.
    pub dividend: f64,

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
    /// 
    /// Non-Worker Owners are treated differently from worker owners. Instead of giving
    /// everything over, the owner is payed excess profits from the business. These
    /// profits are payed out here. All of a job's inputs, up to self.excess_input_max
    /// If this is enough to reach max, then all other property is sent over to the 
    /// owner. If it's not enough to reach EXCESS_INPUT_MIN, then all other property
    /// will go to purchasing the job's inputs (assuming AMV * 0.5), after that,
    /// all property will be split 50/50 between the job and the owner.
    /// 
    /// TODO take into account loans/investment in the business from other accounts. Likely just added to the reserve step and payed out as possible.
    pub fn pay_workers(&mut self, pops: &mut HashMap<usize, Pop>, 
    data: &Data, market: &Market,) {
        if let Some(_) = self.owner { // if owned
            // get the worker pop
            let pop = pops.get_mut(&self.workers).expect("Pop not found.");
            // get total time we'll try to get
            let mut time = self.time_purchase.min(pop.unused_time);
            // calculate the hourly AMV per hour based on specific wage.
            let hourly_amv = self.wage.iter()
                .map(|(good, amt)| {
                    amt * market.get_good_info(good).amv
                }).sum::<f64>();
            let total_amv_cost = time * hourly_amv; // total AMV cost for the time we want.
            // get how much we have in actual wages
            let mut expending_amv = 0.0;
            let mut paycheck = HashMap::new();
            for (&good, amt) in self.wage.iter() {
                let good_data = market.get_good_info(&good); // get good data
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
            self.time = time;
            // reserve input goods and other property to get more inputs goods.
            let mut reserve = HashMap::new();
            let mut dividend = HashMap::new();
            let mut reserved_amv = 0.0;
            let mut available_amv = 0.0;
            let mut min_amv = 0.0;
            let mut max_amv = 0.0;
            let inputs = self.process_inputs(data);
            // sum up our input needs, min, and max.
            for (good, amt) in inputs.iter() {
                let good_data = market.get_good_info(good);
                min_amv += good_data.amv * amt * EXCESS_INPUT_MIN;
                max_amv += good_data.amv * amt * self.excess_input_max;
            }
            for (good, amt) in self.property.iter() {
                let good_amv = market.get_good_info(good).amv;
                available_amv += good_amv * amt;
            }
            // with amv levels gotten, divide up property based on our splits.
            // start by reserving all inputs up to our maximum.
            for (good, amt) in inputs.iter() {
                // Get good data
                let good_data = market.get_good_info(good);
                // How many we can shift, capping at our min target.
                let shift = (amt * self.excess_input_max)
                    .min(*self.property.get(good).unwrap_or(&0.0));
                // add to amv
                reserved_amv += shift * good_data.amv;
                // and shift from property to reserve.
                reserve.entry(*good)
                    .and_modify(|x| *x += shift)
                    .or_insert(shift);
                self.property.entry(*good)
                    .and_modify(|x| *x -= shift);
            }
            // TODO paying off loans would likely go here.
            // with all inputs reserved up to our max, deal with shifting the rest
            if reserved_amv >= max_amv { // more than enough already reserved.
                println!("Above Max");
                let clone = self.property.clone();
                for (good, amt) in clone.iter() {
                    self.property.remove(good); // always remove from property for safety reasons.
                    if *amt == 0.0 { continue; } // if property is empty, just throw it away.
                    let shift = amt.floor(); // shift whole valu.
                    let remainder = amt - shift; // reserve the fractional excess.
                    if remainder > 0.0 {
                        reserve.entry(*good)
                            .and_modify(|x| *x += remainder)
                            .or_insert(remainder);
                    }
                    if shift > 0.0 {
                        dividend.entry(*good)
                            .and_modify(|x| *x += shift)
                            .or_insert(shift);
                    }
                }
            } else if reserved_amv < min_amv {
                println!("Below Min");
                // if unable to reach the minimum, shift over goods until we get past it
                for good in market.get_good_trade_priority() {
                    if self.property.contains_key(good) {
                        let target_amv = min_amv - reserved_amv; // How much AMV is left to get
                        let good_amv = market.get_good_info(good).amv; // good's amv
                        // how many of the goods (with amv reduced) are needed to reach the taregt.
                        let target_goods = (target_amv / (good_amv * 0.5)).ceil();
                        let shift = target_goods // gow many (whole) units we can reserve.
                            .min(self.property.get(good).unwrap().floor());
                        reserved_amv += shift * good_amv * 0.5; // add to our reserved amv
                        // add goods to our reserve.
                        reserve.entry(*good)
                            .and_modify(|x| *x += shift)
                            .or_insert(shift);
                        // remove from property.
                        self.property.entry(*good)
                            .and_modify(|x| *x -= shift);
                    }
                    // if we reached ouur minimum, gfto.
                    if reserved_amv >= min_amv { break; }
                }
                // Having reached the minimum, shift to splitting 50/50 with the owner.
                let mut available_amv = 0.0;
                for (good, amt) in self.property.iter() {
                    let good_amv = market.get_good_info(good).amv;
                    available_amv += good_amv * amt;
                }
                let to_each = available_amv / 2.0;
                let (mut job_amv, mut owner_amv) = (to_each, to_each);
                let unused_prop = self.property.clone(); // copy property.
                self.property.clear(); // clear it out for later purposes.
                for (good, mut amt) in unused_prop.into_iter() {
                    // get good amv
                    let good_amv = market.get_good_info(&good).amv;
                    // move any fractional goods over to the reserve immediately
                    let frac = amt.fract();
                    amt -= frac;
                    if frac > 0.0 {
                        reserve.entry(good)
                            .and_modify(|x| *x += frac)
                            .or_insert(frac);
                        job_amv -= frac * good_amv;
                    }
                    // find parity
                    let odd =(amt / 2.0).fract() > 0.0;
                    if odd { // if odd, give the extra to whoever is behind.
                        amt -= 1.0;
                        if job_amv > owner_amv {
                            reserve.entry(good)
                                .and_modify(|x| *x += 1.0)
                                .or_insert(1.0);
                        } else {
                            dividend.entry(good)
                                .and_modify(|x| *x += 1.0)
                                .or_insert(1.0);
                        }
                    }
                    // split the remaining whole amonut in two and give them out.
                    let res = amt / 2.0;
                    reserve.entry(good)
                        .and_modify(|x| *x += res)
                        .or_insert(res);
                    dividend.entry(good)
                        .and_modify(|x| *x += res)
                        .or_insert(res);
                    let amv_gain = res * good_amv;
                    job_amv -= amv_gain;
                    owner_amv -= amv_gain;
                }
            } else {// if between min and max, split the property between owner and job by amv.
                println!("Between Min and Max");
                // fractional goods stay with the job.
                println!("Available AMV: {}", available_amv);
                println!("Reserved AMV: {}", reserved_amv);
                let to_each = (available_amv - reserved_amv) / 2.0;
                let (mut job_amv, mut owner_amv) = (to_each, to_each);
                let unused_prop = self.property.clone(); // copy property.
                self.property.clear(); // clear it out for later purposes.
                for (good, mut amt) in unused_prop.into_iter() {
                    // get good amv
                    let good_amv = market.get_good_info(&good).amv;
                    // move any fractional goods over to the reserve immediately
                    let frac = amt.fract();
                    amt -= frac;
                    if frac > 0.0 {
                        reserve.entry(good)
                            .and_modify(|x| *x += frac)
                            .or_insert(frac);
                        job_amv -= frac * good_amv;
                    }
                    // find parity
                    let odd =(amt / 2.0).fract() > 0.0;
                    if odd { // if odd, give the extra to whoever is behind.
                        amt -= 1.0;
                        if job_amv > owner_amv {
                            reserve.entry(good)
                                .and_modify(|x| *x += 1.0)
                                .or_insert(1.0);
                        } else {
                            dividend.entry(good)
                                .and_modify(|x| *x += 1.0)
                                .or_insert(1.0);
                        }
                    }
                    // split the remaining whole amonut in two and give them out.
                    let res = amt / 2.0;
                    reserve.entry(good)
                        .and_modify(|x| *x += res)
                        .or_insert(res);
                    dividend.entry(good)
                        .and_modify(|x| *x += res)
                        .or_insert(res);
                    let amv_gain = res * good_amv;
                    job_amv -= amv_gain;
                    owner_amv -= amv_gain;
                }
            }
            // with reserve and dividend gotten, move the goods where they need to go.
            let owner = pops.get_mut(&self.owner.unwrap())
                .expect(format!("Owner Pop '{}' has not been found.", self.owner.unwrap()).as_str());
            for (good, amt) in dividend.into_iter() {
                owner.property.entry(good)
                    .and_modify(|x| *x += amt)
                    .or_insert(amt);
            }
            // don't forget to move the reserve back into job's property also.
            for (good, amt) in reserve.into_iter() {
                self.property.entry(good)
                    .and_modify(|x| *x += amt)
                    .or_insert(amt);
            }

        } else { // No owner, so pops are the job
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

    /// # Process Inputs
    /// 
    /// The overall cost of input goods for our processes, as decided by
    /// plans. Does not take into account optional goods.
    pub fn process_inputs(&self, data: &Data) -> HashMap<usize, f64> {
        let mut result = HashMap::new();
        for (process_id, time) in self.target.iter() {
            let process = data.processes.get(process_id)
                .expect(format!("Process '{}' not found!", process_id).as_str());
            let iterations = time / process.time;
            for (&input, &amt) in process.inputs.iter() {
                result.entry(input)
                    .and_modify(|x| *x += amt * iterations)
                    .or_insert(amt * iterations);
            }
        }
        result
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
                    .and_modify(|x| *x += quant)
                    .or_insert(quant);
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