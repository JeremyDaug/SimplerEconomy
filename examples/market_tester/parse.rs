use simpler_economy::game::actor::Actor;
use simpler_economy::game::config::MarketPriorityConfig;
use simpler_economy::game::marketorder::MarketOrder;

use super::*;

pub(crate) struct Tokens<'a> {
    rest: &'a [&'a str],
}

impl<'a> Tokens<'a> {
    pub(crate) fn new(rest: &'a [&'a str]) -> Self {
        Self { rest }
    }

    fn next(&mut self) -> Option<&'a str> {
        let (first, rest) = self.rest.split_first()?;
        self.rest = rest;
        Some(*first)
    }

    fn expect_empty(&self) -> Result<(), String> {
        if self.rest.is_empty() {
            Ok(())
        } else {
            Err(format!("unexpected extra tokens: {}", self.rest.join(" ")))
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rest.is_empty()
    }
}

pub(crate) fn parse_day_count(rest: &[&str]) -> Result<u32, String> {
    if rest.is_empty() {
        return Ok(1);
    }
    if rest.len() != 1 {
        return Err("usage: day [count]".into());
    }
    let n: u32 = rest[0]
        .parse()
        .map_err(|_| format!("not a day count: {}", rest[0]))?;
    if n == 0 {
        return Err("day count must be >= 1".into());
    }
    if n > 365 {
        return Err("day count max is 365".into());
    }
    Ok(n)
}

pub(crate) fn parse_seed(rest: &[&str]) -> Result<u64, String> {
    if rest.len() != 1 {
        return Err("usage: seed <u64>".into());
    }
    rest[0]
        .parse::<u64>()
        .map_err(|_| format!("not a u64: {}", rest[0]))
}

/// request / offer: actor good amount [priority]
pub(crate) fn parse_simple_order(
    rest: &[&str],
    is_buy: bool,
    cfg: &MarketPriorityConfig,
) -> Result<MarketOrder, String> {
    let kind = if is_buy { "request" } else { "offer" };
    let usage = format!("usage: {kind} <actor> <good> <amount> [priority]");
    let mut tok = Tokens::new(rest);
    let actor = parse_actor(&mut tok).map_err(|e| format!("{e}  {usage}"))?;
    let good = parse_good(&mut tok).map_err(|e| format!("{e}  {usage}"))?;
    let amount = parse_positive_amount(tok.next().ok_or_else(|| usage.clone())?)?;
    let priority = match tok.next() {
        Some(raw) => parse_f64(raw, "priority")?,
        None => {
            if is_buy {
                default_buy_priority(actor, cfg)
            } else {
                compose_sell_priority_with(
                    default_buy_priority(actor, cfg),
                    amount,
                    0.0,
                    cfg.sell_actor_priority_floor,
                    cfg.successful_sell_bonus,
                )
            }
        }
    };
    tok.expect_empty()?;
    check_priority(actor, priority, is_buy, cfg)?;
    if is_buy {
        Ok(MarketOrder::request_order(actor, good, amount, priority))
    } else {
        Ok(MarketOrder::offer_order(actor, good, -amount, priority))
    }
}

/// buy / sell: actor good amount amv other-good other-amount [priority]
pub(crate) fn parse_exchange_order(
    rest: &[&str],
    is_buy: bool,
    cfg: &MarketPriorityConfig,
) -> Result<MarketOrder, String> {
    let kind = if is_buy { "buy" } else { "sell" };
    let usage = format!(
        "usage: {kind} <actor> <good> <amount> <amv> <other-good> <other-amount> [priority]"
    );
    let mut tok = Tokens::new(rest);
    let actor = parse_actor(&mut tok).map_err(|e| format!("{e}  {usage}"))?;
    let good = parse_good(&mut tok).map_err(|e| format!("{e}  {usage}"))?;
    let amount = parse_positive_amount(tok.next().ok_or_else(|| usage.clone())?)?;
    let amv = parse_f64(tok.next().ok_or_else(|| usage.clone())?, "amv")?;
    let other_good = parse_good(&mut tok).map_err(|e| format!("{e}  {usage}"))?;
    let other_amount = parse_positive_amount(tok.next().ok_or_else(|| usage.clone())?)?;
    let priority = match tok.next() {
        Some(raw) => parse_f64(raw, "priority")?,
        None => {
            if is_buy {
                default_buy_priority(actor, cfg)
            } else {
                compose_sell_priority_with(
                    default_buy_priority(actor, cfg),
                    amount,
                    0.0,
                    cfg.sell_actor_priority_floor,
                    cfg.successful_sell_bonus,
                )
            }
        }
    };
    tok.expect_empty()?;
    check_priority(actor, priority, is_buy, cfg)?;
    if is_buy {
        Ok(MarketOrder::buy_order(
            actor,
            good,
            amount,
            amv,
            other_good,
            -other_amount,
            priority,
        ))
    } else {
        Ok(MarketOrder::sell_order(
            actor,
            good,
            -amount,
            amv,
            other_good,
            other_amount,
            priority,
        ))
    }
}

pub(crate) fn parse_actor(tok: &mut Tokens<'_>) -> Result<Actor, String> {
    let first = tok
        .next()
        .ok_or_else(|| "expected actor (prefab name or kind id)".to_string())?;
    let key = first.to_ascii_lowercase();
    if let Some(named) = PREFAB_ACTORS.iter().find(|a| a.name == key) {
        return Ok(named.actor);
    }
    if let Some(id) = parse_glued_pop_id(&key) {
        return Ok(Actor::Pop(id));
    }
    let id_tok = tok
        .next()
        .ok_or_else(|| format!("unknown actor '{first}' (need a prefab name, or kind plus id)"))?;
    parse_actor_kind_id(&key, id_tok)

}

fn parse_glued_pop_id(key: &str) -> Option<usize> {
    let rest = key.strip_prefix("pop")?;
    if rest.is_empty() {
        return None;
    }
    rest.parse().ok()
}

pub(crate) fn parse_actor_kind_id(kind: &str, id: &str) -> Result<Actor, String> {
    let id = parse_usize(id, "actor id")?;
    match kind {
        "pop" | "p" => Ok(Actor::Pop(id)),
        "firm" | "f" => Ok(Actor::Firm(id)),
        "inst" | "institution" | "i" => Ok(Actor::Institution(id)),
        "state" | "s" => Ok(Actor::State(id)),
        other => Err(format!(
            "unknown actor kind '{other}' (pop/firm/inst/state or p/f/i/s)"
        )),
    }
}

pub(crate) fn parse_good(tok: &mut Tokens<'_>) -> Result<usize, String> {
    let raw = tok
        .next()
        .ok_or_else(|| "expected good (prefab name or id)".to_string())?;
    let key = raw.to_ascii_lowercase();
    if let Some(good) = PREFAB_GOODS.iter().find(|g| g.name == key) {
        return Ok(good.id);
    }
    parse_usize(raw, "good").map_err(|_| format!("unknown good '{raw}' (prefab name or id)"))
}

pub(crate) fn parse_usize(raw: &str, name: &str) -> Result<usize, String> {
    raw.parse::<usize>()
        .map_err(|_| format!("{name} must be a usize, got '{raw}'"))
}

pub(crate) fn parse_f64(raw: &str, name: &str) -> Result<f64, String> {
    let v: f64 = raw
        .parse()
        .map_err(|_| format!("{name} must be a number, got '{raw}'"))?;
    if !v.is_finite() {
        return Err(format!("{name} must be finite"));
    }
    Ok(v)
}

pub(crate) fn parse_positive_amount(raw: &str) -> Result<f64, String> {
    let v = parse_f64(raw, "amount")?.abs();
    if v == 0.0 {
        return Err("amount must be non-zero".into());
    }
    Ok(v)
}

pub(crate) fn default_buy_priority(actor: Actor, cfg: &MarketPriorityConfig) -> f64 {
    match actor {
        Actor::Pop(_) => cfg.pop_start,
        Actor::Firm(_) => cfg.firm_producer(),
        Actor::Institution(_) => cfg.institution_before_firms,
        Actor::State(_) => cfg.state_first,
    }
}

pub(crate) fn check_priority(
    actor: Actor,
    priority: f64,
    is_buy: bool,
    cfg: &MarketPriorityConfig,
) -> Result<(), String> {
    if !priority.is_finite() {
        return Err("priority must be finite".into());
    }
    if !is_buy {
        if priority <= 0.0 {
            return Err("sell/offer priority must be > 0".into());
        }
        return Ok(());
    }
    match actor {
        Actor::Pop(_) => {
            if !(cfg.pop_start..cfg.pop_end).contains(&priority) {
                return Err(format!(
                    "pop buy priority must be in [{}, {})",
                    cfg.pop_start, cfg.pop_end
                ));
            }
        }
        Actor::Firm(_) => {
            if !(cfg.firm_merchant_start..cfg.firm_producer_end).contains(&priority) {
                return Err(format!(
                    "firm buy priority must be in [{}, {})",
                    cfg.firm_merchant_start, cfg.firm_producer_end
                ));
            }
        }
        Actor::Institution(_) | Actor::State(_) => {}
    }
    Ok(())
}

pub(crate) fn add_buy(session: &mut Session, order: MarketOrder) -> String {
    let i = session
        .buys
        .partition_point(|o| o.priority <= order.priority);
    session.buys.insert(i, order);
    format!("buy [{i}] {}", fmt_order(session, &session.buys[i]))
}

pub(crate) fn add_sell(session: &mut Session, order: MarketOrder) -> String {
    let i = session.sells.partition_point(|o| o.target <= order.target);
    session.sells.insert(i, order);
    format!("sell [{i}] {}", fmt_order(session, &session.sells[i]))
}

pub(crate) fn drop_order(session: &mut Session, rest: &[&str]) -> Result<String, String> {
    if rest.len() != 2 {
        return Err("usage: drop buy <i>  or  drop sell <i>".into());
    }
    let side = rest[0].to_ascii_lowercase();
    let idx = parse_usize(rest[1], "index")?;
    match side.as_str() {
        "buy" | "b" => {
            if idx >= session.buys.len() {
                return Err(format!("no buy [{idx}]"));
            }
            let removed = session.buys.remove(idx);
            Ok(format!("dropped buy [{idx}] {}", fmt_order(session, &removed)))
        }
        "sell" | "s" => {
            if idx >= session.sells.len() {
                return Err(format!("no sell [{idx}]"));
            }
            let removed = session.sells.remove(idx);
            Ok(format!("dropped sell [{idx}] {}", fmt_order(session, &removed)))
        }
        other => Err(format!("drop side must be buy or sell, got '{other}'")),
    }
}

