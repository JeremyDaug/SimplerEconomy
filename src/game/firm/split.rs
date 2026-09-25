//! Found a firm by splitting a divided shop.
//!
//! One workforce pop leaves with `1/n` of each line's quota and a whole-unit
//! share of that line's goods. The child may add one line and/or remove one
//! line in the same act. A one-pop shop cannot split.

use std::collections::{HashMap, HashSet};

use crate::game::actor::Actor;
use crate::game::factuals::Factuals;
use crate::game::good::TIME;
use crate::game::process::ProcessEffect;
use crate::game::util::whole_units;

use super::{Firm, FirmPRow, ProductionLine};

/// Optional one-line change on the child, applied after the scaled copy.
#[derive(Debug, Clone, Copy, Default)]
pub struct SplitLine {
    /// Process to add on the child. `None` adds nothing.
    pub add_process: Option<usize>,
    /// Iteration quota for [`Self::add_process`]. Ignored when that is `None`.
    pub add_target: f64,
    /// Process to drop from the child. The parent keeps its scaled copy.
    pub remove_process: Option<usize>,
}

/// Why [`Firm::split`] refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitReject {
    /// Fewer than two workforce pops. A one-pop shop is not divided.
    NotDivided,
    /// `departing` is not a workforce pop on this firm.
    PopNotHere,
    /// Child id is `0` or the same as the parent.
    SameFirm,
    /// Added process is not in factuals, or it is the process being removed.
    UnknownProcess,
    /// `remove_process` is not on the parent.
    MissingLine,
    /// Added line needs a positive iteration quota.
    BlankTarget,
}

impl Firm {
    /// Found a child firm by split. See [`SplitReject`] for refusals.
    ///
    /// The departing pop leaves this workforce and becomes the remainder
    /// owner of the child. Line quotas scale with `1/n` of the workforce
    /// pops. In-kind stock of line goods moves in whole units; the parent
    /// keeps the remainder. The line stays even when that share is `0`.
    pub fn split(
        &mut self,
        departing: usize,
        child_id: usize,
        child_name: String,
        change: SplitLine,
        factuals: &Factuals,
    ) -> Result<Firm, SplitReject> {
        if child_id == 0 || child_id == self.id {
            return Err(SplitReject::SameFirm);
        }
        let roster = self.workforce_pops();
        if roster.len() < 2 {
            return Err(SplitReject::NotDivided);
        }
        if !roster.contains(&departing) {
            return Err(SplitReject::PopNotHere);
        }
        if let Some(process_id) = change.add_process {
            if change.add_target <= 0.0 {
                return Err(SplitReject::BlankTarget);
            }
            if change.remove_process == Some(process_id) || !factuals.processes.contains_key(&process_id)
            {
                return Err(SplitReject::UnknownProcess);
            }
        }
        if let Some(process_id) = change.remove_process {
            if !self.production_line.iter().any(|line| line.process == process_id) {
                return Err(SplitReject::MissingLine);
            }
        }

        let fraction = 1.0 / roster.len() as f64;
        let goods = line_goods(self, factuals);
        let mut child_property = HashMap::new();
        for good in goods {
            let Some(row) = self.property.get_mut(&good) else {
                continue;
            };
            let child_row = split_row(row, fraction);
            if row_has_stock(&child_row) {
                child_property.insert(good, child_row);
            }
        }

        let mut child_lines = Vec::with_capacity(self.production_line.len());
        for line in &mut self.production_line {
            child_lines.push(scale_line(line, fraction));
        }

        let mut moved = Vec::new();
        let mut index = 0;
        while index < self.workforce.len() {
            if self.workforce[index].id == departing {
                moved.push(self.workforce.remove(index));
            } else {
                index += 1;
            }
        }
        if self.owners.pop_id() == Some(departing) {
            if let Some(stay) = self.workforce.iter().map(|worker| worker.id).find(|id| *id != 0) {
                self.owners.owner = Actor::Pop(stay);
            }
        }

        let mut child = Firm::new(child_id, child_name, self.market, self.location);
        child.owners.owner = Actor::Pop(departing);
        child.owners.liable = true;
        child.workforce = moved;
        child.production_line = child_lines;
        child.property = child_property;
        apply_line_change(&mut child, change, factuals);
        Ok(child)
    }

    fn workforce_pops(&self) -> HashSet<usize> {
        self.workforce
            .iter()
            .map(|worker| worker.id)
            .filter(|id| *id != 0)
            .collect()
    }
}

fn scale_line(line: &mut ProductionLine, fraction: f64) -> ProductionLine {
    let mut child = line.clone();
    if let Some(target) = line.target {
        let moved = target * fraction;
        child.target = Some(moved);
        line.target = Some(target - moved);
    }
    let moved_aim = line.aim * fraction;
    child.aim = moved_aim;
    line.aim -= moved_aim;
    child.last_iterations = 0.0;
    child.last_success_rate = 0.0;
    child.last_amv_consumed = 0.0;
    child.last_amv_produced = 0.0;
    child.last_effects = Vec::<ProcessEffect>::new();
    child.last_missing_goods.clear();
    child.idle_days = 0;
    child
}

fn line_goods(firm: &Firm, factuals: &Factuals) -> HashSet<usize> {
    let mut goods = HashSet::new();
    for line in &firm.production_line {
        for good in &line.inputs {
            if *good != TIME {
                goods.insert(*good);
            }
        }
        let Some(process) = factuals.processes.get(&line.process) else {
            continue;
        };
        for input in &process.inputs {
            if input.good != TIME {
                goods.insert(input.good);
            }
        }
        for output in &process.outputs {
            if output.good != TIME {
                goods.insert(output.good);
            }
        }
    }
    goods
}

/// Child's whole-unit share. Parent keeps the rest, including a share that
/// truncates to `0`.
fn take_share(qty: f64, fraction: f64) -> f64 {
    if qty <= 0.0 {
        return 0.0;
    }
    whole_units(qty * fraction).clamp(0.0, qty)
}

fn split_counted(qty: &mut f64, fraction: f64) -> f64 {
    let share = take_share(*qty, fraction);
    *qty -= share;
    share
}

fn split_row(row: &mut FirmPRow, fraction: f64) -> FirmPRow {
    let mut child = FirmPRow::new();
    child.average_cost = row.average_cost;
    child.average_price = row.average_price;
    child.amv_target = row.amv_target;
    child.amv_bound = row.amv_bound;
    child.quantity = split_counted(&mut row.quantity, fraction);
    child.held = split_counted(&mut row.held, fraction);
    child.use_target = split_counted(&mut row.use_target, fraction);
    child.stock_target = split_counted(&mut row.stock_target, fraction);
    child.sell_target = split_counted(&mut row.sell_target, fraction);
    child.reserve_target = split_counted(&mut row.reserve_target, fraction);
    child.purchase_target = split_counted(&mut row.purchase_target, fraction);
    child.growth_target = split_counted(&mut row.growth_target, fraction);
    row.sync_reserve();
    child.sync_reserve();
    child
}

fn row_has_stock(row: &FirmPRow) -> bool {
    row.quantity > 0.0
        || row.held > 0.0
        || row.use_target > 0.0
        || row.stock_target > 0.0
        || row.sell_target > 0.0
        || row.reserve_target > 0.0
        || row.purchase_target > 0.0
        || row.growth_target > 0.0
}

fn apply_line_change(child: &mut Firm, change: SplitLine, factuals: &Factuals) {
    if let Some(process_id) = change.remove_process {
        child.production_line.retain(|line| line.process != process_id);
    }
    let Some(process_id) = change.add_process else {
        return;
    };
    let Some(process) = factuals.processes.get(&process_id) else {
        return;
    };
    let inputs: Vec<usize> = process
        .inputs
        .iter()
        .filter(|input| input.good != TIME && !input.is_optional())
        .map(|input| input.good)
        .collect();
    child.production_line.push(ProductionLine {
        process: process_id,
        target: Some(change.add_target),
        aim: change.add_target,
        inputs,
        historical_productivity: 0.0,
        last_success_rate: 0.0,
        last_iterations: 0.0,
        last_effects: Vec::new(),
        last_missing_goods: Vec::new(),
        last_amv_consumed: 0.0,
        last_amv_produced: 0.0,
        idle_days: 0,
    });
}

#[cfg(test)]
mod split_should {
    use super::*;
    use crate::game::firm::Firm;
    use crate::game::workforce::Workforce;
    use crate::game::market::Market;
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput};
    use hexx::Hex;

    const GRAIN: usize = 1;
    const WATER: usize = 2;
    const FARM: usize = 29;
    const WELL: usize = 30;
    const EXTRACT: usize = 1;

    fn garden(process: usize, target: f64) -> ProductionLine {
        ProductionLine {
            process,
            target: Some(target),
            aim: target,
            inputs: vec![TIME],
            historical_productivity: 0.0,
            last_success_rate: 1.0,
            last_iterations: target,
            last_effects: Vec::new(),
            last_missing_goods: Vec::new(),
            last_amv_consumed: 0.0,
            last_amv_produced: 0.0,
            idle_days: 0,
        }
    }

    fn factuals() -> Factuals {
        let mut factuals = Factuals::new();
        factuals.processes.insert(
            FARM,
            Process::new(FARM, "farm", 0)
                .with_input(ProcessInput::new(TIME, 0.5, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(GRAIN, 2.0, true)),
        );
        factuals.processes.insert(
            WELL,
            Process::new(WELL, "well", 0)
                .with_input(ProcessInput::new(TIME, 0.2, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(WATER, 2.0, true)),
        );
        factuals.processes.insert(
            EXTRACT,
            Process::new(EXTRACT, "extract", 0)
                .with_input(ProcessInput::new(TIME, 0.5, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(GRAIN, 6.0, true)),
        );
        factuals
    }

    fn shop(pops: usize) -> Firm {
        let mut firm = Firm::new(1, "gardens".into(), 7, Hex::new(0, 0))
            .with_owner(Actor::Pop(1))
            .with_owner_liability();
        for id in 1..=pops {
            let hours = if id == pops { 12.0 } else { 6.0 };
            firm.workforce
                .push(Workforce::new(id).with_hours(hours));
        }
        firm.production_line.push(garden(FARM, 10.0));
        firm.production_line.push(garden(WELL, 20.0));
        firm.property.insert(
            GRAIN,
            FirmPRow::new().with_quantity(10.0).with_use_target(10.0),
        );
        firm.property
            .insert(WATER, FirmPRow::new().with_quantity(25.0));
        firm
    }

    #[test]
    fn one_pop_shop_cannot_split() {
        let mut firm = shop(1);
        let err = firm
            .split(1, 2, "child".into(), SplitLine::default(), &factuals())
            .expect_err("divided");
        assert_eq!(err, SplitReject::NotDivided);
        assert_eq!(firm.workforce.len(), 1);
    }

    #[test]
    fn absent_pop_is_refused() {
        let mut firm = shop(4);
        let err = firm
            .split(99, 2, "child".into(), SplitLine::default(), &factuals())
            .expect_err("missing");
        assert_eq!(err, SplitReject::PopNotHere);
    }

    #[test]
    fn ten_pops_scale_lines_and_whole_unit_stock() {
        let mut firm = shop(10);
        let factuals = factuals();
        let child = firm
            .split(10, 2, "plot".into(), SplitLine::default(), &factuals)
            .expect("split");
        assert_eq!(firm.production_line[0].target, Some(9.0));
        assert_eq!(firm.production_line[1].target, Some(18.0));
        assert_eq!(child.production_line[0].target, Some(1.0));
        assert_eq!(child.production_line[1].target, Some(2.0));
        assert_eq!(firm.property[&GRAIN].quantity, 9.0);
        assert_eq!(child.property[&GRAIN].quantity, 1.0);
        assert_eq!(firm.property[&GRAIN].use_target, 9.0);
        assert_eq!(child.property[&GRAIN].use_target, 1.0);
        assert_eq!(firm.property[&WATER].quantity, 23.0);
        assert_eq!(child.property[&WATER].quantity, 2.0);
        assert_eq!(child.production_line.len(), 2);
        assert!(firm.workforce.iter().all(|worker| worker.id != 10));
        assert_eq!(child.workforce.len(), 1);
        assert_eq!(child.workforce[0].id, 10);
        assert!((child.workforce[0].hours - 12.0).abs() < 1e-12);
        assert_eq!(child.owners.pop_id(), Some(10));
        assert!(child.owners.liable);
        assert_eq!(firm.owners.pop_id(), Some(1));
        assert_eq!(firm.workforce.len(), 9);
    }

    #[test]
    fn tiny_share_stays_on_the_parent_and_the_line_remains() {
        let mut firm = shop(10);
        let grain = firm.property.get_mut(&GRAIN).unwrap();
        grain.quantity = 4.0;
        grain.use_target = 0.0;
        let child = firm
            .split(10, 2, "plot".into(), SplitLine::default(), &factuals())
            .expect("split");
        assert_eq!(firm.property[&GRAIN].quantity, 4.0);
        assert!(child.property.get(&GRAIN).is_none());
        assert!(child.production_line.iter().any(|line| line.process == FARM));
        assert!(firm.production_line.iter().any(|line| line.process == FARM));
    }

    #[test]
    fn child_may_swap_one_line() {
        let mut firm = shop(10);
        let change = SplitLine {
            add_process: Some(EXTRACT),
            add_target: 2.0,
            remove_process: Some(FARM),
        };
        let child = firm
            .split(10, 2, "extract".into(), change, &factuals())
            .expect("split");
        assert!(firm.production_line.iter().any(|line| line.process == FARM));
        assert!((firm.production_line[0].target.unwrap() - 9.0).abs() < 1e-12);
        assert!(child.production_line.iter().all(|line| line.process != FARM));
        let extract = child
            .production_line
            .iter()
            .find(|line| line.process == EXTRACT)
            .expect("extract");
        assert_eq!(extract.target, Some(2.0));
        assert!(child.production_line.iter().any(|line| line.process == WELL));
    }

    #[test]
    fn market_records_the_founded_firm() {
        let mut market = Market::new(7);
        market.found_firm(2);
        assert!(market.firms.contains(&2));
    }
}
