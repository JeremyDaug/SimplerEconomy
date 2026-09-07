use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use simpler_economy::game::actor::Actor;
use simpler_economy::game::market::MarketDayReport;

use super::*;

pub(crate) fn csv_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/logs")
}

pub(crate) fn csv_path(stem: &str, kind: &str) -> PathBuf {
    csv_dir().join(format!("{stem}_{kind}.csv"))
}

pub(crate) fn csv_stem_dir_display(session: &Session) -> String {
    format!("data/logs/{}", session.csv_stem)
}

pub(crate) fn csv_status_line(session: &Session) -> String {
    format!(
        "logged  {}_{{market,firms,trades}}.csv",
        csv_stem_dir_display(session)
    )
}

pub(crate) fn csv_status(session: &Session) -> String {
    let stem = &session.csv_stem;
    format!(
        "CSV directory  data/logs/\n  {stem}_market.csv   one row/day; good blocks: amv, salability, average_price\n  {stem}_firms.csv    one row/day; firm conf/profit/success, then good blocks: qty, targets, bid, ask, costs, sold, produced\n  {stem}_trades.csv   one row/day; good candles: open, high, low, close, volume\ncsv <name>  changes the stem.  csv reset  wipes these files. Header mismatch (old layout) needs reset or a new stem."
    )
}

pub(crate) fn handle_csv_command(session: &mut Session, rest: &[&str]) -> String {
    if rest.is_empty() {
        return csv_status(session);
    }
    if rest.len() == 1 && rest[0].eq_ignore_ascii_case("reset") {
        return match reset_price_log(session) {
            Ok(()) => format!("wiped CSVs.\n{}", csv_status(session)),
            Err(err) => format!("csv reset failed: {err}"),
        };
    }
    if rest.len() != 1 {
        return "usage: csv [name]  or  csv reset".into();
    }
    match sanitize_csv_stem(rest[0]) {
        Ok(stem) => {
            session.csv_stem = stem;
            csv_status(session)
        }
        Err(err) => err,
    }
}

pub(crate) fn sanitize_csv_stem(raw: &str) -> Result<String, String> {
    if raw.is_empty() || raw.len() > 40 {
        return Err("csv name must be 1 to 40 characters".into());
    }
    if !raw
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("csv name may use letters, digits, _ and - only".into());
    }
    if raw == "reset" {
        return Err("csv name 'reset' is reserved. Use csv reset to wipe.".into());
    }
    Ok(raw.to_string())
}

pub(crate) fn reset_price_log(session: &Session) -> Result<(), String> {
    fs::create_dir_all(csv_dir()).map_err(|err| format!("create data/logs: {err}"))?;
    write_csv_file(
        &csv_path(&session.csv_stem, "market"),
        &market_csv_header(session),
        &[],
        true,
    )?;
    write_csv_file(
        &csv_path(&session.csv_stem, "firms"),
        &firm_csv_header(session),
        &[],
        true,
    )?;
    write_csv_file(
        &csv_path(&session.csv_stem, "trades"),
        &trade_csv_header(session),
        &[],
        true,
    )?;
    Ok(())
}

pub(crate) const MARKET_CSV_FIELDS: &[&str] = &["amv", "salability", "average_price"];
pub(crate) const FIRM_RECORD_FIELDS: &[&str] = &["confidence", "profit", "sell_success"];
pub(crate) const FIRM_CSV_FIELDS: &[&str] = &[
    "quantity",
    "sell_target",
    "amv_target",
    "bid",
    "ask",
    "average_cost",
    "average_price",
    "sold",
    "produced",
];
pub(crate) const TRADE_CSV_FIELDS: &[&str] = &["open", "high", "low", "close", "volume"];

pub(crate) fn append_price_log(session: &Session, report: &MarketDayReport) -> Result<(), String> {
    fs::create_dir_all(csv_dir()).map_err(|err| format!("create data/logs: {err}"))?;
    write_csv_file(
        &csv_path(&session.csv_stem, "market"),
        &market_csv_header(session),
        &[market_csv_row(session)],
        false,
    )?;
    write_csv_file(
        &csv_path(&session.csv_stem, "firms"),
        &firm_csv_header(session),
        &[firm_csv_row(session)],
        false,
    )?;
    write_csv_file(
        &csv_path(&session.csv_stem, "trades"),
        &trade_csv_header(session),
        &[trade_csv_row(session, report)],
        false,
    )?;
    Ok(())
}

pub(crate) fn write_csv_file(
    path: &Path,
    header: &str,
    rows: &[String],
    reset: bool,
) -> Result<(), String> {
    if reset {
        let mut text = String::from(header);
        text.push('\n');
        for row in rows {
            text.push_str(row);
            text.push('\n');
        }
        return fs::write(path, text).map_err(|err| format!("write {}: {err}", path.display()));
    }
    let need_header = match fs::read_to_string(path) {
        Ok(text) if text.is_empty() => true,
        Ok(text) => {
            let existing = text.lines().next().unwrap_or("");
            if existing != header {
                return Err(format!(
                    "{} header does not match (old layout?). Run `csv reset` or `csv <newname>`.",
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("csv")
                ));
            }
            false
        }
        Err(_) => true,
    };
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| format!("open {}: {err}", path.display()))?;
    if need_header {
        writeln!(file, "{header}").map_err(|err| format!("write {}: {err}", path.display()))?;
    }
    for row in rows {
        writeln!(file, "{row}").map_err(|err| format!("write {}: {err}", path.display()))?;
    }
    Ok(())
}

pub(crate) fn csv_num(value: f64) -> String {
    if value.is_finite() {
        format!("{value}")
    } else {
        String::new()
    }
}

pub(crate) fn market_csv_goods(session: &Session) -> Vec<usize> {
    let mut ids: Vec<usize> = PREFAB_GOODS.iter().map(|good| good.id).collect();
    let mut extra: Vec<usize> = session
        .market
        .goods
        .keys()
        .copied()
        .filter(|id| !ids.contains(id))
        .collect();
    extra.sort_unstable();
    ids.extend(extra);
    ids
}

pub(crate) fn csv_good_col(id: usize) -> String {
    fmt_good(id)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

pub(crate) fn market_csv_header(session: &Session) -> String {
    let mut cols = vec!["day".to_string()];
    for id in market_csv_goods(session) {
        let name = csv_good_col(id);
        for field in MARKET_CSV_FIELDS {
            cols.push(format!("{name}_{field}"));
        }
    }
    cols.join(",")
}

pub(crate) fn market_csv_row(session: &Session) -> String {
    let mut cells = vec![session.day.to_string()];
    for id in market_csv_goods(session) {
        match session.market.goods.get(&id) {
            Some(row) => {
                cells.push(csv_num(row.amv));
                cells.push(csv_num(row.salability));
                if row.purchased > 0.0 {
                    cells.push(csv_num(row.average_price));
                } else {
                    cells.push(String::new());
                }
            }
            None => {
                for _ in MARKET_CSV_FIELDS {
                    cells.push(String::new());
                }
            }
        }
    }
    cells.join(",")
}

pub(crate) fn csv_actor_col(actor: Actor) -> String {
    fmt_actor(actor)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

pub(crate) fn firm_csv_header(session: &Session) -> String {
    let mut cols = vec!["day".to_string()];
    let goods = market_csv_goods(session);
    for firm in &session.firms {
        let firm_name = csv_actor_col(Actor::Firm(firm.id));
        for field in FIRM_RECORD_FIELDS {
            cols.push(format!("{firm_name}_{field}"));
        }
        for id in &goods {
            let good_name = csv_good_col(*id);
            for field in FIRM_CSV_FIELDS {
                cols.push(format!("{firm_name}_{good_name}_{field}"));
            }
        }
    }
    cols.join(",")
}

pub(crate) fn firm_csv_row(session: &Session) -> String {
    let mut cells = vec![session.day.to_string()];
    let goods = market_csv_goods(session);
    for firm in &session.firms {
        cells.push(csv_num(firm.records.confidence));
        cells.push(csv_num(firm.records.profit_ratio));
        cells.push(csv_num(firm.records.sell_success));
        for id in &goods {
            match firm.property.get(id) {
                Some(row) => {
                    let market_amv = session.history.price(*id);
                    let mid = row.mid_amv(market_amv);
                    cells.push(csv_num(row.quantity));
                    cells.push(csv_num(row.sell_target));
                    cells.push(csv_num(row.amv_target));
                    cells.push(csv_num(row.bid_amv(mid)));
                    cells.push(csv_num(row.ask_amv(mid)));
                    cells.push(csv_num(row.average_cost));
                    cells.push(csv_num(row.average_price));
                    cells.push(csv_num(row.sold));
                    cells.push(csv_num(row.produced));
                }
                None => {
                    for _ in FIRM_CSV_FIELDS {
                        cells.push(String::new());
                    }
                }
            }
        }
    }
    cells.join(",")
}

pub(crate) fn trade_csv_header(session: &Session) -> String {
    let mut cols = vec!["day".to_string()];
    for id in market_csv_goods(session) {
        let name = csv_good_col(id);
        for field in TRADE_CSV_FIELDS {
            cols.push(format!("{name}_{field}"));
        }
    }
    cols.join(",")
}

pub(crate) struct DayCandle {
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
    seen: bool,
}

impl DayCandle {
    fn new() -> Self {
        Self {
            open: 0.0,
            high: 0.0,
            low: 0.0,
            close: 0.0,
            volume: 0.0,
            seen: false,
        }
    }

    fn push(&mut self, unit_amv: f64, qty: f64) {
        if !unit_amv.is_finite() || qty <= 0.0 {
            return;
        }
        if !self.seen {
            self.open = unit_amv;
            self.high = unit_amv;
            self.low = unit_amv;
            self.close = unit_amv;
            self.volume = qty;
            self.seen = true;
        } else {
            self.high = self.high.max(unit_amv);
            self.low = self.low.min(unit_amv);
            self.close = unit_amv;
            self.volume += qty;
        }
    }
}

pub(crate) fn trade_csv_row(session: &Session, report: &MarketDayReport) -> String {
    let goods = market_csv_goods(session);
    let mut candles: HashMap<usize, DayCandle> = goods
        .iter()
        .map(|id| (*id, DayCandle::new()))
        .collect();
    for meeting in &report.meetings {
        let MeetingOutcome::Traded { goods: basket, .. } = &meeting.outcome else {
            continue;
        };
        let sought = meeting.buy.target;
        let qty = basket.get(&sought).copied().unwrap_or(0.0).abs();
        let pay_amv: f64 = basket
            .iter()
            .filter(|(id, q)| **id != sought && **q > 0.0)
            .map(|(id, q)| *q * session.history.price(*id))
            .sum();
        if qty <= 0.0 {
            continue;
        }
        let unit_amv = pay_amv / qty;
        if let Some(candle) = candles.get_mut(&sought) {
            candle.push(unit_amv, qty);
        }
    }
    let mut cells = vec![session.day.to_string()];
    for id in goods {
        match candles.get(&id) {
            Some(candle) if candle.seen => {
                cells.push(csv_num(candle.open));
                cells.push(csv_num(candle.high));
                cells.push(csv_num(candle.low));
                cells.push(csv_num(candle.close));
                cells.push(csv_num(candle.volume));
            }
            _ => {
                cells.push(String::new());
                cells.push(String::new());
                cells.push(String::new());
                cells.push(String::new());
                cells.push(csv_num(0.0));
            }
        }
    }
    cells.join(",")
}


