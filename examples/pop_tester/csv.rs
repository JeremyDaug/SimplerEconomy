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

pub(crate) fn csv_kind_brace(session: &Session) -> String {
    let mut kinds = vec!["market", "trades"];
    if !session.csv_firms.is_empty() {
        kinds.push("firms");
    }
    if !session.csv_pops.is_empty() {
        kinds.push("pops");
    }
    format!("{{{}}}", kinds.join(","))
}

pub(crate) fn csv_status_line(session: &Session) -> String {
    format!(
        "logged  {}_{}.csv{}",
        csv_stem_dir_display(session),
        csv_kind_brace(session),
        csv_flag_suffix(session)
    )
}

pub(crate) fn csv_flag_suffix(session: &Session) -> String {
    let firms = csv_flagged_firm_names(session);
    let pops = csv_flagged_pop_names(session);
    match (firms.is_empty(), pops.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("  firms: {firms}"),
        (true, false) => format!("  pops: {pops}"),
        (false, false) => format!("  firms: {firms}  pops: {pops}"),
    }
}

fn csv_flagged_firm_names(session: &Session) -> String {
    session
        .firms
        .iter()
        .filter(|firm| session.csv_firms.contains(&firm.id))
        .map(|firm| fmt_actor(Actor::Firm(firm.id)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn csv_flagged_pop_names(session: &Session) -> String {
    session
        .pops
        .iter()
        .filter(|pop| session.csv_pops.contains(&pop.id))
        .map(|pop| fmt_actor(Actor::Pop(pop.id)))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn csv_status(session: &Session) -> String {
    let stem = &session.csv_stem;
    let mut out = format!(
        "CSV directory  data/logs/\n  {stem}_market.csv   always; one row/day; good blocks: amv, salability, average_price\n  {stem}_trades.csv   always; one row/day; good candles: open, high, low, close, volume\n"
    );
    if session.csv_firms.is_empty() {
        out.push_str(&format!(
            "  {stem}_firms.csv    off until `csv on <firm>`\n"
        ));
    } else {
        out.push_str(&format!(
            "  {stem}_firms.csv    flagged {}; firm profit/success, then good blocks: qty, targets, bid, ask, costs, sold, produced\n",
            csv_flagged_firm_names(session)
        ));
    }
    if session.csv_pops.is_empty() {
        out.push_str(&format!(
            "  {stem}_pops.csv     off until `csv on <pop>`\n"
        ));
    } else {
        out.push_str(&format!(
            "  {stem}_pops.csv     flagged {}; pop sat/SOL/shop/income/wealth, then good blocks: qty, shop, save, consumed\n",
            csv_flagged_pop_names(session)
        ));
    }
    out.push_str(
        "csv <name>  changes the stem.  csv reset  wipes these files.\ncsv on|off <actor>...  flag pops/firms.  csv off  clears flags.\nHeader mismatch (old layout or a new flag set) needs reset or a new stem.",
    );
    out
}

const CSV_USAGE: &str = "usage: csv [name]  or  csv reset  or  csv on|off <actor>...";

pub(crate) fn handle_csv_command(session: &mut Session, rest: &[&str]) -> String {
    if rest.is_empty() {
        return csv_status(session);
    }
    let verb = rest[0].to_ascii_lowercase();
    if verb == "reset" {
        if rest.len() != 1 {
            return CSV_USAGE.into();
        }
        return match reset_price_log(session) {
            Ok(()) => format!("wiped CSVs.\n{}", csv_status(session)),
            Err(err) => format!("csv reset failed: {err}"),
        };
    }
    if verb == "on" {
        return match flag_csv_actors(session, &rest[1..], true) {
            Ok(msg) => msg,
            Err(err) => err,
        };
    }
    if verb == "off" {
        if rest.len() == 1 {
            session.csv_pops.clear();
            session.csv_firms.clear();
            return format!("csv flags cleared.\n{}", csv_status(session));
        }
        return match flag_csv_actors(session, &rest[1..], false) {
            Ok(msg) => msg,
            Err(err) => err,
        };
    }
    if rest.len() != 1 {
        return CSV_USAGE.into();
    }
    match sanitize_csv_stem(rest[0]) {
        Ok(stem) => {
            session.csv_stem = stem;
            csv_status(session)
        }
        Err(err) => err,
    }
}

/// Flags or unflags roster pops and firms for `{stem}_pops.csv` / `{stem}_firms.csv`.
fn flag_csv_actors(session: &mut Session, rest: &[&str], on: bool) -> Result<String, String> {
    if rest.is_empty() {
        return Err(CSV_USAGE.into());
    }
    let mut tok = Tokens::new(rest);
    let mut names = Vec::new();
    while !tok.is_empty() {
        let actor = parse_actor(&mut tok)?;
        names.push(set_csv_flag(session, actor, on)?);
    }
    let verb = if on { "logging" } else { "stopped" };
    Ok(format!(
        "csv {verb} {}.\n{}",
        names.join(" "),
        csv_status(session)
    ))
}

/// Inserts or removes one roster pop/firm id from the CSV flag set.
fn set_csv_flag(session: &mut Session, actor: Actor, on: bool) -> Result<String, String> {
    let name = fmt_actor(actor);
    match actor {
        Actor::Pop(id) => {
            if !session.pops.iter().any(|pop| pop.id == id) {
                return Err(format!("{name} is not on the roster"));
            }
            if on {
                session.csv_pops.insert(id);
            } else {
                session.csv_pops.remove(&id);
            }
            Ok(name)
        }
        Actor::Firm(id) => {
            if !session.firms.iter().any(|firm| firm.id == id) {
                return Err(format!("{name} is not on the roster"));
            }
            if on {
                session.csv_firms.insert(id);
            } else {
                session.csv_firms.remove(&id);
            }
            Ok(name)
        }
        other => Err(format!(
            "csv on/off is pops and firms only (got {})",
            fmt_actor(other)
        )),
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
    let key = raw.to_ascii_lowercase();
    if key == "reset" || key == "on" || key == "off" {
        return Err(format!(
            "csv name '{raw}' is reserved. Use csv reset / csv on / csv off."
        ));
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
        &csv_path(&session.csv_stem, "trades"),
        &trade_csv_header(session),
        &[],
        true,
    )?;
    reset_actor_csv(session, "firms", session.csv_firms.is_empty(), &firm_csv_header(session))?;
    reset_actor_csv(session, "pops", session.csv_pops.is_empty(), &pop_csv_header(session))?;
    Ok(())
}

/// Rewrites a flagged actor CSV, or deletes it when nothing is flagged.
fn reset_actor_csv(
    session: &Session,
    kind: &str,
    skip: bool,
    header: &str,
) -> Result<(), String> {
    let path = csv_path(&session.csv_stem, kind);
    if skip {
        if path.exists() {
            fs::remove_file(&path).map_err(|err| format!("remove {}: {err}", path.display()))?;
        }
        return Ok(());
    }
    write_csv_file(&path, header, &[], true)
}

pub(crate) const MARKET_CSV_FIELDS: &[&str] = &["amv", "salability", "average_price"];
pub(crate) const FIRM_RECORD_FIELDS: &[&str] = &["profit", "sell_success"];
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
pub(crate) const POP_RECORD_FIELDS: &[&str] = &[
    "basic",
    "common",
    "luxury",
    "living_standard",
    "shop_fill",
    "income",
    "wealth",
    "liquid",
    "saved",
    "consumption",
    "pop_size",
    "labor",
];
pub(crate) const POP_CSV_FIELDS: &[&str] = &["quantity", "shop_target", "save_target", "consumed"];

pub(crate) fn append_price_log(session: &Session, report: &MarketDayReport) -> Result<(), String> {
    fs::create_dir_all(csv_dir()).map_err(|err| format!("create data/logs: {err}"))?;
    write_csv_file(
        &csv_path(&session.csv_stem, "market"),
        &market_csv_header(session),
        &[market_csv_row(session)],
        false,
    )?;
    write_csv_file(
        &csv_path(&session.csv_stem, "trades"),
        &trade_csv_header(session),
        &[trade_csv_row(session, report)],
        false,
    )?;
    if !session.csv_firms.is_empty() {
        write_csv_file(
            &csv_path(&session.csv_stem, "firms"),
            &firm_csv_header(session),
            &[firm_csv_row(session)],
            false,
        )?;
    }
    if !session.csv_pops.is_empty() {
        write_csv_file(
            &csv_path(&session.csv_stem, "pops"),
            &pop_csv_header(session),
            &[pop_csv_row(session)],
            false,
        )?;
    }
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
    for firm in session
        .firms
        .iter()
        .filter(|firm| session.csv_firms.contains(&firm.id))
    {
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
    for firm in session
        .firms
        .iter()
        .filter(|firm| session.csv_firms.contains(&firm.id))
    {
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

pub(crate) fn pop_csv_header(session: &Session) -> String {
    let mut cols = vec!["day".to_string()];
    let goods = market_csv_goods(session);
    for pop in session
        .pops
        .iter()
        .filter(|pop| session.csv_pops.contains(&pop.id))
    {
        let pop_name = csv_actor_col(Actor::Pop(pop.id));
        for field in POP_RECORD_FIELDS {
            cols.push(format!("{pop_name}_{field}"));
        }
        for id in &goods {
            let good_name = csv_good_col(*id);
            for field in POP_CSV_FIELDS {
                cols.push(format!("{pop_name}_{good_name}_{field}"));
            }
        }
    }
    cols.join(",")
}

pub(crate) fn pop_csv_row(session: &Session) -> String {
    let mut cells = vec![session.day.to_string()];
    let goods = market_csv_goods(session);
    for pop in session
        .pops
        .iter()
        .filter(|pop| session.csv_pops.contains(&pop.id))
    {
        cells.push(csv_num(pop.records.tier_sat[0]));
        cells.push(csv_num(pop.records.tier_sat[1]));
        cells.push(csv_num(pop.records.tier_sat[2]));
        cells.push(csv_num(pop.records.living_standard));
        cells.push(csv_num(pop.records.shop_fill));
        cells.push(csv_num(pop.records.income_amv));
        cells.push(csv_num(pop.records.wealth_amv));
        cells.push(csv_num(pop.records.liquid_wealth));
        cells.push(csv_num(pop.records.saved_amv));
        cells.push(csv_num(pop.records.consumption_amv));
        cells.push(csv_num(pop.records.pop_size));
        cells.push(csv_num(pop.records.labor));
        for id in &goods {
            match pop.property.get(id) {
                Some(row) => {
                    cells.push(csv_num(row.quantity));
                    cells.push(csv_num(row.shop_target));
                    cells.push(csv_num(row.save_target));
                    cells.push(csv_num(row.consumed));
                }
                None => {
                    for _ in POP_CSV_FIELDS {
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

#[cfg(test)]
mod csv_should {
    use super::*;

    fn session() -> Session {
        boot_session()
    }

    #[test]
    fn default_kind_brace_is_market_and_trades() {
        let session = session();
        assert_eq!(csv_kind_brace(&session), "{market,trades}");
        assert!(session.csv_firms.is_empty());
        assert!(session.csv_pops.is_empty());
    }

    #[test]
    fn pop_labels_include_specialty() {
        assert_eq!(fmt_actor(Actor::Pop(1)), "pop1-grain");
        assert_eq!(fmt_actor(Actor::Pop(18)), "pop18-bronze_mirror");
        assert_eq!(fmt_actor(Actor::Pop(28)), "pop28-time");
        assert_eq!(csv_actor_col(Actor::Pop(1)), "pop1_grain");
        assert_eq!(fmt_actor(Actor::Firm(1)), "firm1-grain");
        assert_eq!(fmt_actor(Actor::Firm(18)), "firm18-bronze_mirror");
        assert_eq!(fmt_actor(Actor::Firm(28)), "firm28-time");
        assert_eq!(csv_actor_col(Actor::Firm(1)), "firm1_grain");
    }

    #[test]
    fn flags_specialty_prefab_names() {
        let mut session = session();
        let msg = handle_csv_command(&mut session, &["on", "pop1-grain", "pop28-time"]);
        assert!(msg.contains("logging pop1-grain pop28-time"), "{msg}");
        assert!(session.csv_pops.contains(&1));
        assert!(session.csv_pops.contains(&28));
        let firm = handle_csv_command(&mut session, &["on", "firm1-grain"]);
        assert!(firm.contains("not on the roster"), "{firm}");
        assert!(session.csv_firms.is_empty());
    }

    #[test]
    fn flags_only_named_roster_actors() {
        let mut session = session();
        let msg = handle_csv_command(&mut session, &["on", "pop1", "pop2"]);
        assert!(msg.contains("logging pop1-grain pop2-water"), "{msg}");
        assert!(session.csv_pops.contains(&1));
        assert!(session.csv_pops.contains(&2));
        assert!(!session.csv_pops.contains(&3));
        assert!(session.csv_firms.is_empty());
        assert_eq!(csv_kind_brace(&session), "{market,trades,pops}");

        let pop_header = pop_csv_header(&session);
        assert!(
            pop_header.contains("pop1_grain_living_standard"),
            "{pop_header}"
        );
        assert!(
            pop_header.contains("pop2_water_living_standard"),
            "{pop_header}"
        );
        assert!(!pop_header.contains("pop3_"), "{pop_header}");

        let off = handle_csv_command(&mut session, &["off", "pop1"]);
        assert!(off.contains("stopped pop1-grain"), "{off}");
        assert!(!session.csv_pops.contains(&1));
        assert!(session.csv_pops.contains(&2));
        assert_eq!(csv_kind_brace(&session), "{market,trades,pops}");
    }

    #[test]
    fn off_with_no_actors_clears_all_flags() {
        let mut session = session();
        handle_csv_command(&mut session, &["on", "pop1", "pop2"]);
        assert!(!session.csv_pops.is_empty());
        let msg = handle_csv_command(&mut session, &["off"]);
        assert!(msg.contains("flags cleared"), "{msg}");
        assert!(session.csv_firms.is_empty());
        assert!(session.csv_pops.is_empty());
    }

    #[test]
    fn rejects_unknown_and_non_roster_actors() {
        let mut session = session();
        let unknown = handle_csv_command(&mut session, &["on", "ghost"]);
        assert!(unknown.contains("unknown actor"), "{unknown}");
        let missing = handle_csv_command(&mut session, &["on", "firm", "99"]);
        assert!(missing.contains("not on the roster"), "{missing}");
        let inst = handle_csv_command(&mut session, &["on", "inst", "1"]);
        assert!(inst.contains("pops and firms only"), "{inst}");
        assert!(session.csv_firms.is_empty());
        assert!(session.csv_pops.is_empty());
    }

    #[test]
    fn stem_reserves_on_off_reset() {
        assert!(sanitize_csv_stem("reset").is_err());
        assert!(sanitize_csv_stem("on").is_err());
        assert!(sanitize_csv_stem("off").is_err());
        assert_eq!(sanitize_csv_stem("run1").unwrap(), "run1");
    }
}

