//! Quick terminal probe for [`DemographicRates`] / [`Household::update`].
//!
//! Run (defaults: baseline rates, 50 years, print every year for first 10 then
//! every 10th year):
//!
//! ```text
//! cargo run --example rates_tester
//! cargo run --example rates_tester -- 100
//! cargo run --example rates_tester -- 50 --every 5
//! ```
//!
//! Prints the rate bundle, year-by-year household breakdown under
//! `Household::update`, one-step and multi-year growth, and a rough intrinsic
//! growth factor from the equal-sex linear projection (Leslie-style 2x2 on
//! child/adult bands).

use simpler_economy::game::household::{DemographicRates, Household};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let years = parse_years(&args).unwrap_or(50);
    let every = parse_flag_u32(&args, "--every").unwrap_or(0);

    let rates = DemographicRates::baseline();
    let start = Household::new();

    println!("=== DemographicRates probe ===\n");
    print_rates(&rates);
    println!();
    print_household("start (Household::new)", &start);
    println!();

    // One-step growth from the default household.
    let mut one = start;
    let total0 = one.total_count();
    one.update(&rates);
    let total1 = one.total_count();
    let step_g = if total0 > 0.0 {
        total1 / total0 - 1.0
    } else {
        f64::NAN
    };
    println!("--- One-step from Household::new ---");
    print_household("after 1 year", &one);
    println!(
        "  total people: {:.6} -> {:.6}  (g = {:+.4}%, {})",
        total0,
        total1,
        step_g * 100.0,
        sign_label(step_g)
    );
    println!();

    // Multi-year path using the real update.
    println!("--- Multi-year sim (Household::update, fixed rates) ---");
    println!(
        "{:>6}  {:>10}  {:>8}  {:>8}  {:>8}  {:>8}  {:>10}  {:>10}",
        "year", "count", "child", "adult", "elder", "size", "total", "g_total%"
    );

    let mut h = start;
    let mut prev_total = h.total_count();
    print_row(0, &h, f64::NAN);

    for year in 1..=years {
        if h.count < 1.0 {
            println!("  stopped: household count fell below 1.0 (update requires count >= 1)");
            break;
        }
        h.update(&rates);
        let total = h.total_count();
        let g = if prev_total > 0.0 {
            total / prev_total - 1.0
        } else {
            f64::NAN
        };
        let show = year <= 10
            || year == years
            || (every > 0 && year % every == 0)
            || (every == 0 && year % 10 == 0);
        if show {
            print_row(year, &h, g);
        }
        prev_total = total;
    }

    let final_total = h.total_count();
    let annualized = if years > 0 && total0 > 0.0 && final_total > 0.0 {
        (final_total / total0).powf(1.0 / years as f64) - 1.0
    } else {
        f64::NAN
    };
    println!();
    println!("--- Summary after {years} years ---");
    print_household("final averages", &h);
    println!(
        "  total people: {:.6} -> {:.6}",
        total0, final_total
    );
    println!(
        "  annualized total growth: {:+.4}%  ({})",
        annualized * 100.0,
        sign_label(annualized)
    );
    println!(
        "  household count: {:.6} -> {:.6}  (x{:.4})",
        start.count,
        h.count,
        h.count / start.count
    );
    println!();

    // Intrinsic growth from equal-sex linear child/adult projection.
    let (lambda, g_intr, sc, sa, se, live_birth_factor) = intrinsic_growth(&rates);
    println!("--- Intrinsic (equal-sex linear projection) ---");
    println!("  effective survivals (approx):");
    println!("    s_child = {sc:.6}  (m_c eff ~ {:.6})", 1.0 - sc);
    println!("    s_adult = {sa:.6}  (m_a eff ~ {:.6})", 1.0 - sa);
    println!("    s_elder = {se:.6}  (m_e eff ~ {:.6})", 1.0 - se);
    println!(
        "  live births per adult headcount (B/A): {:.6}",
        live_birth_factor
    );
    println!("  dominant lambda (child-adult 2x2): {:.6}", lambda);
    println!(
        "  intrinsic g = lambda - 1: {:+.4}%  ({})",
        g_intr * 100.0,
        sign_label(g_intr)
    );
    println!();
    println!(
        "Note: one-step g depends on the starting pyramid; intrinsic g is the"
    );
    println!(
        "long-run sign of these rates if held fixed (stable-population growth)."
    );
}

fn print_rates(r: &DemographicRates) {
    println!("DemographicRates::baseline():");
    println!("  birth_per_woman     = {:.6}", r.birth_per_woman);
    println!("  infant_mortality    = {:.6}", r.infant_mortality);
    println!("  maternal_mortality  = {:.6}", r.maternal_mortality);
    println!(
        "  child_mortality     = (total={:.6}, male={:.6}, female={:.6})",
        r.child_mortality.0, r.child_mortality.1, r.child_mortality.2
    );
    println!(
        "  adult_mortality     = (total={:.6}, male={:.6}, female={:.6})",
        r.adult_mortality.0, r.adult_mortality.1, r.adult_mortality.2
    );
    println!(
        "  elder_mortality     = (total={:.6}, male={:.6}, female={:.6})",
        r.elder_mortality.0, r.elder_mortality.1, r.elder_mortality.2
    );
    println!("  partnership_rate    = {:.6}", r.partnership_rate);
}

fn print_household(label: &str, h: &Household) {
    println!("{label}:");
    println!(
        "  count={:.6}  child={:.6}  adult={:.6}  elder={:.6}  size={:.6}",
        h.count,
        h.child,
        h.adult,
        h.elder,
        h.household_size()
    );
    println!(
        "  mf child/adult/elder = {:.4} / {:.4} / {:.4}  partnership_rate={:.4}",
        h.child_mf, h.adult_mf, h.elder_mf, h.partnership_rate
    );
    println!(
        "  totals C/A/E = {:.6} / {:.6} / {:.6}  people={:.6}",
        h.total_children(),
        h.total_adults(),
        h.total_elders(),
        h.total_count()
    );
}

fn print_row(year: u32, h: &Household, g: f64) {
    let g_s = if g.is_finite() {
        format!("{:+.4}", g * 100.0)
    } else {
        "   -".to_string()
    };
    println!(
        "{:>6}  {:>10.4}  {:>8.4}  {:>8.4}  {:>8.4}  {:>8.4}  {:>10.4}  {:>10}",
        year,
        h.count,
        h.child,
        h.adult,
        h.elder,
        h.household_size(),
        h.total_count(),
        g_s
    );
}

fn sign_label(g: f64) -> &'static str {
    if !g.is_finite() {
        "unknown"
    } else if g > 1e-9 {
        "NET POSITIVE"
    } else if g < -1e-9 {
        "NET NEGATIVE"
    } else {
        "NEAR ZERO"
    }
}

/// Equal-sex intrinsic growth from a Leslie-style child/adult map.
///
/// Approximates `Household::update` when sex ratios stay 0.5 and maternal
/// deaths are folded into effective adult survival:
///   s_a = 1 - m_a_total - 0.5*b*(1-i)*mm   (avg over both sexes)
/// which matches stacking male extra = maternal burden on women when sexes equal.
///
/// Returns (lambda, g, s_c, s_a, s_e, B_per_adult).
fn intrinsic_growth(r: &DemographicRates) -> (f64, f64, f64, f64, f64, f64) {
    let b = r.birth_per_woman.max(0.0);
    let i = r.infant_mortality.clamp(0.0, 1.0);
    let mm = r.maternal_mortality.clamp(0.0, 1.0);

    // Band rates: total + average of sex-specific extras (equal population shares).
    let m_c = (r.child_mortality.0 + 0.5 * (r.child_mortality.1 + r.child_mortality.2)).max(0.0);
    let m_e = (r.elder_mortality.0 + 0.5 * (r.elder_mortality.1 + r.elder_mortality.2)).max(0.0);

    // Adult survival: average of both sexes after band deaths + maternal on women.
    // remain_f/W = (1 - m_af) - b(1-i)mm, remain_m/W = (1 - m_am)
    // s_a = 1 - 0.5*(m_af + m_am) - 0.5*b(1-i)mm
    let m_af = (r.adult_mortality.0 + r.adult_mortality.2).max(0.0);
    let m_am = (r.adult_mortality.0 + r.adult_mortality.1).max(0.0);
    let s_a = (1.0 - 0.5 * (m_af + m_am) - 0.5 * b * (1.0 - i) * mm).clamp(0.0, 1.0);
    let s_c = (1.0 - m_c).clamp(0.0, 1.0);
    let s_e = (1.0 - m_e).clamp(0.0, 1.0);

    // Live births per adult: B/A = 0.5 * b * (1-i)
    let birth_per_adult = 0.5 * b * (1.0 - i);

    // C' = s_c*(19/20)*C + birth_per_adult * A
    // A' = s_c/20 * C + s_a*(39/40)*A
    let alpha = s_c * 19.0 / 20.0;
    let beta = birth_per_adult;
    let gamma = s_c / 20.0;
    let delta = s_a * 39.0 / 40.0;

    let tr = alpha + delta;
    let det = alpha * delta - beta * gamma;
    let disc = (tr * tr - 4.0 * det).max(0.0).sqrt();
    let lambda = 0.5 * (tr + disc);
    let g = lambda - 1.0;

    (lambda, g, s_c, s_a, s_e, birth_per_adult)
}

fn parse_years(args: &[String]) -> Option<u32> {
    args.iter()
        .find(|a| !a.starts_with('-'))
        .and_then(|s| s.parse().ok())
}

fn parse_flag_u32(args: &[String], flag: &str) -> Option<u32> {
    args.windows(2).find_map(|w| {
        if w[0] == flag {
            w[1].parse().ok()
        } else {
            None
        }
    })
}
