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
/// institutions, and nations.
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
    Flat { kind: SentimentKind, delta: f64 },
    /// Relative change to `kind`'s current share, then renormalize.
    Relative { kind: SentimentKind, relative: f64 },
    /// Transfer a share from one mood to another.
    Transfer {
        from: SentimentKind,
        to: SentimentKind,
        amount: f64,
    },
}

impl Default for Sentiment {
    /// # Default
    ///
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

    /// # Content
    ///
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

    /// # Even
    ///
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

    /// # From Parts
    ///
    /// Build from raw non-negative shares, then renormalize so they sum to 1.
    ///
    /// Debug-asserts each part is finite and `>= 0`, and that the total is
    /// positive. Does not accept negatives or an all-zero vector.
    pub fn from_parts(happiness: f64, contentment: f64, anger: f64, fear: f64, hope: f64) -> Self {
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

    /// # Happiness
    ///
    /// Share that is actively happy / pleased.
    pub fn happiness(&self) -> f64 {
        self.happiness
    }

    /// # Contentment
    ///
    /// Share that is quietly satisfied with the status quo.
    pub fn contentment(&self) -> f64 {
        self.contentment
    }

    /// # Anger
    ///
    /// Share that is angry / hostile.
    pub fn anger(&self) -> f64 {
        self.anger
    }

    /// # Fear
    ///
    /// Share that is fearful / anxious.
    pub fn fear(&self) -> f64 {
        self.fear
    }

    /// # Hope
    ///
    /// Share that is hopeful / aspirational.
    pub fn hope(&self) -> f64 {
        self.hope
    }

    /// # Total
    ///
    /// Sum of all emotion shares (≈ 1 for any published value).
    pub fn total(&self) -> f64 {
        self.happiness + self.contentment + self.anger + self.fear + self.hope
    }

    /// # Get
    ///
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

    /// # Renormalize
    ///
    /// Scale shares so the vector sums to 1.
    ///
    /// Precondition: shares are finite, non-negative, and total mass is
    /// positive. Does not clamp or invent a fallback.
    pub fn renormalize(&mut self) {
        self.debug_assert_renormalizable();
        let total = self.total();
        self.happiness /= total;
        self.contentment /= total;
        self.anger /= total;
        self.fear /= total;
        self.hope /= total;
    }

    /// # Normalized
    ///
    /// Renormalize and return by value.
    pub fn normalized(mut self) -> Self {
        self.renormalize();
        debug_assert!(self.is_valid());
        self
    }

    /// # Is Valid
    ///
    /// Published invariant: finite, non-negative, sum ≈ 1.
    pub fn is_valid(&self) -> bool {
        self.parts_nonneg_finite() && (self.total() - 1.0).abs() < 1e-6
    }

    /// # Parts Nonneg Finite
    ///
    /// True when every share is finite and `>= 0`.
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

    /// # Debug Assert Renormalizable
    ///
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
}

impl SentimentKind {
    /// # All
    ///
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
mod sentiment_should {
    use super::*;

    /// # Assert Unit
    ///
    /// The sentiment is valid and its shares sum to 1.
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
}
