/// # Sentiment
///
/// Political / social feeling of a population slice (typically one [`crate::game::pop::Pop`]).
/// Design name: **Population Sentiment** (EconCiv `Pops.md`).
///
/// Values are **shares of the pop** in each emotion and form a **unit partition**:
/// finite, non-negative, sum ≈ `1.0`. Every public constructor and mutator leaves a
/// [`Self::is_valid`] value. Fields are private so invalid states cannot be built
/// outside this module; only in-method intermediates may briefly leave the unit sum
/// before [`Self::renormalize`].
///
/// The same type can be **blended** by population weight for firms, markets,
/// institutions, and states.
///
/// ## Validation policy
///
/// **`debug_assert!`** for:
/// - finite scalars (`delta`, `relative`, `people_delta`, weights, shares)
/// - non-negative shares at construction / before renormalize
/// - positive population / blend weights (living-mass invariant)
/// - positive total mass before [`Self::renormalize`]
///
/// **Clamp / no-op** (still valid requests):
/// - relative scale ≤ −1 → that part becomes 0 (then renormalize remaining mass)
/// - transfer / shift larger than the donor → take only what exists
/// - same-axis transfer → no-op
/// - wiping the last remaining mass → result is [`Self::content`] (handled in the
///   mutator, not by “fixing” a broken value in renormalize)
///
/// ## Axes
///
/// | Accessor | Role |
/// |----------|------|
/// | [`Self::happiness`] | Active positive affect; loyalty-friendly, low unrest |
/// | [`Self::contentment`] | Calm status-quo comfort; stable, low energy |
/// | [`Self::anger`] | Active hostility; unrest / resistance fuel |
/// | [`Self::fear`] | Anxiety and flight risk; savings / migration pressure |
/// | [`Self::hope`] | Forward-looking optimism; reform / investment appetite |
///
/// These are a **partition**, not independent meters: moving people into one emotion
/// comes from the others.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sentiment {
    happiness: f64,
    contentment: f64,
    anger: f64,
    fear: f64,
    hope: f64,
}

/// Which emotion share to read or adjust.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SentimentKind {
    Happiness,
    Contentment,
    Anger,
    Fear,
    Hope,
}

/// A single sentiment change, for batch application via [`Sentiment::apply_mods`].
///
/// Focuses on the two common shapes used by desires / day effects:
/// - **Flat**: absolute share of the whole pop (donors unspecified)
/// - **Relative**: percent-of-part scale on one axis
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SentimentMod {
    /// Absolute share of the whole pop into (positive) or out of (negative) `kind`.
    /// Other axes scale proportionally — no donor named.
    Flat {
        kind: SentimentKind,
        delta: f64,
    },
    /// Relative change to `kind`'s current share, then renormalize.
    Relative {
        kind: SentimentKind,
        relative: f64,
    },
    /// Transfer a share from one mood to another.
    Transfer {
        from: SentimentKind,
        to: SentimentKind,
        amount: f64,
    },
}

impl Default for Sentiment {
    /// Fully content baseline (stable starting pops).
    fn default() -> Self {
        Self::content()
    }
}

impl Sentiment {
    /// # New
    ///
    /// Alias of [`Self::content`] — default political calm.
    pub fn new() -> Self {
        Self::content()
    }

    /// Entire pop is content (1.0 contentment, others 0).
    pub fn content() -> Self {
        let s = Self {
            happiness: 0.0,
            contentment: 1.0,
            anger: 0.0,
            fear: 0.0,
            hope: 0.0,
        };
        debug_assert!(s.is_valid());
        s
    }

    /// Even split across all five emotions (each `1/5`).
    pub fn even() -> Self {
        let s = 1.0 / 5.0;
        Self {
            happiness: s,
            contentment: s,
            anger: s,
            fear: s,
            hope: s,
        }
        // Already a unit partition; renormalize for float hygiene.
        .normalized()
    }

    /// Build from raw non-negative shares, then renormalize so they sum to 1.
    ///
    /// Debug-asserts each part is **finite and ≥ 0**, and that the total is
    /// **positive**. Does not accept negatives or an all-zero vector.
    pub fn from_parts(
        happiness: f64,
        contentment: f64,
        anger: f64,
        fear: f64,
        hope: f64,
    ) -> Self {
        let s = Self {
            happiness,
            contentment,
            anger,
            fear,
            hope,
        };
        s.debug_assert_renormalizable();
        s.normalized()
    }

    /// Share that is actively happy / pleased.
    pub fn happiness(&self) -> f64 {
        self.happiness
    }
    /// Share that is quietly satisfied with the status quo.
    pub fn contentment(&self) -> f64 {
        self.contentment
    }
    /// Share that is angry / hostile.
    pub fn anger(&self) -> f64 {
        self.anger
    }
    /// Share that is fearful / anxious.
    pub fn fear(&self) -> f64 {
        self.fear
    }
    /// Share that is hopeful / aspirational.
    pub fn hope(&self) -> f64 {
        self.hope
    }

    /// Sum of all emotion shares (≈ 1 for any published value).
    pub fn total(&self) -> f64 {
        self.happiness + self.contentment + self.anger + self.fear + self.hope
    }

    /// Read one axis.
    pub fn get(&self, kind: SentimentKind) -> f64 {
        match kind {
            SentimentKind::Happiness => self.happiness,
            SentimentKind::Contentment => self.contentment,
            SentimentKind::Anger => self.anger,
            SentimentKind::Fear => self.fear,
            SentimentKind::Hope => self.hope,
        }
    }

    /// Write one axis without renormalizing. Value must be finite and ≥ 0.
    fn set_raw(&mut self, kind: SentimentKind, value: f64) {
        debug_assert!(value.is_finite(), "Sentiment share must be finite.");
        debug_assert!(value >= 0.0, "Sentiment share must be non-negative.");
        match kind {
            SentimentKind::Happiness => self.happiness = value,
            SentimentKind::Contentment => self.contentment = value,
            SentimentKind::Anger => self.anger = value,
            SentimentKind::Fear => self.fear = value,
            SentimentKind::Hope => self.hope = value,
        }
    }

    /// # Renormalize
    ///
    /// Scale shares so the vector sums to 1.
    ///
    /// **Precondition:** shares are finite, non-negative, and total mass is
    /// positive (a renormalizable intermediate). Does not clamp or invent a
    /// fallback — callers must only pass valid mass.
    pub fn renormalize(&mut self) {
        self.debug_assert_renormalizable();
        let total = self.total();
        self.happiness /= total;
        self.contentment /= total;
        self.anger /= total;
        self.fear /= total;
        self.hope /= total;
    }

    /// Renormalize and return by value (fluent).
    pub fn normalized(mut self) -> Self {
        self.renormalize();
        debug_assert!(self.is_valid());
        self
    }

    /// Published invariant: finite, non-negative, sum ≈ 1.
    pub fn is_valid(&self) -> bool {
        self.parts_nonneg_finite() && (self.total() - 1.0).abs() < 1e-6
    }

    fn parts_nonneg_finite(&self) -> bool {
        [
            self.happiness,
            self.contentment,
            self.anger,
            self.fear,
            self.hope,
        ]
        .iter()
        .all(|v| v.is_finite() && *v >= 0.0)
    }

    /// Intermediate state safe to renormalize: non-negative finite parts, total > 0.
    fn debug_assert_renormalizable(&self) {
        for (name, v) in [
            ("happiness", self.happiness),
            ("contentment", self.contentment),
            ("anger", self.anger),
            ("fear", self.fear),
            ("hope", self.hope),
        ] {
            debug_assert!(v.is_finite(), "Sentiment.{name} must be finite (got {v}).");
            debug_assert!(v >= 0.0, "Sentiment.{name} must be non-negative (got {v}).");
        }
        debug_assert!(
            self.total() > f64::EPSILON,
            "Sentiment mass must be positive before renormalize (total={}).",
            self.total()
        );
    }

    // -----------------------------------------------------------------------
    // Modifications
    // -----------------------------------------------------------------------

    /// # Adjust Global Share
    ///
    /// Change one emotion by a **fraction of the whole pop** (absolute share).
    ///
    /// Example: `adjust_global_share(Anger, 0.05)` moves 5% of the entire pop into
    /// anger, taking proportionally from the other emotions. Negative `delta`
    /// moves people *out* of that emotion into the others (by their relative weight).
    ///
    /// `delta == 0.0` is a no-op. Non-finite `delta` debug-asserts. Oversize moves
    /// are clamped to available mass on the source side.
    ///
    /// Requires a valid [`Sentiment`].
    pub fn adjust_global_share(&mut self, kind: SentimentKind, delta: f64) {
        debug_assert!(self.is_valid(), "adjust_global_share requires a valid Sentiment.");
        debug_assert!(delta.is_finite(), "Delta must be finite.");
        if delta == 0.0 {
            return;
        }
        if delta > 0.0 {
            self.shift_in(kind, delta);
        } else {
            self.shift_out(kind, -delta);
        }
        self.renormalize();
        debug_assert!(self.is_valid(), "Sentiment shares must form a unit partition.");
    }

    /// # Add Share
    ///
    /// Grow (or shrink, if `delta` is negative) one emotion by an absolute fraction
    /// of the **whole pop**, without naming which other moods supply the mass.
    ///
    /// Alias of [`Self::adjust_global_share`] for call sites that only care about
    /// “more of X,” not transfers between named pairs.
    pub fn add_share(&mut self, kind: SentimentKind, delta: f64) {
        self.adjust_global_share(kind, delta);
    }

    /// # Apply Mods Simultaniously
    /// 
    /// Given a list of mods, it applies all of them to the sentiment at once.
    /// 
    /// - `Flat` are added to to the base.
    /// - `Relative` multiply against the base then applies that to the output.
    /// 
    /// After modifiers are collected and applied, negative shares are clamped to 0.
    /// If the result is all 0s, then we return `Self::Content` as a fallback.
    /// Otherwise, it renormalizes the sentiment.
    /// 
    /// This is intended for batch applications of modifications, as applying 
    /// sequentially can lead to unintended interactions.
    pub fn apply_mods_simultaniously(&mut self, mods: impl IntoIterator<Item = SentimentMod>) {
        debug_assert!(
            self.is_valid(),
            "apply_mods_simultanious requires a valid Sentiment."
        );

        // get current shares
        let mut net = [0.0_f64; 5];
        for m in mods {
            match m {
                SentimentMod::Flat { kind, delta } => {
                    net[Self::kind_index(kind)] += delta;
                }
                SentimentMod::Relative { kind, relative } => {
                    let current = self.get(kind);
                    net[Self::kind_index(kind)] += current * relative;
                }
                SentimentMod::Transfer { from, to, amount } => {
                    net[Self::kind_index(from)] -= amount;
                    net[Self::kind_index(to)] += amount;
                }
            }
        }

        // apply net and clamp negatives.
        self.happiness = (self.happiness + net[0]).max(0.0);
        self.contentment = (self.contentment + net[1]).max(0.0);
        self.anger = (self.anger + net[2]).max(0.0);
        self.fear = (self.fear + net[3]).max(0.0);
        self.hope = (self.hope + net[4]).max(0.0);

        // if all 0s, fallback to content.
        if self.total() <= f64::EPSILON {
            *self = Self::content();
        }

        // renormalize
        self.renormalize();

        debug_assert!(
            self.is_valid(),
            "Sentiment shares must form a unit partition after apply_mods_simultaniously."
        );
    }

    #[inline]
    fn kind_index(kind: SentimentKind) -> usize {
        match kind {
            SentimentKind::Happiness => 0,
            SentimentKind::Contentment => 1,
            SentimentKind::Anger => 2,
            SentimentKind::Fear => 3,
            SentimentKind::Hope => 4,
        }
    }

    /// Apply one flat or relative modifier.
    pub fn apply_mod(&mut self, m: SentimentMod) {
        match m {
            SentimentMod::Flat { kind, delta } => self.adjust_global_share(kind, delta),
            SentimentMod::Relative { kind, relative } => {
                self.adjust_part_relative(kind, relative)
            }
            SentimentMod::Transfer { from, to, amount } => {
                self.transfer(from, to, amount);
            },
        }
    }

    /// Apply several modifiers in order (each step leaves a valid partition).
    pub fn apply_mods(&mut self, mods: impl IntoIterator<Item = SentimentMod>) {
        for m in mods {
            self.apply_mod(m);
        }
    }

    /// # Adjust Part Relative
    ///
    /// Change one emotion by a **fraction of its current share** (percent-of-part).
    ///
    /// Example: if anger is `0.20` and `relative` is `0.50`, anger becomes
    /// `0.20 * 1.5 = 0.30` before renormalization (then the whole vector is scaled
    /// back to sum 1). `relative = -0.25` shrinks that part by 25%.
    ///
    /// Debug-asserts `relative` is finite. A total reduction of 1 or more
    /// (`relative <= -1`) sets that part to **0**. If that removes all mass,
    /// the result is [`Self::content`] (still a valid published sentiment).
    ///
    /// Requires a valid [`Sentiment`].
    pub fn adjust_part_relative(&mut self, kind: SentimentKind, relative: f64) {
        debug_assert!(self.is_valid(), "adjust_part_relative requires a valid Sentiment.");
        debug_assert!(relative.is_finite(), "Relative must be finite.");
        let current = self.get(kind);
        // Full wipe and beyond: clamp to 0 rather than rejecting the call.
        let next = (current * (1.0 + relative)).max(0.0);
        self.set_raw(kind, next);
        if self.total() <= f64::EPSILON {
            // Wiped the only remaining mass — still emit a valid published value.
            *self = Self::content();
        } else {
            self.renormalize();
        }
        debug_assert!(self.is_valid(), "Sentiment shares must form a unit partition.");
    }

    /// # Adjust By People
    ///
    /// Move a **headcount** into or out of an emotion, given the pop's size.
    ///
    /// `people_delta / population` is applied as a global share via
    /// [`Self::adjust_global_share`]. Positive `people_delta` grows that emotion.
    ///
    /// Debug-asserts `people_delta` and `population` are finite and
    /// `population > 0` (living-pop invariant).
    pub fn adjust_by_people(&mut self, kind: SentimentKind, people_delta: f64, population: f64) {
        debug_assert!(people_delta.is_finite(), "People delta must be finite.");
        debug_assert!(population.is_finite(), "Population must be finite.");
        debug_assert!(population > 0.0, "Population must be positive.");
        self.adjust_global_share(kind, people_delta / population);
    }

    /// Transfer an absolute share from one emotion directly into another.
    ///
    /// `amount` is a **fraction of the whole pop** (e.g. `0.1` = 10%), clamped to
    /// the donor's available share. Debug-asserts `amount` is finite and
    /// non-negative. Same-axis transfer is a no-op. `amount == 0` is a no-op.
    /// Transfer preserves total mass, so renormalize is float hygiene.
    pub fn transfer(&mut self, from: SentimentKind, to: SentimentKind, amount: f64) {
        debug_assert!(self.is_valid(), "transfer requires a valid Sentiment.");
        debug_assert!(amount.is_finite(), "Transfer amount must be finite.");
        debug_assert!(amount >= 0.0, "Transfer amount must be non-negative.");
        if from == to || amount == 0.0 {
            return;
        }
        let take = amount.min(self.get(from));
        self.set_raw(from, self.get(from) - take);
        self.set_raw(to, self.get(to) + take);
        self.renormalize();
        debug_assert!(self.is_valid(), "Sentiment shares must form a unit partition.");
    }

    /// # Transfer People
    ///
    /// Move a **headcount** from one emotion into another, given the pop's size.
    ///
    /// Converts `people / population` into a whole-pop share and applies
    /// [`Self::transfer`]. Clamped to how many people are actually in the donor
    /// state (cannot move more than that share holds).
    ///
    /// Example: with 100 people, 40% content and 10% hopeful, `transfer_people(
    /// Contentment, Hope, 15.0, 100.0)` moves 15 people → +0.15 hope, −0.15 contentment.
    ///
    /// Debug-asserts `people` and `population` are finite, `people >= 0`, and
    /// `population > 0`. Same-axis / zero people are no-ops via [`Self::transfer`].
    pub fn transfer_people(
        &mut self,
        from: SentimentKind,
        to: SentimentKind,
        people: f64,
        population: f64,
    ) {
        debug_assert!(people.is_finite(), "People count must be finite.");
        debug_assert!(people >= 0.0, "People count must be non-negative.");
        debug_assert!(population.is_finite(), "Population must be finite.");
        debug_assert!(population > 0.0, "Population must be positive.");
        self.transfer(from, to, people / population);
    }

    // -----------------------------------------------------------------------
    // Consolidation
    // -----------------------------------------------------------------------

    /// # Blend
    ///
    /// Population-weighted average of several sentiments (for markets, firms,
    /// institutions, etc.). Weights are typically household counts or headcounts.
    ///
    /// Debug-asserts every weight is finite and **positive**, every input is
    /// already valid, and that at least one pair is provided.
    pub fn blend(parts: impl IntoIterator<Item = (Sentiment, f64)>) -> Self {
        let mut acc_h = 0.0;
        let mut acc_c = 0.0;
        let mut acc_a = 0.0;
        let mut acc_f = 0.0;
        let mut acc_p = 0.0;
        let mut wsum = 0.0;
        let mut any = false;
        for (s, w) in parts {
            any = true;
            debug_assert!(w.is_finite(), "Blend weight must be finite.");
            debug_assert!(w > 0.0, "Blend weight must be positive (got {w}).");
            debug_assert!(
                s.is_valid(),
                "Blend input sentiment must already be a valid unit partition."
            );
            acc_h += s.happiness * w;
            acc_c += s.contentment * w;
            acc_a += s.anger * w;
            acc_f += s.fear * w;
            acc_p += s.hope * w;
            wsum += w;
        }
        debug_assert!(any, "Blend requires at least one (Sentiment, weight) pair.");
        debug_assert!(wsum > f64::EPSILON, "Blend weight sum must be positive.");
        // Weighted average of unit partitions is unit; construct via from_parts path.
        Self::from_parts(acc_h / wsum, acc_c / wsum, acc_a / wsum, acc_f / wsum, acc_p / wsum)
    }

    // -----------------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------------

    /// # Shift In
    /// 
    /// Shifts percentage `amount` of the total weight into the given `kind`.
    /// 
    /// Does this by adding `amount` to the given kind, then scaling the rest down 
    /// relative to their current weight. While it should be close to 1.0 total, it may
    /// be off due to floating point error, so it is recommended to call 
    /// [`Self::renormalize`] after this.
    fn shift_in(&mut self, kind: SentimentKind, amount: f64) {
        debug_assert!(amount.is_finite(), "shift_in amount must be finite.");
        debug_assert!(amount >= 0.0, "shift_in amount must be non-negative.");
        if amount == 0.0 {
            return;
        }
        let others = self.total() - self.get(kind);
        if others <= f64::EPSILON {
            self.set_raw(kind, self.get(kind) + amount);
            return;
        }
        let take = amount.min(others);
        let scale = 1.0 - take / others;
        for k in SentimentKind::ALL {
            if k == kind {
                continue;
            }
            self.set_raw(k, self.get(k) * scale);
        }
        self.set_raw(kind, self.get(kind) + take);
    }

    /// # Shift Out
    /// 
    /// Shifts percentage `amount` of the total weight out of the given `kind`.
    /// 
    /// Does this by subtracting `amount` from the given kind, then scaling the rest up 
    /// relative to their current weight. While it should be close to 1.0 total, it may
    /// be off due to floating point error, so it is recommended to call 
    /// [`Self::renormalize`] after this.
    fn shift_out(&mut self, kind: SentimentKind, amount: f64) {
        debug_assert!(amount.is_finite(), "shift_out amount must be finite.");
        debug_assert!(amount >= 0.0, "shift_out amount must be non-negative.");
        let have = self.get(kind);
        let give = amount.min(have);
        if give <= f64::EPSILON {
            return;
        }
        self.set_raw(kind, have - give);
        let others = self.total() - self.get(kind);
        if others <= f64::EPSILON {
            let each = give / 4.0;
            for k in SentimentKind::ALL {
                if k != kind {
                    self.set_raw(k, self.get(k) + each);
                }
            }
        } else {
            let scale = 1.0 + give / others;
            for k in SentimentKind::ALL {
                if k == kind {
                    continue;
                }
                self.set_raw(k, self.get(k) * scale);
            }
        }
    }
}

impl SentimentKind {
    /// All axes in a stable order.
    pub const ALL: [SentimentKind; 5] = [
        SentimentKind::Happiness,
        SentimentKind::Contentment,
        SentimentKind::Anger,
        SentimentKind::Fear,
        SentimentKind::Hope,
    ];
}

#[cfg(test)]
mod sentiment_tests {
    use super::*;

    fn assert_unit(s: &Sentiment) {
        assert!(s.is_valid(), "invalid sentiment: {s:?} total={}", s.total());
        assert!((s.total() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn new_is_fully_content() {
        let s = Sentiment::new();
        assert_eq!(s.contentment(), 1.0);
        assert_unit(&s);
    }

    #[test]
    fn from_parts_renormalizes() {
        let s = Sentiment::from_parts(1.0, 1.0, 0.0, 0.0, 0.0);
        assert!((s.happiness() - 0.5).abs() < 1e-9);
        assert!((s.contentment() - 0.5).abs() < 1e-9);
        assert_unit(&s);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "non-negative")]
    fn from_parts_rejects_negative_share() {
        let _ = Sentiment::from_parts(-0.1, 1.0, 0.0, 0.0, 0.0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "positive")]
    fn from_parts_rejects_all_zero() {
        let _ = Sentiment::from_parts(0.0, 0.0, 0.0, 0.0, 0.0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "must be finite")]
    fn from_parts_rejects_nan() {
        let _ = Sentiment::from_parts(f64::NAN, 1.0, 0.0, 0.0, 0.0);
    }

    #[test]
    fn adjust_global_share_moves_absolute_fraction() {
        let mut s = Sentiment::content();
        s.adjust_global_share(SentimentKind::Anger, 0.10);
        assert!((s.anger() - 0.10).abs() < 1e-9);
        assert!((s.contentment() - 0.90).abs() < 1e-9);
        assert_unit(&s);

        s.adjust_global_share(SentimentKind::Happiness, 0.10);
        assert!((s.happiness() - 0.10).abs() < 1e-9);
        assert!((s.anger() - 0.10 * 0.9).abs() < 1e-9);
        assert_unit(&s);
    }

    #[test]
    fn adjust_part_relative_scales_one_component() {
        let mut s = Sentiment::from_parts(0.0, 0.5, 0.5, 0.0, 0.0);
        s.adjust_part_relative(SentimentKind::Anger, 1.0);
        assert!((s.anger() - 2.0 / 3.0).abs() < 1e-9);
        assert!((s.contentment() - 1.0 / 3.0).abs() < 1e-9);
        assert_unit(&s);
    }

    #[test]
    fn adjust_part_relative_full_wipe_clamps_to_zero() {
        let mut s = Sentiment::from_parts(0.0, 0.5, 0.5, 0.0, 0.0);
        s.adjust_part_relative(SentimentKind::Anger, -1.0);
        assert!((s.anger() - 0.0).abs() < 1e-9);
        assert!((s.contentment() - 1.0).abs() < 1e-9);
        assert_unit(&s);

        let mut s2 = Sentiment::from_parts(0.0, 0.5, 0.5, 0.0, 0.0);
        s2.adjust_part_relative(SentimentKind::Anger, -2.5);
        assert!((s2.anger() - 0.0).abs() < 1e-9);
        assert!((s2.contentment() - 1.0).abs() < 1e-9);
        assert_unit(&s2);

        // Wiping the only mass → still a valid published sentiment (content).
        let mut s3 = Sentiment::content();
        s3.adjust_part_relative(SentimentKind::Contentment, -1.0);
        assert_eq!(s3, Sentiment::content());
        assert_unit(&s3);
    }

    #[test]
    fn adjust_by_people_uses_population_size() {
        let mut s = Sentiment::content();
        s.adjust_by_people(SentimentKind::Fear, 25.0, 100.0);
        assert!((s.fear() - 0.25).abs() < 1e-9);
        assert!((s.contentment() - 0.75).abs() < 1e-9);
        assert_unit(&s);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Population must be positive")]
    fn adjust_by_people_rejects_non_positive_population() {
        let mut s = Sentiment::content();
        s.adjust_by_people(SentimentKind::Anger, 10.0, 0.0);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "must be finite")]
    fn adjust_global_share_rejects_nan_delta() {
        let mut s = Sentiment::content();
        s.adjust_global_share(SentimentKind::Anger, f64::NAN);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "must be finite")]
    fn adjust_part_relative_rejects_nan_relative() {
        let mut s = Sentiment::content();
        s.adjust_part_relative(SentimentKind::Anger, f64::NAN);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "positive")]
    fn blend_rejects_non_positive_weight() {
        let _ = Sentiment::blend([(Sentiment::content(), 0.0)]);
    }

    #[test]
    fn transfer_moves_between_axes() {
        let mut s = Sentiment::from_parts(0.0, 0.8, 0.2, 0.0, 0.0);
        s.transfer(SentimentKind::Contentment, SentimentKind::Hope, 0.3);
        assert!((s.hope() - 0.3).abs() < 1e-9);
        assert!((s.contentment() - 0.5).abs() < 1e-9);
        assert!((s.anger() - 0.2).abs() < 1e-9);
        assert_unit(&s);
    }

    #[test]
    fn transfer_same_axis_is_noop() {
        let mut s = Sentiment::from_parts(0.0, 0.8, 0.2, 0.0, 0.0);
        let before = s;
        s.transfer(SentimentKind::Contentment, SentimentKind::Contentment, 0.1);
        assert_eq!(s, before);
    }

    #[test]
    fn transfer_overshoot_clamps_to_donor() {
        let mut s = Sentiment::from_parts(0.0, 0.3, 0.7, 0.0, 0.0);
        s.transfer(SentimentKind::Contentment, SentimentKind::Hope, 5.0);
        assert!((s.contentment() - 0.0).abs() < 1e-9);
        assert!((s.hope() - 0.3).abs() < 1e-9);
        assert_unit(&s);
    }

    #[test]
    fn transfer_people_moves_headcount() {
        // 40 content, 10 hope, rest anger — 100 people.
        let mut s = Sentiment::from_parts(0.0, 0.40, 0.50, 0.0, 0.10);
        s.transfer_people(SentimentKind::Contentment, SentimentKind::Hope, 15.0, 100.0);
        // 15/100 = 0.15 share moved.
        assert!((s.contentment() - 0.25).abs() < 1e-9);
        assert!((s.hope() - 0.25).abs() < 1e-9);
        assert!((s.anger() - 0.50).abs() < 1e-9);
        assert_unit(&s);
    }

    #[test]
    fn transfer_people_clamps_to_donor_headcount() {
        // Only 10 people equivalent in contentment (0.10 of 100).
        let mut s = Sentiment::from_parts(0.0, 0.10, 0.90, 0.0, 0.0);
        s.transfer_people(SentimentKind::Contentment, SentimentKind::Fear, 50.0, 100.0);
        assert!((s.contentment() - 0.0).abs() < 1e-9);
        assert!((s.fear() - 0.10).abs() < 1e-9);
        assert_unit(&s);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Population must be positive")]
    fn transfer_people_rejects_non_positive_population() {
        let mut s = Sentiment::content();
        s.transfer_people(SentimentKind::Contentment, SentimentKind::Anger, 1.0, 0.0);
    }

    #[test]
    fn blend_weights_by_population() {
        let calm = Sentiment::content();
        let mut mad = Sentiment::content();
        mad.adjust_global_share(SentimentKind::Anger, 1.0);

        let blended = Sentiment::blend([(calm, 75.0), (mad, 25.0)]);
        assert!((blended.anger() - 0.25).abs() < 1e-9);
        assert!((blended.contentment() - 0.75).abs() < 1e-9);
        assert_unit(&blended);
    }

    #[test]
    fn negative_global_share_releases_to_others() {
        let mut s = Sentiment::from_parts(0.2, 0.2, 0.6, 0.0, 0.0);
        s.adjust_global_share(SentimentKind::Anger, -0.2);
        assert!((s.anger() - 0.4).abs() < 1e-9);
        assert!((s.happiness() - 0.3).abs() < 1e-9);
        assert!((s.contentment() - 0.3).abs() < 1e-9);
        assert_unit(&s);
    }

    #[test]
    fn add_share_matches_adjust_global_share() {
        let mut a = Sentiment::content();
        let mut b = Sentiment::content();
        a.add_share(SentimentKind::Hope, 0.2);
        b.adjust_global_share(SentimentKind::Hope, 0.2);
        assert_eq!(a, b);
        assert_unit(&a);
    }

    #[test]
    fn apply_mods_batch_flat_and_relative() {
        let mut s = Sentiment::from_parts(0.0, 1.0, 0.0, 0.0, 0.0);
        s.apply_mods([
            SentimentMod::Flat {
                kind: SentimentKind::Anger,
                delta: 0.20,
            },
            SentimentMod::Relative {
                kind: SentimentKind::Anger,
                relative: 0.50, // 0.20 * 1.5 = 0.30 before renorm against rest
            },
        ]);
        // After flat: anger 0.2, content 0.8
        // After relative *1.5 on anger: anger 0.3, content 0.8 → renorm → 0.3/1.1, 0.8/1.1
        assert!((s.anger() - 0.3 / 1.1).abs() < 1e-9);
        assert!((s.contentment() - 0.8 / 1.1).abs() < 1e-9);
        assert_unit(&s);
    }
}
