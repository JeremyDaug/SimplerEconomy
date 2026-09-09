use std::collections::HashMap;

use simpler_economy::game::actor::Actor;
use simpler_economy::game::firm::{Firm, FirmAmvBound, FirmPRow};
use simpler_economy::game::market::{MarketGood, MarketMeeting, WashReason};
use simpler_economy::game::marketorder::MarketOrder;
use simpler_economy::game::workforce::LaborSettlement;

use super::*;

pub(crate) fn is_page_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "day"
            | "d"
            | "days"
            | "stock"
            | "inv"
            | "orders"
            | "books"
            | "book"
            | "processes"
            | "process"
            | "recipes"
            | "amv"
            | "prices"
            | "help"
            | "?"
            | "h"
            | "match"
            | "m"
    )
}

pub(crate) fn format_home(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("=== market tester ===\n");
    out.push_str(&rng_line(session));
    out.push('\n');
    out.push_str("goods  (amv  sal)\n");
    out.push_str("  --  --------  ------  ----\n");
    for good in PREFAB_GOODS {
        out.push_str(&format!(
            "  {:>2}  {:<8}  {:>6}  {:>4}\n",
            good.id,
            good.name,
            fmt_num(session.history.price(good.id)),
            fmt_num(session.history.salability(good.id))
        ));
    }
    let pops: Vec<String> = session
        .pops
        .iter()
        .map(|pop| fmt_actor(Actor::Pop(pop.id)))
        .collect();
    let firms: Vec<String> = session
        .firms
        .iter()
        .map(|firm| fmt_actor(Actor::Firm(firm.id)))
        .collect();
    out.push('\n');
    out.push_str(&format!(
        "pops   {}\n",
        if pops.is_empty() {
            "(none)".to_string()
        } else {
            pops.join("  ")
        }
    ));
    out.push_str(&format!(
        "firms  {}\n",
        if firms.is_empty() {
            "(none)".to_string()
        } else {
            firms.join("  ")
        }
    ));
    out.push_str(&format!(
        "books  {} buys / {} sells\n",
        session.buys.len(),
        session.sells.len()
    ));
    out.push_str(&format!(
        "csv    {}_{}.csv{}\n",
        csv_stem_dir_display(session),
        csv_kind_brace(session),
        csv_flag_suffix(session)
    ));
    out.push_str("\nstock  orders  processes  day  amv  csv  help\n");
    if !session.log.is_empty() {
        out.push_str("\n---\n");
        out.push_str(session.log.trim_end());
        out.push('\n');
    }
    out
}

pub(crate) fn format_stock_page(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("Stock  (live on-hand)\n");
    out.push_str(&format!("  {:<10}", "actor"));
    for good in PREFAB_GOODS {
        out.push_str(&format!("  {:>8}", good.name));
    }
    out.push('\n');
    out.push_str(&format!("  {:-<10}", ""));
    for _ in PREFAB_GOODS {
        out.push_str(&format!("  {:-<8}", ""));
    }
    out.push('\n');
    for pop in &session.pops {
        out.push_str(&format!("  {:<10}", fmt_actor(Actor::Pop(pop.id))));
        for good in PREFAB_GOODS {
            out.push_str(&format!(
                "  {:>8}",
                stock_cell(pop.property.get(&good.id).map(|r| r.quantity))
            ));
        }
        out.push('\n');
    }
    for firm in &session.firms {
        out.push_str(&format!("  {:<10}", fmt_actor(Actor::Firm(firm.id))));
        for good in PREFAB_GOODS {
            out.push_str(&format!(
                "  {:>8}",
                stock_cell(firm.property.get(&good.id).map(|r| r.quantity))
            ));
        }
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&format_firm_bounds(session));
    out.push('\n');
    out.push_str(&format_firm_quotes(session));
    out
}

pub(crate) fn format_firm_bounds(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("Firm bounds  (min = sell floor, max = buy cap)\n");
    out.push_str("  planning guidestones; they do not skip or void trades.\n");
    out.push_str(&format!("  {:<10}  {:<8}  {}\n", "actor", "good", "bound"));
    out.push_str(&format!("  {:-<10}  {:-<8}  {:-<16}\n", "", "", ""));
    let mut any = false;
    for firm in &session.firms {
        let mut rows: Vec<_> = firm.property.iter().collect();
        rows.sort_by_key(|(id, _)| *id);
        for (&good, row) in rows {
            if row.amv_bound == FirmAmvBound::None {
                continue;
            }
            any = true;
            out.push_str(&format!(
                "  {:<10}  {:<8}  {}\n",
                fmt_actor(Actor::Firm(firm.id)),
                fmt_good(good),
                fmt_bound(row.amv_bound)
            ));
        }
    }
    if !any {
        out.push_str("  (none)\n");
    }
    out
}

pub(crate) fn format_firm_records(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("Firm records  (confidence 0 cautious .. 1 aggressive)\n");
    out.push_str(&format!(
        "  {:<10} {:>6} {:>7} {:>8} {:>8}\n",
        "firm", "conf", "profit", "success", "sold AMV"
    ));
    out.push_str(&format!(
        "  {:-<10} {:-<6} {:-<7} {:-<8} {:-<8}\n",
        "", "", "", "", ""
    ));
    if session.firms.is_empty() {
        out.push_str("  (none)\n");
        return out;
    }
    for firm in &session.firms {
        out.push_str(&format!(
            "  {:<10} {:>6} {:>7} {:>8} {:>8}\n",
            fmt_actor(Actor::Firm(firm.id)),
            fmt_num(firm.records.confidence),
            fmt_num(firm.records.profit_ratio),
            fmt_num(firm.records.sell_success),
            fmt_qty(firm.records.sold_amv)
        ));
    }
    out
}

pub(crate) fn format_firm_quotes(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("Firm quotes  (next-day sell / own AMV)\n");
    out.push_str(&format!(
        "  {:<10} {:<8} {:>6} {:>7} {:>7}\n",
        "firm", "good", "sell", "quote", "cost"
    ));
    out.push_str(&format!(
        "  {:-<10} {:-<8} {:-<6} {:-<7} {:-<7}\n",
        "", "", "", "", ""
    ));
    let mut any = false;
    for firm in &session.firms {
        if let Some((good, row)) = firm_primary_output(session, firm) {
            any = true;
            out.push_str(&format!(
                "  {:<10} {:<8} {:>6} {:>7} {:>7}\n",
                fmt_actor(Actor::Firm(firm.id)),
                fmt_good(good),
                fmt_qty(row.sell_target),
                fmt_num(row.amv_target),
                fmt_num(row.average_cost)
            ));
        }
    }
    if !any {
        out.push_str("  (none)\n");
    }
    out
}

/// First process output row, or the first row with a sell target.
pub(crate) fn firm_primary_output<'a>(session: &Session, firm: &'a Firm) -> Option<(usize, &'a FirmPRow)> {
    for line in &firm.production_line {
        if let Some(process) = session.factuals.processes.get(&line.process) {
            if let Some(output) = process.outputs.first() {
                if let Some(row) = firm.property.get(&output.good) {
                    return Some((output.good, row));
                }
            }
        }
    }
    let mut rows: Vec<_> = firm
        .property
        .iter()
        .filter(|(_, row)| row.sell_target > 0.0)
        .collect();
    rows.sort_by_key(|(id, _)| *id);
    rows.into_iter().next().map(|(&id, row)| (id, row))
}

pub(crate) fn format_orders_page(session: &Session) -> String {
    let mut out = String::new();
    out.push_str(&format_order_table(
        session,
        "Buys  (priority, lowest first)",
        &session.buys,
    ));
    out.push('\n');
    out.push_str(&format_order_table(
        session,
        "Sells  (target good id)",
        &session.sells,
    ));
    out
}

pub(crate) fn format_order_table(session: &Session, title: &str, orders: &[MarketOrder]) -> String {
    let mut out = String::new();
    out.push_str(title);
    out.push('\n');
    out.push_str(&order_header());
    out.push('\n');
    out.push_str(&order_rule());
    out.push('\n');
    if orders.is_empty() {
        out.push_str("  (empty)\n");
    } else {
        for (i, order) in orders.iter().enumerate() {
            out.push_str(&order_row(session, i, order));
            out.push('\n');
        }
    }
    out
}

pub(crate) fn format_processes_page(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("Processes  (world recipes)\n");
    out.push_str(&format!("  {:>3}  {:<14}  {}\n", "id", "name", "recipe"));
    out.push_str(&format!("  {:->3}  {:-<14}  {:-<28}\n", "", "", ""));
    let mut ids: Vec<usize> = session.factuals.processes.keys().copied().collect();
    ids.sort_unstable();
    if ids.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for id in ids {
            let process = &session.factuals.processes[&id];
            out.push_str(&format!(
                "  {:>3}  {:<14}  {}\n",
                process.id,
                process.name,
                fmt_recipe(process)
            ));
        }
    }
    out.push('\n');
    out.push_str(&format_firm_records(session));
    out.push('\n');
    out.push_str("Firm lines\n");
    out.push_str(&format!(
        "  {:<10} {:<14} {:>6} {:>6}  {}\n",
        "firm", "process", "did", "want", "missing"
    ));
    out.push_str(&format!(
        "  {:-<10} {:-<14} {:-<6} {:-<6}  {:-<16}\n",
        "", "", "", "", ""
    ));
    let mut any = false;
    for firm in &session.firms {
        for line in &firm.production_line {
            any = true;
            let name = session
                .factuals
                .processes
                .get(&line.process)
                .map(|p| p.name.as_str())
                .unwrap_or("?");
            let missing = if line.last_missing_goods.is_empty() {
                "-".into()
            } else {
                line.last_missing_goods
                    .iter()
                    .copied()
                    .map(fmt_good)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            out.push_str(&format!(
                "  {:<10} {:<14} {:>6} {:>6}  {}\n",
                fmt_actor(Actor::Firm(firm.id)),
                name,
                fmt_qty(line.last_iterations),
                fmt_qty(line.target.unwrap_or(0.0)),
                missing
            ));
        }
    }
    if !any {
        out.push_str("  (none)\n");
    }
    out
}

pub(crate) fn fmt_recipe(process: &simpler_economy::game::process::Process) -> String {
    let inputs: Vec<String> = process
        .inputs
        .iter()
        .map(|input| format!("{} {}", fmt_qty(input.amount), fmt_good(input.good)))
        .collect();
    let outputs: Vec<String> = process
        .outputs
        .iter()
        .map(|output| format!("{} {}", fmt_qty(output.amount), fmt_good(output.good)))
        .collect();
    let left = if inputs.is_empty() {
        "-".into()
    } else {
        inputs.join(" + ")
    };
    let right = if outputs.is_empty() {
        "-".into()
    } else {
        outputs.join(" + ")
    };
    format!("{left} -> {right}")
}

pub(crate) fn stock_cell(qty: Option<f64>) -> String {
    match qty {
        Some(x) => fmt_qty(x),
        None => "-".into(),
    }
}

pub(crate) fn help_text() -> String {
    "\
commands
  day [N]               run N calendar days (default 1)
  stock                 live on-hand + firm AMV bounds and quotes
  orders                current buy/sell books
  processes             world recipes + firm records and lines
  amv                   AMV trail (old -> new)
  csv                   show day-end CSV paths and flags
  csv <name>            write under data/logs/<name>_*.csv
  csv reset             wipe current CSVs and rewrite headers
  csv on <actor>...     log those pops/firms (market/trades always)
  csv off <actor>...    stop logging those actors
  csv off               clear pop/firm flags
  shop                  reload books from create_orders
  home                  back to the summary
  request <actor> <good> <amount> [priority]
  offer   <actor> <good> <amount> [priority]
  buy     <actor> <good> <amount> <amv> <pay-good> <pay-amount> [priority]
  sell    <actor> <good> <amount> <amv> <want-good> <want-amount> [priority]
  match                 one match_orders pass; does not remove anything
  drop buy <i>          remove buy at list index
  drop sell <i>
  keep_alive [on|off]   emergency: feed collapsed firms (default off)
  seed <n>              deterministic rng from n
  unseed                os rng again
  clear                 empty the books
  help
  quit

Home is a short summary. stock / orders / processes / day / amv / help
open a page; home returns. Each `day` appends one-row-per-day CSVs in
data/logs/ (market quotes and trade candles always; flagged pops/firms).
Startup runs shop once. `day` grants Time, settles labor contracts, runs the
market, runs each firm's process, pops consume, then pop and firm
record keeping (firm plan), labor budget / decay.
actor: prefab name (pop1, pop2, ...) or kind id (pop 1)
good:  prefab name (time, grain, gold_token, wood_tools, ...) or id

examples
  day
  stock
  orders
  processes
  day 5
  csv
  csv run1
  csv on pop1 pop2
  request pop1 grain 3"
        .into()
}

pub(crate) fn day_digest(
    session: &Session,
    report: &MarketDayReport,
    wages: &[(usize, LaborSettlement)],
) -> String {
    let n_trade = report
        .meetings
        .iter()
        .filter(|m| matches!(m.outcome, MeetingOutcome::Traded { .. }))
        .count();
    let n_wash = report.meetings.len() - n_trade;
    let wage_coin: f64 = wages.iter().map(|(_, s)| labor_coin_paid(s)).sum();
    let sol: f64 = if session.pops.is_empty() {
        0.0
    } else {
        session
            .pops
            .iter()
            .map(|pop| pop.records.living_standard)
            .sum::<f64>()
            / session.pops.len() as f64
    };
    let conf: f64 = if session.firms.is_empty() {
        0.0
    } else {
        session
            .firms
            .iter()
            .map(|firm| firm.records.confidence)
            .sum::<f64>()
            / session.firms.len() as f64
    };
    format!(
        "{} trade{}  {} wash{}  wages {} coin  SOL {}  conf {}",
        n_trade,
        if n_trade == 1 { "" } else { "s" },
        n_wash,
        if n_wash == 1 { "" } else { "es" },
        fmt_qty(wage_coin),
        fmt_num(sol),
        fmt_num(conf)
    )
}

pub(crate) fn format_day_report(
    session: &Session,
    report: &MarketDayReport,
    wages: &[(usize, LaborSettlement)],
) -> String {
    let n_trade = report
        .meetings
        .iter()
        .filter(|m| matches!(m.outcome, MeetingOutcome::Traded { .. }))
        .count();
    let n_wash = report.meetings.len() - n_trade;
    let haul: f64 = report
        .meetings
        .iter()
        .map(|m| match &m.outcome {
            MeetingOutcome::Traded { transport_needed, .. } => *transport_needed,
            MeetingOutcome::Wash { transport, .. } => *transport,
        })
        .sum();
    let has_cargo = session
        .factuals
        .goods
        .values()
        .any(|good| good.is_transport());

    let mut out = String::new();
    out.push_str(&format!("=== market day {} ===\n", session.day));
    out.push_str(&format!(
        "{} trade{}   {} wash{}   {} unmatched   leftover {} buy / {} sell\n",
        n_trade,
        if n_trade == 1 { "" } else { "s" },
        n_wash,
        if n_wash == 1 { "" } else { "es" },
        report.unmatched_buys.len(),
        report.leftover_buys.len(),
        report.leftover_sells.len(),
    ));
    if has_cargo {
        out.push_str(&format!("haul  {}\n", fmt_qty(haul)));
    } else {
        out.push_str("no transport-tagged goods; haul skipped\n");
    }
    out.push_str(&format!("{:-<64}\n", ""));

    if !report.unmatched_buys.is_empty() {
        out.push_str("\nUnmatched  (no seller)\n");
        out.push_str(&format!("  {:<10}  {:>6} {}\n", "buyer", "qty", "good"));
        out.push_str(&format!("  {:-<10}  {:-<6} {:-<8}\n", "", "", ""));
        for order in &report.unmatched_buys {
            out.push_str(&format!(
                "  {:<10}  {:>6} {}\n",
                fmt_actor(order.origin),
                fmt_qty(order.target_amount.abs()),
                fmt_good(order.target)
            ));
        }
    }

    out.push_str("\nTrades\n");
    let trades: Vec<_> = report
        .meetings
        .iter()
        .filter(|m| matches!(m.outcome, MeetingOutcome::Traded { .. }))
        .collect();
    if trades.is_empty() {
        out.push_str("  (none)\n");
    } else {
        out.push_str(&format!(
            "  {:<10}  {:>6} {:<8} | {:<10}  {}\n",
            "buyer", "qty", "good", "seller", "pays"
        ));
        out.push_str(&format!(
            "  {:-<10}  {:-<6} {:-<8}-+-{:-<10}  {:-<16}\n",
            "", "", "", "", ""
        ));
        for meeting in trades {
            let MeetingOutcome::Traded {
                goods,
                transport_needed,
            } = &meeting.outcome
            else {
                continue;
            };
            let mut line = format!(
                "  {:<10}  {:>6} {:<8} | {:<10}  {}",
                fmt_actor(meeting.buy.origin),
                fmt_qty(bought_qty(goods, meeting.buy.target)),
                fmt_good(meeting.buy.target),
                fmt_actor(meeting.sell.origin),
                fmt_payment(goods, meeting.buy.target)
            );
            if has_cargo && *transport_needed > 0.0 {
                line.push_str(&format!("  haul {}", fmt_qty(*transport_needed)));
            }
            out.push_str(&line);
            out.push('\n');
        }
    }

    out.push_str("\nWashes\n");
    let washes = group_washes(&report.meetings);
    if washes.is_empty() {
        out.push_str("  (none)\n");
    } else {
        out.push_str(&format!(
            "  {:<10}  {:<8} | {:<10}  {:<14}  {}\n",
            "buyer", "good", "seller", "why", "end"
        ));
        out.push_str(&format!(
            "  {:-<10}  {:-<8}-+-{:-<10}  {:-<14}  {:-<10}\n",
            "", "", "", "", ""
        ));
        for group in washes {
            out.push_str(&format!(
                "  {:<10}  {:<8} | {:<10}  {:<14}  x{} {}\n",
                fmt_actor(group.buyer),
                fmt_good(group.good),
                fmt_actor(group.seller),
                fmt_wash_reason(group.reason),
                group.count,
                if group.closed { "closed" } else { "open" }
            ));
        }
    }

    if !report.leftover_buys.is_empty() {
        out.push_str("\nLeftover buys\n");
        out.push_str(&format!("  {:<10}  {:>6} {}\n", "buyer", "qty", "good"));
        out.push_str(&format!("  {:-<10}  {:-<6} {:-<8}\n", "", "", ""));
        fmt_compact_orders(&mut out, &report.leftover_buys);
    }
    if !report.leftover_sells.is_empty() {
        out.push_str("\nLeftover sells\n");
        out.push_str(&format!("  {:<10}  {:>6} {}\n", "seller", "qty", "good"));
        out.push_str(&format!("  {:-<10}  {:-<6} {:-<8}\n", "", "", ""));
        fmt_compact_orders(&mut out, &report.leftover_sells);
    }

    out.push_str("\nOutcomes\n");
    out.push_str(&format!(
        "  {:<8} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7}\n",
        "good", "demand", "supply", "bought", "paid", "vol", "amv", "sal"
    ));
    out.push_str(&format!(
        "  {:-<8} {:-<7} {:-<7} {:-<7} {:-<7} {:-<7} {:-<7} {:-<7}\n",
        "", "", "", "", "", "", "", ""
    ));
    let mut ids: Vec<usize> = session.market.goods.keys().copied().collect();
    ids.sort_unstable();
    let mut any_row = false;
    for id in ids {
        let row = &session.market.goods[&id];
        if row.demand == 0.0
            && row.supply == 0.0
            && row.purchased == 0.0
            && row.payment == 0.0
        {
            continue;
        }
        any_row = true;
        out.push_str(&format!(
            "  {:<8} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7}\n",
            fmt_good(id),
            fmt_qty(row.demand),
            fmt_qty(row.supply),
            fmt_qty(row.purchased),
            fmt_qty(row.payment),
            fmt_qty(row.volume()),
            fmt_qty(row.amv),
            fmt_qty(row.salability)
        ));
    }
    if !any_row {
        out.push_str("  (none)\n");
    }
    if !session.market.unavailable_goods.is_empty() {
        let mut names: Vec<String> = session
            .market
            .unavailable_goods
            .iter()
            .copied()
            .map(fmt_good)
            .collect();
        names.sort();
        out.push_str(&format!("unavailable  {}\n", names.join(", ")));
    }
    out.push('\n');
    out.push_str(&format_wage_report(session, wages));
    out.push_str(&format_production_report(session));
    out.push_str(&format_plan_report(session));
    out.push_str(&format_pop_report(session));
    out.push_str(&format_amv_trail(session));
    out.push_str("shop reloaded from current stock.\n");
    out.push_str(&csv_status_line(session));
    out
}

fn labor_coin_paid(settle: &LaborSettlement) -> f64 {
    let workers: f64 = settle
        .workers
        .iter()
        .map(|w| w.paid.get(&GOLD_TOKEN).copied().unwrap_or(0.0))
        .sum();
    let owner = settle
        .owner
        .as_ref()
        .map(|o| o.paid.get(&GOLD_TOKEN).copied().unwrap_or(0.0))
        .unwrap_or(0.0);
    workers + owner
}

fn format_paid_map(paid: &HashMap<usize, f64>) -> String {
    let mut parts: Vec<(usize, f64)> = paid
        .iter()
        .filter(|(_, qty)| **qty > 0.0)
        .map(|(good, qty)| (*good, *qty))
        .collect();
    parts.sort_by_key(|(good, _)| *good);
    if parts.is_empty() {
        return "-".into();
    }
    parts
        .iter()
        .map(|(good, qty)| format!("{} {}", fmt_qty(*qty), fmt_good(*good)))
        .collect::<Vec<_>>()
        .join(" + ")
}

pub(crate) fn format_wage_report(session: &Session, wages: &[(usize, LaborSettlement)]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Labor  (settle; work-time cap {:.0}%)\n",
        session.factuals.config.labor.work_time_fraction * 100.0
    ));
    out.push_str(&format!(
        "  {:<10} {:<10} {:>6} {:>6}  {}\n",
        "firm", "who", "hours", "time", "paid"
    ));
    out.push_str(&format!(
        "  {:-<10} {:-<10} {:-<6} {:-<6}  {:-<24}\n",
        "", "", "", "", ""
    ));
    if wages.is_empty() {
        out.push_str("  (none)\n\n");
        return out;
    }
    for (firm_id, settle) in wages {
        let firm_name = fmt_actor(Actor::Firm(*firm_id));
        if settle.workers.is_empty() && settle.owner.is_none() {
            out.push_str(&format!(
                "  {:<10} {:<10} {:>6} {:>6}  {}\n",
                firm_name, "-", "-", "-", "-"
            ));
            continue;
        }
        for worker in &settle.workers {
            out.push_str(&format!(
                "  {:<10} {:<10} {:>6} {:>6}  {}\n",
                firm_name,
                fmt_actor(Actor::Pop(worker.pop)),
                fmt_qty(worker.time_claimed),
                fmt_qty(worker.time_given),
                format_paid_map(&worker.paid)
            ));
        }
        if let Some(owner) = &settle.owner {
            let paid = format_paid_map(&owner.paid);
            let paid = if owner.remainder {
                if paid == "-" {
                    "remainder".into()
                } else {
                    format!("remainder {paid}")
                }
            } else {
                paid
            };
            out.push_str(&format!(
                "  {:<10} {:<10} {:>6} {:>6}  {}\n",
                firm_name,
                format!("owner {}", fmt_actor(Actor::Pop(owner.pop))),
                "-",
                "-",
                paid
            ));
        }
    }
    out.push('\n');
    out
}

pub(crate) fn format_production_report(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("Production  (did today; want is next-day after plan)\n");
    out.push_str(&format!(
        "  {:<10} {:>6} {:>6}  {}\n",
        "firm", "did", "want", "flows / missing"
    ));
    out.push_str(&format!(
        "  {:-<10} {:-<6} {:-<6}  {:-<28}\n",
        "", "", "", ""
    ));
    if session.firms.is_empty() {
        out.push_str("  (none)\n\n");
        return out;
    }
    for firm in &session.firms {
        if firm.production_line.is_empty() {
            continue;
        }
        for (idx, line) in firm.production_line.iter().enumerate() {
            let want = line.target.unwrap_or(0.0);
            let mut bits: Vec<String> = Vec::new();
            // Property flows are firm-wide; print them once on the first line.
            if idx == 0 {
                let mut ids: Vec<usize> = firm.property.keys().copied().collect();
                ids.sort_unstable();
                for id in ids {
                    let row = &firm.property[&id];
                    if row.produced > 0.0 {
                        bits.push(format!("+{} {}", fmt_qty(row.produced), fmt_good(id)));
                    }
                    if row.consumed > 0.0 {
                        bits.push(format!("-{} {}", fmt_qty(row.consumed), fmt_good(id)));
                    }
                }
            }
            if !line.last_missing_goods.is_empty() {
                let missing: Vec<String> = line
                    .last_missing_goods
                    .iter()
                    .copied()
                    .map(fmt_good)
                    .collect();
                bits.push(format!("missing {}", missing.join(", ")));
            }
            if bits.is_empty() {
                bits.push("-".into());
            }
            out.push_str(&format!(
                "  {:<10} {:>6} {:>6}  {}\n",
                fmt_actor(Actor::Firm(firm.id)),
                fmt_qty(line.last_iterations),
                fmt_qty(want),
                bits.join("  ")
            ));
        }
    }
    out.push('\n');
    out
}

pub(crate) fn format_plan_report(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("Plans  (after firm record keeping)\n");
    out.push_str(&format!(
        "  {:<10} {:>6} {:>7} {:>8} {:>6} {:>6} {:<8} {:>7}\n",
        "firm", "conf", "profit", "success", "want", "sell", "good", "quote"
    ));
    out.push_str(&format!(
        "  {:-<10} {:-<6} {:-<7} {:-<8} {:-<6} {:-<6} {:-<8} {:-<7}\n",
        "", "", "", "", "", "", "", ""
    ));
    if session.firms.is_empty() {
        out.push_str("  (none)\n\n");
        return out;
    }
    for firm in &session.firms {
        let want = firm
            .production_line
            .first()
            .and_then(|line| line.target)
            .unwrap_or(0.0);
        let (good_name, sell, quote) = match firm_primary_output(session, firm) {
            Some((good, row)) => (
                fmt_good(good),
                fmt_qty(row.sell_target),
                fmt_num(row.amv_target),
            ),
            None => ("-".into(), "-".into(), "-".into()),
        };
        out.push_str(&format!(
            "  {:<10} {:>6} {:>7} {:>8} {:>6} {:>6} {:<8} {:>7}\n",
            fmt_actor(Actor::Firm(firm.id)),
            fmt_num(firm.records.confidence),
            fmt_num(firm.records.profit_ratio),
            fmt_num(firm.records.sell_success),
            fmt_qty(want),
            sell,
            good_name,
            quote
        ));
    }
    out.push('\n');
    out
}

pub(crate) fn format_pop_report(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("Pops  (after consume)\n");
    out.push_str(&format!(
        "  {:<10} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}\n",
        "actor", "basic", "common", "luxury", "SOL", "shop", "income"
    ));
    out.push_str(&format!(
        "  {:-<10} {:-<6} {:-<6} {:-<6} {:-<6} {:-<6} {:-<6}\n",
        "", "", "", "", "", "", ""
    ));
    if session.pops.is_empty() {
        out.push_str("  (none)\n\n");
        return out;
    }
    for pop in &session.pops {
        out.push_str(&format!(
            "  {:<10} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}\n",
            fmt_actor(Actor::Pop(pop.id)),
            fmt_num(pop.records.tier_sat[0]),
            fmt_num(pop.records.tier_sat[1]),
            fmt_num(pop.records.tier_sat[2]),
            fmt_num(pop.records.living_standard),
            fmt_num(pop.records.shop_fill),
            fmt_qty(pop.records.income_amv)
        ));
    }
    out.push('\n');
    out
}

pub(crate) fn format_amv_trail(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("AMV trail  (old -> new)\n");
    out.push_str(&format!(
        "  {:<8} {:>7} {:>7}  {}\n",
        "good", "now", "diff", "trail"
    ));
    out.push_str(&format!(
        "  {:-<8} {:-<7} {:-<7}  {:-<16}\n",
        "", "", "", ""
    ));
    let mut ids: Vec<usize> = session.market.goods.keys().copied().collect();
    ids.sort_unstable();
    if ids.is_empty() {
        out.push_str("  (none)\n");
        return out;
    }
    for id in ids {
        let row = &session.market.goods[&id];
        out.push_str(&format!(
            "  {:<8} {:>7} {:>7}  {}\n",
            fmt_good(id),
            fmt_num(row.amv),
            fmt_amv_delta(row),
            fmt_amv_history(row)
        ));
    }
    out
}

pub(crate) fn fmt_amv_history(row: &MarketGood) -> String {
    let trail = row.amv_trail();
    if trail.is_empty() {
        return "-".into();
    }
    trail
        .iter()
        .map(|v| fmt_num(*v))
        .collect::<Vec<_>>()
        .join("  ")
}

pub(crate) fn fmt_amv_delta(row: &MarketGood) -> String {
    let trail = row.amv_trail();
    if trail.len() < 2 {
        return "-".into();
    }
    let d = trail[trail.len() - 1] - trail[trail.len() - 2];
    if d.abs() < 1e-12 {
        "0".into()
    } else if d > 0.0 {
        format!("+{}", fmt_num(d))
    } else {
        fmt_num(d)
    }
}

pub(crate) struct WashGroup {
    buyer: Actor,
    good: usize,
    seller: Actor,
    reason: WashReason,
    count: usize,
    closed: bool,
}

pub(crate) fn group_washes(meetings: &[MarketMeeting]) -> Vec<WashGroup> {
    let mut groups: Vec<WashGroup> = Vec::new();
    for meeting in meetings {
        let MeetingOutcome::Wash {
            reason,
            closed,
            ..
        } = meeting.outcome
        else {
            continue;
        };
        if let Some(group) = groups.iter_mut().find(|group| {
            group.buyer == meeting.buy.origin
                && group.good == meeting.buy.target
                && group.seller == meeting.sell.origin
                && group.reason == reason
        }) {
            group.count += 1;
            group.closed = closed;
        } else {
            groups.push(WashGroup {
                buyer: meeting.buy.origin,
                good: meeting.buy.target,
                seller: meeting.sell.origin,
                reason,
                count: 1,
                closed,
            });
        }
    }
    groups
}

pub(crate) fn bought_qty(goods: &HashMap<usize, f64>, target: usize) -> f64 {
    goods.get(&target).copied().unwrap_or(0.0).abs()
}

pub(crate) fn fmt_payment(goods: &HashMap<usize, f64>, target: usize) -> String {
    let mut ids: Vec<usize> = goods
        .iter()
        .filter(|(id, qty)| **id != target && **qty > 0.0)
        .map(|(id, _)| *id)
        .collect();
    ids.sort_unstable();
    if ids.is_empty() {
        return "-".into();
    }
    ids.into_iter()
        .map(|id| format!("{} {}", fmt_qty(goods[&id]), fmt_good(id)))
        .collect::<Vec<_>>()
        .join(" + ")
}

pub(crate) fn fmt_compact_orders(out: &mut String, orders: &[MarketOrder]) {
    for order in orders {
        out.push_str(&format!(
            "  {:<10}  {:>6} {}\n",
            fmt_actor(order.origin),
            fmt_qty(order.target_amount.abs()),
            fmt_good(order.target)
        ));
    }
}

pub(crate) fn fmt_wash_reason(reason: WashReason) -> &'static str {
    match reason {
        WashReason::NoProposal => "no proposal",
        WashReason::Rejected => "rejected",
        WashReason::EmptyFill => "empty fill",
    }
}

pub(crate) fn fmt_qty(x: f64) -> String {
    if x.is_finite() && (x - x.round()).abs() < 1e-9 && x.abs() < 1e12 {
        format!("{:.0}", x.round())
    } else {
        format!("{x:.2}")
    }
}

pub(crate) fn coincidence(buy: &MarketOrder, sell: &MarketOrder) -> bool {
    match (buy.counter_offer, sell.counter_offer) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

pub(crate) fn actor_label(actor: Actor) -> Option<&'static str> {
    PREFAB_ACTORS
        .iter()
        .find(|a| a.actor == actor)
        .map(|a| a.name)
}

pub(crate) fn good_label(id: usize) -> Option<&'static str> {
    PREFAB_GOODS.iter().find(|g| g.id == id).map(|g| g.name)
}

pub(crate) fn fmt_actor_kind_id(actor: Actor) -> String {
    match actor {
        Actor::Pop(id) => format!("pop {id}"),
        Actor::Firm(id) => format!("firm {id}"),
        Actor::Institution(id) => format!("inst {id}"),
        Actor::State(id) => format!("state {id}"),
    }
}

pub(crate) fn fmt_actor(actor: Actor) -> String {
    if let Actor::Pop(id) = actor {
        return format!("pop{id}");
    }
    match actor_label(actor) {
        Some(name) => name.to_string(),
        None => fmt_actor_kind_id(actor),
    }
}

pub(crate) fn fmt_good(id: usize) -> String {
    match good_label(id) {
        Some(name) => name.to_string(),
        None => format!("#{id}"),
    }
}

pub(crate) fn fmt_order(session: &Session, order: &MarketOrder) -> String {
    format!(
        "{} {} {} amt {} prio {} amv {} bound {} counter {}",
        order_kind(order),
        fmt_actor(order.origin),
        fmt_good(order.target),
        fmt_num(order.target_amount),
        fmt_num(order.priority),
        amv_cell(order),
        order_bound_cell(session, order),
        counter_cell(order)
    )
}

pub(crate) fn order_kind(order: &MarketOrder) -> &'static str {
    if order.is_request_order() {
        "request"
    } else if order.is_buy_order() {
        "buy"
    } else if order.is_offer_order() {
        "offer"
    } else if order.is_sell_order() {
        "sell"
    } else {
        "order"
    }
}

pub(crate) fn fmt_num(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e12 {
        format!("{x:.0}")
    } else {
        format!("{x:.4}")
    }
}

pub(crate) fn counter_cell(order: &MarketOrder) -> String {
    match (order.counter_offer, order.counter_offer_amount) {
        (Some(good), Some(amt)) => format!("{} {}", fmt_good(good), fmt_num(amt)),
        _ => "-".into(),
    }
}

pub(crate) fn amv_cell(order: &MarketOrder) -> String {
    match order.amv_target {
        Some(amv) => fmt_num(amv),
        None => "-".into(),
    }
}

pub(crate) fn fmt_bound(bound: FirmAmvBound) -> String {
    match bound {
        FirmAmvBound::None => "-".into(),
        FirmAmvBound::Minimum(v) => format!("min {}", fmt_num(v)),
        FirmAmvBound::Maximum(v) => format!("max {}", fmt_num(v)),
        FirmAmvBound::MinMax(min, max) => {
            format!("min {} max {}", fmt_num(min), fmt_num(max))
        }
    }
}

pub(crate) fn order_bound_cell(session: &Session, order: &MarketOrder) -> String {
    let Actor::Firm(id) = order.origin else {
        return "-".into();
    };
    let Some(firm) = session.firms.iter().find(|f| f.id == id) else {
        return "-".into();
    };
    match firm.property.get(&order.target) {
        Some(row) => fmt_bound(row.amv_bound),
        None => "-".into(),
    }
}

pub(crate) fn order_header() -> String {
    format!(
        "{:>2}  {:<7}  {:<10}  {:<8}  {:>8}  {:>8}  {:>6}  {:<16}  {}",
        "#", "kind", "actor", "good", "amt", "prio", "amv", "bound", "counter"
    )
}

pub(crate) fn order_rule() -> String {
    format!(
        "{:-<2}  {:-<7}  {:-<10}  {:-<8}  {:-<8}  {:-<8}  {:-<6}  {:-<16}  {:-<16}",
        "", "", "", "", "", "", "", "", ""
    )
}

pub(crate) fn order_row(session: &Session, idx: usize, order: &MarketOrder) -> String {
    format!(
        "{:>2}  {:<7}  {:<10}  {:<8}  {:>8}  {:>8}  {:>6}  {:<16}  {}",
        idx,
        order_kind(order),
        fmt_actor(order.origin),
        fmt_good(order.target),
        fmt_num(order.target_amount),
        fmt_num(order.priority),
        amv_cell(order),
        order_bound_cell(session, order),
        counter_cell(order)
    )
}

