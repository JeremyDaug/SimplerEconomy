use std::collections::{HashMap, HashSet};

use rand::RngCore;

use crate::game::factuals::Factuals;
use crate::game::good::TIME;
use crate::game::job::{Job, JobLine};
use crate::game::pop::{line_value, Pop};
use crate::game::process::{InputType, Process};

use super::Market;

/// What one founding attempt did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoundingEvent {
    /// The market was waiting. No roll.
    Rest,
    /// The attempt missed.
    Miss,
    /// The attempt hit and nobody could leave.
    Nobody,
    /// Households left `parent` to form pop `id`, running `process`.
    New { parent: usize, process: usize, id: usize },
    /// Households left `parent` and joined `dest`, to run `process`.
    Join { parent: usize, process: usize, dest: usize },
}

/// The founding attempt that just ran, and the chance it used.
///
/// A wait stores a chance of 0. [`Market::founding_day`] returns this.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoundingDay {
    /// The chance [`Market::found_pop`] compared with its draw.
    pub chance: f64,
    /// How that attempt came out.
    pub event: FoundingEvent,
}

/// One parent and process a hit might draw, and where those households go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoundingProspect {
    /// The pop who would lose the households.
    pub parent: usize,
    /// The process those households would run.
    pub process: usize,
    /// This pair's share of a hit.
    pub chance: f64,
    /// The member who already does this work. None means a new pop.
    pub joins: Option<usize>,
}

/// The next founding attempt, and every split a hit could draw.
///
/// [`Market::founding_outlook`] builds this.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundingOutlook {
    /// Chance of the next attempt, from [`attempt_chance`].
    pub chance: f64,
    /// Days left before another pop may be created.
    pub rest: u32,
    /// Parent and process pairs, highest [`FoundingProspect::chance`] first.
    pub prospects: Vec<FoundingProspect>,
}


impl Market {
    /// # Found Pop
    ///
    /// May split one member into a new pop, or send those households to one
    /// that already does that work.
    ///
    /// `pops` are the world's pops. `factuals` supplies processes, goods, and
    /// the founding knobs. `rng` draws the attempt and the choice. `next_id`
    /// is the id of a pop this call creates. Savings are still set aside.
    ///
    /// A market with [`Self::founding_rest`] days left waits, and does not
    /// roll. Otherwise it attempts at [`attempt_chance`].
    /// A miss leaves the chance climbing. A hit picks one member with at least
    /// [`MarketConfig::founding_min_households`](crate::game::config::MarketConfig::founding_min_households)
    /// households whose double headcount share can fund the process.
    /// [`MarketConfig::founding_households`](crate::game::config::MarketConfig::founding_households)
    /// households leave. They take twice that headcount's share of property
    /// and of savings, and not more than the parent has. That share has to
    /// cover [`FOUNDING_INPUT_RUNS`] of the job's required inputs and
    /// [`FOUNDING_DESIRE_DAYS`] of the new pop's basic desires. Stock and
    /// savings count together. Time is not an input the pile has to hold.
    ///
    /// The process is one loaded process with a non-time output.
    /// [`founding_weights`] gives profitable crafts most of the draw, and a
    /// process that makes a desired good nobody supplies weighs more. The
    /// parent keeps its job. The same species, culture, craft, and labor on
    /// another member is a join: the households and the share move there, and
    /// the chance keeps climbing. A process that would only copy the parent
    /// is not drawn. A new pop resets the wait to [`FOUNDING_REST_DAYS`] and
    /// the chance to 0. Its line starts at target 0. Returns the new pop's
    /// id, or nothing when no pop is created.
    pub fn found_pop(
        &mut self,
        pops: &mut HashMap<usize, Pop>,
        factuals: &Factuals,
        rng: &mut dyn RngCore,
        next_id: usize,
    ) -> Option<usize> {
        if self.founding_rest > 0 {
            self.founding_rest -= 1;
            self.founding_day = Some(FoundingDay { chance: 0.0, event: FoundingEvent::Rest });
            return None;
        }
        let chance = attempt_chance(self.founding_open, factuals.config.market.founding_drought_step);
        let hit = chance >= 1.0 || (chance > 0.0 && unit_draw(rng) < chance);
        if !hit {
            self.founding_open = self.founding_open.saturating_add(1);
            self.founding_day = Some(FoundingDay { chance, event: FoundingEvent::Miss });
            return None;
        }
        let Some(choice) = choose_founding(self, pops, factuals, rng) else {
            self.founding_open = self.founding_open.saturating_add(1);
            self.founding_day = Some(FoundingDay { chance, event: FoundingEvent::Nobody });
            return None;
        };
        let parent = choice.parent;
        let process_id = choice.process;
        let created = match choice.dest {
            Some(dest) => {
                move_founders(pops, FoundingDest::Join(dest), &choice);
                self.founding_day = Some(FoundingDay {
                    chance,
                    event: FoundingEvent::Join { parent, process: process_id, dest },
                });
                None
            }
            None => {
                let id = fresh_id(pops, next_id);
                let process = factuals
                    .get_process(process_id)
                    .expect("chosen process is loaded");
                move_founders(pops, FoundingDest::Create { id, process }, &choice);
                self.pops.insert(id);
                self.founding_day = Some(FoundingDay {
                    chance,
                    event: FoundingEvent::New { parent, process: process_id, id },
                });
                Some(id)
            }
        };
        if created.is_some() {
            self.founding_rest = FOUNDING_REST_DAYS;
            self.founding_open = 0;
        } else {
            self.founding_open = self.founding_open.saturating_add(1);
        }
        created
    }

    /// # Founding Day
    ///
    /// The last attempt this market ran.
    ///
    /// Returns that day, or nothing before the first attempt.
    pub fn founding_day(&self) -> Option<FoundingDay> {
        self.founding_day
    }

    /// # Founding Outlook
    ///
    /// The next attempt, and every split a hit could draw.
    ///
    /// `pops` are the world's pops. `factuals` supplies processes and the
    /// knobs. The chance is [`attempt_chance`] for the open days so far,
    /// including while [`Self::founding_rest`] is counting down. Each
    /// prospect is one parent and process from [`ready_founders`]. Its
    /// chance is the share [`choose_founding`] would give that pair.
    /// Prospects are highest chance first, then
    /// parent id, then process id. Returns that outlook.
    pub fn founding_outlook(&self, pops: &HashMap<usize, Pop>, factuals: &Factuals) -> FoundingOutlook {
        let chance = attempt_chance(self.founding_open, factuals.config.market.founding_drought_step);
        let ready = ready_founders(self, pops, factuals);
        let mut prospects = Vec::new();
        if !ready.is_empty() {
            let parent_share = 1.0 / ready.len() as f64;
            for founder in &ready {
                let total: f64 = founder.options.iter().map(|option| option.weight).sum();
                if total <= 0.0 {
                    continue;
                }
                for option in &founder.options {
                    prospects.push(FoundingProspect {
                        parent: founder.parent,
                        process: option.process,
                        chance: parent_share * option.weight / total,
                        joins: option.dest,
                    });
                }
            }
            prospects.sort_by(|left, right| {
                right.chance.total_cmp(&left.chance)
                    .then(left.parent.cmp(&right.parent))
                    .then(left.process.cmp(&right.process))
            });
        }
        FoundingOutlook { chance, rest: self.founding_rest, prospects }
    }
}

const FOUNDING_REST_DAYS: u32 = 5;
/// Required input runs the leaving share has to cover.
const FOUNDING_INPUT_RUNS: f64 = 3.0;
/// Days of the new pop's basic desires the leaving share has to cover.
const FOUNDING_DESIRE_DAYS: f64 = 1.5;
/// Share of the draw held by profitable crafts, before the unmet boost.
const FOUNDING_CRAFT_SHARE: f64 = 0.95;
/// Extra weight on a process that makes a desired good nobody supplies.
const FOUNDING_UNMET_WEIGHT: f64 = 20.0;

/// One process a parent can leave to do.
struct FoundingChoice {
    parent: usize,
    process: usize,
    dest: Option<usize>,
    fraction: f64,
    leaving: f64,
}

/// Where the leaving households go.
enum FoundingDest<'a> {
    /// A new pop with this id and process.
    Create { id: usize, process: &'a Process },
    /// An existing member.
    Join(usize),
}

/// Profit and gap flags for one process.
struct FoundingScore {
    profitable_craft: bool,
    per_time: f64,
    unmet: bool,
}

/// # Unit Draw
///
/// One draw in `0..1` from `rng`.
///
/// The draw is `next_u64` over `u64::MAX`. Returns that number.
fn unit_draw(rng: &mut dyn RngCore) -> f64 {
    (rng.next_u64() as f64) / (u64::MAX as f64)
}

/// # Fresh Id
///
/// The first id at or after `next_id` that `pops` does not use.
///
/// Returns that id.
fn fresh_id(pops: &HashMap<usize, Pop>, next_id: usize) -> usize {
    let mut id = next_id;
    while pops.contains_key(&id) {
        id = id.saturating_add(1);
    }
    id
}

/// # Attempt Chance
///
/// The chance a founding is attempted after `open` days that did not create a pop.
///
/// `step` is [`MarketConfig::founding_drought_step`](crate::game::config::MarketConfig::founding_drought_step).
/// The first open day is one step. Each recorded open day adds another.
/// The chance stops at 1. Returns that chance.
fn attempt_chance(open: u32, step: f64) -> f64 {
    debug_assert!(step.is_finite() && (0.0..=1.0).contains(&step));
    ((open as f64 + 1.0) * step).clamp(0.0, 1.0)
}

/// One parent who can fund at least one process.
struct ReadyParent {
    parent: usize,
    fraction: f64,
    options: Vec<FoundingOption>,
}

/// One process a ready parent can fund.
#[derive(Clone, Copy)]
struct FoundingOption {
    /// The process the leaving households would run.
    process: usize,
    /// Its [`founding_weights`] weight.
    weight: f64,
    /// The member who already does that work. None means a new pop.
    dest: Option<usize>,
}

/// # Choose Founding
///
/// The parent, process, and destination for one successful attempt.
///
/// `market` is the market. `pops` are the world's pops. `factuals` supplies
/// processes and the household knobs. `rng` picks the parent, then the
/// process. The candidates are [`ready_founders`]. Parents are equally
/// likely. Processes use their weights. Returns that choice, or nothing
/// when nobody can leave.
fn choose_founding(
    market: &Market,
    pops: &HashMap<usize, Pop>,
    factuals: &Factuals,
    rng: &mut dyn RngCore,
) -> Option<FoundingChoice> {
    let leaving = factuals.config.market.founding_households;
    let ready = ready_founders(market, pops, factuals);
    let index = weighted_index(rng, &vec![1.0; ready.len()])?;
    let founder = &ready[index];
    let picked = weighted_index(
        rng,
        &founder.options.iter().map(|option| option.weight).collect::<Vec<_>>(),
    )?;
    let option = founder.options[picked];
    Some(FoundingChoice {
        parent: founder.parent,
        process: option.process,
        dest: option.dest,
        fraction: founder.fraction,
        leaving,
    })
}

/// # Ready Founders
///
/// The parents who can leave, and the processes each one can fund.
///
/// `market` is the market. `pops` are the world's pops. `factuals` supplies
/// processes and the household knobs. A parent under the minimum size is
/// left out, and so is one that would keep fewer than one household. A
/// process the double share cannot fund is left out. A process that would
/// only copy the parent is left out. Each funded process is one
/// [`FoundingOption`]. Returns those parents in id order.
fn ready_founders(market: &Market, pops: &HashMap<usize, Pop>, factuals: &Factuals) -> Vec<ReadyParent> {
    let leaving = factuals.config.market.founding_households;
    let minimum = factuals.config.market.founding_min_households;
    debug_assert!(leaving.is_finite() && leaving > 0.0);
    debug_assert!(minimum.is_finite() && minimum > 0.0);
    let unmet = unmet_goods(market, pops, factuals);
    let mut processes: Vec<&Process> = factuals.processes.values().collect();
    processes.sort_by_key(|process| process.id);
    processes.retain(|process| has_product(process));
    let scores: Vec<FoundingScore> = processes
        .iter()
        .map(|process| founding_score(process, market, &unmet))
        .collect();
    let weights = founding_weights(&scores);
    let mut parents: Vec<usize> = market.pops.iter().copied().collect();
    parents.sort_unstable();
    let mut ready = Vec::new();
    for parent in parents {
        let Some(pop) = pops.get(&parent) else {
            continue;
        };
        let count = pop.demographics.household.count;
        if count + 1e-9 < minimum || count + 1e-9 < leaving + 1.0 {
            continue;
        }
        let fraction = (2.0 * leaving / count).min(1.0);
        let parent_key = work_key(pop, factuals);
        let mut options = Vec::new();
        for (process, weight) in processes.iter().zip(&weights) {
            if *weight <= 0.0 || !share_funds(pop, market, process, fraction, leaving) {
                continue;
            }
            let key = key_for(process);
            let dest = same_work(market, pops, factuals, pop, key, parent);
            if dest.is_none() && parent_key == key {
                continue;
            }
            options.push(FoundingOption { process: process.id, weight: *weight, dest });
        }
        if !options.is_empty() {
            ready.push(ReadyParent { parent, fraction, options });
        }
    }
    ready
}

/// # Share Funds
///
/// Whether `fraction` of `pop` covers the new job and the desire days.
///
/// `process` is the job. `leaving` households out of the parent's household
/// count are the new pop, so its basic-desire day is that share of
/// [`Pop::day_savings`].
/// Required destroyed or consumed inputs, other than time, have to cover
/// [`FOUNDING_INPUT_RUNS`]. The priced pile has to cover
/// [`FOUNDING_DESIRE_DAYS`] of that day. Property and savings both count.
/// Returns whether both hold.
fn share_funds(
    pop: &Pop, market: &Market, process: &Process, fraction: f64, leaving: f64,
) -> bool {
    let count = pop.demographics.household.count;
    if count <= 0.0 || fraction <= 0.0 {
        return false;
    }
    for input in &process.inputs {
        if input.good == TIME || input.is_optional() {
            continue;
        }
        if !matches!(input.input_type, InputType::Destroyed | InputType::Consumed) {
            continue;
        }
        let have = held(pop, input.good) * fraction;
        if have + 1e-9 < input.amount * FOUNDING_INPUT_RUNS {
            return false;
        }
    }
    let child_day = pop.day_savings(market) * (leaving / count);
    let need = FOUNDING_DESIRE_DAYS * child_day;
    let have = pop.wealth_per_household(Some(market)) * count * fraction;
    have + 1e-9 >= need
}

/// # Held
///
/// Units of `good` in property and in savings.
///
/// Returns that sum.
fn held(pop: &Pop, good: usize) -> f64 {
    let stock = pop.property.get(&good).map(|row| row.quantity).unwrap_or(0.0);
    let saved = pop.savings().get(&good).copied().unwrap_or(0.0);
    stock + saved
}

/// # Move Founders
///
/// Moves the leaving households and their double share off the parent.
///
/// `choice` is the split and names the parent. `dest` is the new pop or the
/// member they join.
/// Desire amounts and satisfaction scale with the household count, so the
/// met share stays. The parent's job is left as it is.
fn move_founders(
    pops: &mut HashMap<usize, Pop>, dest: FoundingDest<'_>, choice: &FoundingChoice,
) {
    let parent_id = choice.parent;
    let parent = pops
        .get(&parent_id)
        .unwrap_or_else(|| panic!("pop {parent_id} is not in the founding set"));
    let count = parent.demographics.household.count;
    let snapshot = parent.demographics.household;
    let (stock, saved) = parent.belonging_snapshot();
    let kept = (count - choice.leaving) / count;
    match dest {
        FoundingDest::Create { id, process } => {
            let mut child = new_pop_from(parent, id, choice.leaving, process);
            let mut parent = pops
                .remove(&parent_id)
                .unwrap_or_else(|| panic!("pop {parent_id} is not in the founding set"));
            scale_desire_size(&mut parent, kept);
            parent.demographics.household.count -= choice.leaving;
            parent.shift_belongings_to(&mut child, choice.fraction, &stock, &saved);
            pops.insert(parent_id, parent);
            pops.insert(id, child);
        }
        FoundingDest::Join(dest_id) => {
            let mut parent = pops
                .remove(&parent_id)
                .unwrap_or_else(|| panic!("pop {parent_id} is not in the founding set"));
            scale_desire_size(&mut parent, kept);
            parent.demographics.household.count -= choice.leaving;
            let dest = pops
                .get_mut(&dest_id)
                .unwrap_or_else(|| panic!("pop {dest_id} is not in the founding set"));
            let dest_count = dest.demographics.household.count;
            scale_desire_size(dest, (dest_count + choice.leaving) / dest_count);
            dest.demographics.household.absorb(&snapshot, choice.leaving);
            parent.shift_belongings_to(dest, choice.fraction, &stock, &saved);
            pops.insert(parent_id, parent);
        }
    }
}

/// # New Pop From
///
/// A pop of `leaving` households split from `parent` to run `process`.
///
/// Species, culture, subsistence, sentiment, and desires are copied. Desire
/// amounts and satisfaction are that household share of the parent. Property
/// and savings start empty. The process sits on the craft line, or on the
/// labor line when it is labor, at target 0. The other line is empty.
/// Returns that pop.
fn new_pop_from(parent: &Pop, id: usize, leaving: f64, process: &Process) -> Pop {
    let mut child = Pop::new(id);
    child.subsistence_rate = parent.subsistence_rate;
    child.subsistence_type = parent.subsistence_type;
    child.demographics = parent.demographics;
    child.demographics.household.count = leaving;
    child.pre_growth_population = child.demographics.total_population();
    child.sentiment = parent.sentiment;
    child.desires = parent.desires.clone();
    let count = parent.demographics.household.count;
    scale_desire_size(&mut child, leaving / count);
    child.job = job_for(process);
    child
}

/// # Job For
///
/// A job whose only line is `process` at target 0.
///
/// A labor process takes the second line. Any other process takes the
/// first. The empty line is process 0 at target 0. Returns that job.
fn job_for(process: &Process) -> Job {
    let line = JobLine::new(process.id, Some(0.0), Vec::new());
    let empty = JobLine::new(0, Some(0.0), Vec::new());
    if process.is_labor() {
        Job::new([empty, line])
    } else {
        Job::new([line, empty])
    }
}

/// # Scale Desire Size
///
/// Multiplies every desire amount and its satisfaction by `factor`.
///
/// The met share stays. A non-finite factor leaves the desires alone.
fn scale_desire_size(pop: &mut Pop, factor: f64) {
    if !factor.is_finite() || factor < 0.0 {
        return;
    }
    for tier in &mut pop.desires {
        for desire in tier.iter_mut() {
            desire.amount *= factor;
            desire.satisfaction *= factor;
        }
    }
}

/// # Has Product
///
/// Whether `process` outputs something other than time.
///
/// Returns that check.
fn has_product(process: &Process) -> bool {
    process
        .outputs
        .iter()
        .any(|row| row.good != TIME && row.amount > 0.0)
}

/// # Founding Score
///
/// The craft, profit, and gap flags of `process` on `market`.
///
/// A profitable craft is not labor and its value per time is above 0.
/// Unmet is a variable output in `unmet`. Returns that score.
fn founding_score(process: &Process, market: &Market, unmet: &HashSet<usize>) -> FoundingScore {
    let value = line_value(process, Some(market));
    FoundingScore {
        profitable_craft: !process.is_labor() && value.per_time > 0.0,
        per_time: value.per_time,
        unmet: process.variable_goods().into_iter().any(|good| unmet.contains(&good)),
    }
}

/// # Founding Weights
///
/// Draw weights for `scores`, in the same order.
///
/// Profitable crafts share [`FOUNDING_CRAFT_SHARE`] in proportion to value
/// per time. Every other score shares the rest equally. With no profitable
/// craft, every score shares equally. A score that fills an unmet desire
/// then weighs [`FOUNDING_UNMET_WEIGHT`] times that share. Returns those
/// weights.
fn founding_weights(scores: &[FoundingScore]) -> Vec<f64> {
    let craft_sum: f64 = scores
        .iter()
        .filter(|score| score.profitable_craft)
        .map(|score| score.per_time)
        .sum();
    let crafts = craft_sum > 0.0;
    let others = scores.iter().filter(|score| !score.profitable_craft).count();
    let count = scores.len();
    scores
        .iter()
        .map(|score| {
            let base = if score.profitable_craft && crafts {
                FOUNDING_CRAFT_SHARE * score.per_time / craft_sum
            } else if crafts && others > 0 {
                (1.0 - FOUNDING_CRAFT_SHARE) / others as f64
            } else if count > 0 {
                1.0 / count as f64
            } else {
                0.0
            };
            if score.unmet { base * FOUNDING_UNMET_WEIGHT } else { base }
        })
        .collect()
}

/// # Unmet Goods
///
/// Desired goods nobody in `market` supplies.
///
/// A desire target on a member with a positive amount is desired. A member's
/// job output supplies its variable goods. A posted offer supplies that
/// good. Returns the desired goods that are not supplied.
fn unmet_goods(market: &Market, pops: &HashMap<usize, Pop>, factuals: &Factuals) -> HashSet<usize> {
    let mut desired = HashSet::new();
    let mut supplied = HashSet::new();
    for &id in &market.pops {
        let Some(pop) = pops.get(&id) else {
            continue;
        };
        for tier in &pop.desires {
            for desire in tier {
                if desire.amount <= 0.0 {
                    continue;
                }
                for target in &desire.target {
                    desired.insert(target.good);
                }
            }
        }
        for line in &pop.job.lines {
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            for good in process.variable_goods() {
                supplied.insert(good);
            }
        }
    }
    for (&good, row) in &market.goods {
        if row.offered > 0.0 {
            supplied.insert(good);
        }
    }
    desired.retain(|good| !supplied.contains(good));
    desired
}

/// # Same Work
///
/// The lowest member, other than `parent`, with this species, culture, and work.
///
/// `key` is the craft and labor. A member under one household is left out.
/// Returns that pop, or nothing when the work is free.
fn same_work(
    market: &Market,
    pops: &HashMap<usize, Pop>,
    factuals: &Factuals,
    parent: &Pop,
    key: (Option<usize>, Option<usize>),
    parent_id: usize,
) -> Option<usize> {
    let mut ids: Vec<usize> = market.pops.iter().copied().filter(|id| *id != parent_id).collect();
    ids.sort_unstable();
    for id in ids {
        let Some(pop) = pops.get(&id) else {
            continue;
        };
        if pop.demographics.household.count < 1.0 {
            continue;
        }
        if pop.demographics.species != parent.demographics.species
            || pop.demographics.culture != parent.demographics.culture
        {
            continue;
        }
        if work_key(pop, factuals) == key {
            return Some(id);
        }
    }
    None
}

/// # Work Key
///
/// The craft process and the labor process on `pop`.
///
/// A line whose process is not loaded is empty. The first labor line is the
/// labor. The first other line is the craft. Returns that pair.
fn work_key(pop: &Pop, factuals: &Factuals) -> (Option<usize>, Option<usize>) {
    let mut craft = None;
    let mut labor = None;
    for line in &pop.job.lines {
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        if process.is_labor() {
            if labor.is_none() {
                labor = Some(process.id);
            }
        } else if craft.is_none() {
            craft = Some(process.id);
        }
    }
    (craft, labor)
}

/// # Key For
///
/// The craft and labor pair a new pop running `process` would have.
///
/// Labor fills the labor side. Any other process fills the craft side.
/// Returns that pair.
fn key_for(process: &Process) -> (Option<usize>, Option<usize>) {
    if process.is_labor() {
        (None, Some(process.id))
    } else {
        (Some(process.id), None)
    }
}

/// # Weighted Index
///
/// An index into `weights`, drawn in proportion to the weight.
///
/// A non-positive weight is skipped. `rng` draws the roll. The last
/// positive weight catches a roll that lands on the end. Returns that
/// index, or nothing when every weight is empty.
fn weighted_index(rng: &mut dyn RngCore, weights: &[f64]) -> Option<usize> {
    let total: f64 = weights.iter().copied().filter(|weight| *weight > 0.0).sum();
    if total <= 0.0 {
        return None;
    }
    let mut cursor = unit_draw(rng) * total;
    let mut last = None;
    for (index, weight) in weights.iter().copied().enumerate() {
        if weight <= 0.0 {
            continue;
        }
        last = Some(index);
        cursor -= weight;
        if cursor <= 0.0 {
            return Some(index);
        }
    }
    last
}

#[cfg(test)]
mod founding_should {
    use std::collections::{HashMap, HashSet};

    use rand::RngCore;

    use super::{founding_score, founding_weights, unmet_goods, FoundingDay, FoundingEvent, FoundingScore};
    use crate::game::market::{Market, MarketGood};
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{Desire, DesireTarget, DesireTargetType};
    use crate::game::factuals::Factuals;
    use crate::game::good::TIME;
    use crate::game::household::Household;
    use crate::game::job::{Job, JobLine};
    use crate::game::pop::{Pop, SubsistenceType};
    use crate::game::pop_property::PopPRow;
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput, ProcessTag};
    use crate::game::scalingfactor::ScalingFactor;
    use crate::game::sentiment::Sentiment;

    const GRAIN: usize = 1;
    const BREAD: usize = 2;
    const WOOD: usize = 3;
    const WATER: usize = 4;

    fn near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{actual} != {expected}"
        );
    }

    /// Every draw is the same `u64`.
    struct FixedRng(u64);

    impl RngCore for FixedRng {
        fn next_u32(&mut self) -> u32 {
            self.next_u64() as u32
        }

        fn next_u64(&mut self) -> u64 {
            self.0
        }

        fn fill_bytes(&mut self, dest: &mut [u8]) {
            for chunk in dest.chunks_mut(8) {
                let bytes = self.next_u64().to_le_bytes();
                chunk.copy_from_slice(&bytes[..chunk.len()]);
            }
        }

        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
            self.fill_bytes(dest);
            Ok(())
        }
    }

    /// A draw here means the founding rolled when it should not have.
    struct PanicRng;

    impl RngCore for PanicRng {
        fn next_u32(&mut self) -> u32 {
            panic!("founding drew");
        }

        fn next_u64(&mut self) -> u64 {
            panic!("founding drew");
        }

        fn fill_bytes(&mut self, _dest: &mut [u8]) {
            panic!("founding drew");
        }

        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
            self.fill_bytes(dest);
            Ok(())
        }
    }

    fn basic(amount: f64, satisfaction: f64, good: usize) -> Desire {
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 0,
            target: vec![DesireTarget::new(good, DesireTargetType::Consume, 1.0)],
            amount,
            satisfaction,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        }
    }

    fn score(profitable_craft: bool, per_time: f64, unmet: bool) -> FoundingScore {
        FoundingScore {
            profitable_craft,
            per_time,
            unmet,
        }
    }

    /// Four bread from one wood and one time. Capital and an optional input sit on it.
    fn bake(id: usize, good: usize) -> Process {
        Process::new(id, format!("bake {id}"), 0)
            .with_input(ProcessInput::new(TIME, 1.0, false, InputType::Destroyed, false))
            .with_input(ProcessInput::new(WOOD, 1.0, false, InputType::Destroyed, false))
            .with_input(ProcessInput::new(99, 100.0, false, InputType::Capital, false))
            .with_input(ProcessInput::new(98, 50.0, false, InputType::Destroyed, true))
            .with_output(ProcessOutput::new(good, 4.0, false))
    }

    fn labor(id: usize, wood: f64) -> Process {
        let mut process = Process::new(id, "haul", 0)
            .with_input(ProcessInput::new(TIME, 1.0, false, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(WATER, 10.0, false));
        if wood > 0.0 {
            process = process.with_input(ProcessInput::new(WOOD, wood, false, InputType::Destroyed, false));
        }
        process.process_tags.push(ProcessTag::Labor);
        process
    }

    fn time_only(id: usize) -> Process {
        Process::new(id, "gather", 0)
            .with_input(ProcessInput::new(TIME, 1.0, false, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(BREAD, 4.0, false))
    }

    fn world(step: f64, processes: Vec<Process>) -> Factuals {
        let mut factuals = Factuals::new();
        factuals.config.market.founding_drought_step = step;
        for process in processes {
            factuals.processes.insert(process.id, process);
        }
        factuals
    }

    fn member(count: f64, wood: f64, grain: f64, desire: f64) -> Pop {
        let mut pop = Pop::new(1);
        pop.demographics.household = Household::with_count(count);
        pop.demographics.species = 3;
        pop.demographics.culture = 4;
        pop.subsistence_rate = 0.4;
        pop.subsistence_type = SubsistenceType::Nomadic;
        pop.sentiment = Sentiment::from_parts(1.0, 0.0, 0.0, 0.0, 0.0);
        pop.property.insert(WOOD, PopPRow::new(wood));
        pop.property.insert(GRAIN, PopPRow::new(grain));
        pop.desires[0].push(basic(desire, desire, GRAIN));
        pop.job = Job::new([
            JobLine::new(7, Some(3.0), Vec::new()),
            JobLine::new(0, None, Vec::new()),
        ]);
        pop
    }

    fn market_of(ids: &[usize]) -> Market {
        let mut market = Market::new(1);
        market.pops.extend(ids);
        market
    }

    fn qty(pop: &Pop, good: usize) -> f64 {
        pop.property.get(&good).map(|row| row.quantity).unwrap_or(0.0)
    }

    fn found(market: &mut Market, pops: &mut HashMap<usize, Pop>, factuals: &Factuals, rng: &mut dyn RngCore) -> Option<usize> {
        market.found_pop(pops, factuals, rng, 2)
    }

    #[test]
    fn weights_follow_profit_then_the_unmet_gap() {
        // Crafts at 1 and 3 share 0.95. Two others share 0.05, and one of those is unmet.
        let weights = founding_weights(&[
            score(true, 1.0, false),
            score(true, 3.0, false),
            score(false, 0.0, true),
            score(false, 4.0, false),
        ]);
        near(weights[0], 0.2375);
        near(weights[1], 0.7125);
        near(weights[2], 0.5);
        near(weights[3], 0.025);

        // Equal crafts. The unmet one keeps its share and then weighs 20 times.
        let weights = founding_weights(&[score(true, 2.0, false), score(true, 2.0, true)]);
        near(weights[0], 0.475);
        near(weights[1], 0.475 * 20.0);

        // No profitable craft, so the base is equal and the unmet boost still applies.
        let weights = founding_weights(&[
            score(false, 1.0, false),
            score(false, 9.0, true),
            score(false, 0.0, false),
        ]);
        near(weights[0], 1.0 / 3.0);
        near(weights[1], 20.0 / 3.0);
        near(weights[2], 1.0 / 3.0);

        // Labor's value per time stays out of the craft share.
        let market = Market::new(1);
        let craft = founding_score(&bake(1, BREAD), &market, &HashSet::new());
        let haul = founding_score(&labor(9, 100.0), &market, &HashSet::new());
        assert!(craft.profitable_craft);
        near(craft.per_time, 3.0);
        assert!(!haul.profitable_craft);
        near(haul.per_time, 10.0);
        let weights = founding_weights(&[craft, haul]);
        near(weights[0], 0.95);
        near(weights[1], 0.05);
    }

    #[test]
    fn an_unmet_good_is_desired_and_not_made_or_offered() {
        let water = Process::new(4, "draw", 0)
            .with_output(ProcessOutput::new(WATER, 1.0, false))
            .with_output(ProcessOutput::new(BREAD, 1.0, true));
        let factuals = world(1.0, vec![water]);
        let mut market = market_of(&[1]);
        market.goods.insert(GRAIN, MarketGood { offered: 3.0, ..MarketGood::new() });
        market.goods.insert(BREAD, MarketGood { offered: 0.0, ..MarketGood::new() });
        let mut pop = member(25.0, 0.0, 0.0, 0.0);
        pop.desires[0].clear();
        pop.job = Job::new([
            JobLine::new(4, Some(1.0), Vec::new()),
            JobLine::new(0, None, Vec::new()),
        ]);
        pop.property.insert(BREAD, PopPRow::new(50.0));
        pop.desires[0].push(basic(1.0, 0.0, BREAD));
        pop.desires[1].push(basic(1.0, 0.0, WATER));
        pop.desires[1].push(basic(1.0, 0.0, GRAIN));
        pop.desires[2].push(basic(0.0, 0.0, 8));
        let mut pops = HashMap::new();
        pops.insert(1, pop);
        assert_eq!(unmet_goods(&market, &pops, &factuals), HashSet::from([BREAD]));
    }

    #[test]
    fn an_unmet_craft_wins_the_draw_the_equal_craft_would_have_taken() {
        // A quarter of two equal weights lands on the first. The second weighs 20 times, so it wins.
        let factuals = world(1.0, vec![bake(1, BREAD), bake(2, WATER)]);
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(25.0, 100.0, 100.0, 10.0))]);
        pops.get_mut(&1).unwrap().desires[0][0] = basic(10.0, 10.0, WATER);
        let outlook = market.founding_outlook(&pops, &factuals);
        assert_eq!(outlook.prospects[0].process, 2);
        assert_eq!(outlook.prospects[1].process, 1);
        near(outlook.prospects[0].chance, 9.5 / 9.975);
        near(outlook.prospects[1].chance, 0.475 / 9.975);
        let mut rng = FixedRng(1 << 62);
        let id = found(&mut market, &mut pops, &factuals, &mut rng).unwrap();
        assert_eq!(id, 2);
        assert_eq!(pops[&2].job.lines[0].process, 2);
        assert_eq!(market.founding_rest, 5);
        assert_eq!(market.founding_open, 0);
    }

    #[test]
    fn a_rich_pop_splits_off_five_households() {
        let factuals = world(1.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(25.0, 100.0, 100.0, 10.0))]);
        let before = pops[&1].pre_growth_population;
        let outlook = market.founding_outlook(&pops, &factuals);
        near(outlook.chance, 1.0);
        assert_eq!(outlook.rest, 0);
        assert_eq!(outlook.prospects.len(), 1);
        near(outlook.prospects[0].chance, 1.0);
        assert_eq!(outlook.prospects[0].joins, None);
        let mut rng = FixedRng(0);
        let id = found(&mut market, &mut pops, &factuals, &mut rng).unwrap();

        assert_eq!(id, 2);
        assert_eq!(
            market.founding_day(),
            Some(FoundingDay {
                chance: 1.0,
                event: FoundingEvent::New { parent: 1, process: 1, id: 2 },
            }),
        );
        assert_eq!(market.founding_rest, 5);
        assert_eq!(market.founding_open, 0);
        assert!(market.pops.contains(&1) && market.pops.contains(&2));
        let parent = &pops[&1];
        let child = &pops[&2];
        near(parent.demographics.household.count, 20.0);
        near(child.demographics.household.count, 5.0);
        near(qty(parent, WOOD), 60.0);
        near(qty(parent, GRAIN), 60.0);
        near(qty(child, WOOD), 40.0);
        near(qty(child, GRAIN), 40.0);
        near(parent.desires[0][0].amount, 8.0);
        near(parent.desires[0][0].satisfaction, 8.0);
        near(child.desires[0][0].amount, 2.0);
        near(child.desires[0][0].satisfaction, 2.0);
        assert_eq!(parent.job.lines[0].process, 7);
        assert_eq!(parent.job.lines[0].target, Some(3.0));
        assert_eq!(parent.job.lines[1].process, 0);
        assert_eq!(parent.job.lines[1].target, None);
        assert_eq!(child.job.lines[0].process, 1);
        assert_eq!(child.job.lines[0].target, Some(0.0));
        assert!(child.job.lines[0].inputs.is_empty());
        assert_eq!(child.job.lines[1].process, 0);
        assert_eq!(child.job.lines[1].target, Some(0.0));
        assert_eq!(child.demographics.species, 3);
        assert_eq!(child.demographics.culture, 4);
        near(child.subsistence_rate, 0.4);
        assert!(matches!(child.subsistence_type, SubsistenceType::Nomadic));
        near(child.sentiment.happiness(), 1.0);
        near(child.pre_growth_population, child.demographics.total_population());
        assert_eq!(parent.pre_growth_population, before);
        assert!(parent.savings().is_empty() && child.savings().is_empty());
        assert_eq!(parent.migration, 0.0);
        assert_eq!(child.migration, 0.0);
    }

    #[test]
    fn labor_takes_the_second_line() {
        let factuals = world(1.0, vec![labor(5, 0.0)]);
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(25.0, 100.0, 100.0, 10.0))]);
        let mut rng = FixedRng(0);
        let id = found(&mut market, &mut pops, &factuals, &mut rng).unwrap();
        let child = &pops[&id];
        assert_eq!(child.job.lines[0].process, 0);
        assert_eq!(child.job.lines[0].target, Some(0.0));
        assert_eq!(child.job.lines[1].process, 5);
        assert_eq!(child.job.lines[1].target, Some(0.0));
    }

    #[test]
    fn the_rest_blocks_five_days_and_the_next_open_day_joins() {
        let factuals = world(1.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(40.0, 1000.0, 1000.0, 10.0))]);
        let mut rng = FixedRng(0);
        assert_eq!(found(&mut market, &mut pops, &factuals, &mut rng), Some(2));
        assert_eq!(market.founding_rest, 5);
        for left in (0..5).rev() {
            let mut rng = PanicRng;
            assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
            assert_eq!(market.founding_rest, left);
            assert_eq!(market.founding_open, 0);
            assert_eq!(pops.len(), 2);
        }
        // The new pop already bakes, so the next open day joins it and the chance keeps climbing.
        let mut rng = FixedRng(0);
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(pops.len(), 2);
        assert_eq!(market.founding_rest, 0);
        assert_eq!(market.founding_open, 1);
        near(pops[&1].demographics.household.count, 30.0);
        near(pops[&2].demographics.household.count, 10.0);
        // 40 then 35. The leaving share is 0.25, then 10/35, and the second share joins the child.
        near(qty(&pops[&2], GRAIN), 250.0 + 750.0 * 10.0 / 35.0);
        near(qty(&pops[&2], WOOD), 250.0 + 750.0 * 10.0 / 35.0);
        near(qty(&pops[&1], GRAIN), 750.0 * 25.0 / 35.0);
        near(pops[&2].desires[0][0].amount, 2.5);
        near(pops[&2].desires[0][0].satisfaction, 2.5);
        near(pops[&1].desires[0][0].amount, 7.5);
        near(pops[&1].desires[0][0].satisfaction, 7.5);
        assert_eq!(pops[&1].job.lines[0].process, 7);
        assert_eq!(pops[&2].job.lines[0].process, 1);
    }

    #[test]
    fn a_short_input_pile_does_not_found() {
        let factuals = world(1.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(25.0, 5.0, 100.0, 10.0))]);
        let mut rng = PanicRng;
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(market.founding_open, 1);
        assert_eq!(market.founding_rest, 0);
        near(pops[&1].demographics.household.count, 25.0);
        near(qty(&pops[&1], WOOD), 5.0);
        assert_eq!(pops.len(), 1);
    }

    #[test]
    fn a_short_desire_pile_does_not_found() {
        let factuals = world(1.0, vec![time_only(1)]);
        let mut market = market_of(&[1]);
        // Child day is 20. The 0.4 share of 70 is 28, under the 30 the gate asks.
        let mut pops = HashMap::from([(1, member(25.0, 0.0, 70.0, 100.0))]);
        let mut rng = PanicRng;
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(market.founding_open, 1);
        near(pops[&1].demographics.household.count, 25.0);
        near(qty(&pops[&1], GRAIN), 70.0);
    }

    #[test]
    fn the_same_work_joins_the_lowest_member() {
        let factuals = world(1.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1, 2, 8]);
        let mut parent = member(25.0, 100.0, 100.0, 10.0);
        parent.job = Job::none();
        let mut early = member(10.0, 0.0, 0.0, 10.0);
        early.id = 2;
        early.desires[0][0].satisfaction = 4.0;
        early.job = Job::new([
            JobLine::new(1, Some(1.0), Vec::new()),
            JobLine::new(0, None, Vec::new()),
        ]);
        let mut later = early.clone();
        later.id = 8;
        later.desires[0][0].satisfaction = 1.0;
        let mut pops = HashMap::from([(1, parent), (2, early), (8, later)]);
        let outlook = market.founding_outlook(&pops, &factuals);
        assert_eq!(outlook.prospects.len(), 1);
        assert_eq!(outlook.prospects[0].joins, Some(2));
        let mut rng = FixedRng(0);
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(
            market.founding_day(),
            Some(FoundingDay {
                chance: 1.0,
                event: FoundingEvent::Join { parent: 1, process: 1, dest: 2 },
            }),
        );
        assert_eq!(pops.len(), 3);
        assert_eq!(market.pops.len(), 3);
        assert_eq!(market.founding_rest, 0);
        assert_eq!(market.founding_open, 1);
        near(pops[&1].demographics.household.count, 20.0);
        near(pops[&2].demographics.household.count, 15.0);
        near(pops[&8].demographics.household.count, 10.0);
        near(qty(&pops[&1], GRAIN), 60.0);
        near(qty(&pops[&1], WOOD), 60.0);
        near(qty(&pops[&2], GRAIN), 40.0);
        near(qty(&pops[&2], WOOD), 40.0);
        near(qty(&pops[&8], GRAIN), 0.0);
        near(pops[&1].desires[0][0].amount, 8.0);
        near(pops[&1].desires[0][0].satisfaction, 8.0);
        near(pops[&2].desires[0][0].amount, 15.0);
        near(pops[&2].desires[0][0].satisfaction, 6.0);
        near(pops[&8].desires[0][0].satisfaction, 1.0);
        assert_eq!(pops[&2].job.lines[0].process, 1);
        assert_eq!(pops[&2].job.lines[0].target, Some(1.0));
        assert_eq!(pops[&1].job.lines[0].process, 0);
    }

    #[test]
    fn a_different_culture_is_a_new_pop() {
        let factuals = world(1.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1, 2]);
        let mut parent = member(25.0, 100.0, 100.0, 10.0);
        parent.job = Job::none();
        let mut other = member(10.0, 0.0, 0.0, 10.0);
        other.id = 2;
        other.demographics.culture = 9;
        other.job = Job::new([
            JobLine::new(1, Some(1.0), Vec::new()),
            JobLine::new(0, None, Vec::new()),
        ]);
        let mut pops = HashMap::from([(1, parent), (2, other)]);
        let mut rng = FixedRng(0);
        assert_eq!(found(&mut market, &mut pops, &factuals, &mut rng), Some(3));
        near(pops[&2].demographics.household.count, 10.0);
        near(qty(&pops[&2], GRAIN), 0.0);
        assert_eq!(pops[&3].demographics.culture, 4);
        assert_eq!(pops[&3].job.lines[0].process, 1);
        near(pops[&1].demographics.household.count, 20.0);
    }

    #[test]
    fn a_pop_under_the_minimum_does_not_found() {
        let factuals = world(1.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(24.0, 1000.0, 1000.0, 10.0))]);
        let mut rng = PanicRng;
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(market.founding_open, 1);
        near(pops[&1].demographics.household.count, 24.0);
        assert_eq!(pops.len(), 1);
    }

    #[test]
    fn a_parent_keeps_at_least_one_household() {
        let mut factuals = world(1.0, vec![bake(1, BREAD)]);
        factuals.config.market.founding_min_households = 1.0;
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(5.5, 100.0, 100.0, 10.0))]);
        let mut rng = PanicRng;
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(market.founding_open, 1);
        near(pops[&1].demographics.household.count, 5.5);

        pops.get_mut(&1).unwrap().demographics.household.count = 6.0;
        market.founding_open = 0;
        let mut rng = FixedRng(0);
        assert_eq!(found(&mut market, &mut pops, &factuals, &mut rng), Some(2));
        near(pops[&1].demographics.household.count, 1.0);
        near(pops[&2].demographics.household.count, 5.0);
        near(qty(&pops[&2], GRAIN), 100.0);
        near(qty(&pops[&1], GRAIN), 0.0);
    }

    #[test]
    fn a_zero_step_misses_without_a_draw() {
        let factuals = world(0.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(25.0, 100.0, 100.0, 10.0))]);
        let mut rng = PanicRng;
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(market.founding_open, 1);
        assert_eq!(market.founding_rest, 0);
        assert_eq!(
            market.founding_day(),
            Some(FoundingDay { chance: 0.0, event: FoundingEvent::Miss }),
        );
        near(market.founding_outlook(&pops, &factuals).chance, 0.0);
        near(pops[&1].demographics.household.count, 25.0);
    }

    #[test]
    fn a_high_draw_misses_and_the_next_certain_day_founds() {
        let factuals = world(0.5, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(25.0, 100.0, 100.0, 10.0))]);
        let mut rng = FixedRng(u64::MAX);
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(market.founding_open, 1);
        assert_eq!(found(&mut market, &mut pops, &factuals, &mut rng), Some(2));
        assert_eq!(market.founding_rest, 5);
        assert_eq!(market.founding_open, 0);
    }

    #[test]
    fn copying_the_parent_is_not_a_founding() {
        let factuals = world(1.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1]);
        let mut pop = member(25.0, 100.0, 100.0, 10.0);
        pop.job = Job::new([
            JobLine::new(1, Some(2.0), Vec::new()),
            JobLine::new(0, None, Vec::new()),
        ]);
        let mut pops = HashMap::from([(1, pop)]);
        let mut rng = PanicRng;
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(market.founding_open, 1);
        near(pops[&1].demographics.household.count, 25.0);
        assert_eq!(pops[&1].job.lines[0].target, Some(2.0));
        near(qty(&pops[&1], WOOD), 100.0);
        assert_eq!(pops.len(), 1);
    }
}
