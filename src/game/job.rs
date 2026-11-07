#![allow(dead_code)]

//! A pop's cottage work: the processes it can run and the day's quota.
//!
//! The stock is the pop's. The pop posts the offer.

/// # Job
///
/// The job of a pop: the work they do at home, with their own property.
///
/// As compared to a firm, this is disorganized work done by pops within their own home
/// and with their own property.
///
/// It's primary advantage is what it gives to the pop, subsistence and stability when
/// the wider market is underdeveloped. It's also highly stable in that it typically
/// supplies most of it's effective wages directly rather than needing to trade for them.
/// This makes it highly stable, even in shaky markets.
///
/// The disadvantages: It can't do directed research, simply pushing a bit of research everywhere
/// and quickly distributing it to the wider society, reducing it's competative
/// advantage. It can't separate pop consumption from the work fully. It has a higher
/// complexity penalty, as it's not unified.
///
/// ## Additional Notes
///
/// Two pops doing similar work still keep separate lines and targets.
/// The goods they hold stay on the pop.
#[derive(Debug, Clone)]
pub struct Job {
    /// Processes this pop can run, and the quota for each.
    ///
    /// If there are no processes, the job does not run.
    pub lines: [JobLine; 2],
}

/// One process the job can run, plus the day's quota.
///
/// Same shape as a firm's production line. `inputs` is the on/off list of
/// optional goods this line may draw. Required inputs are always drawn.
#[derive(Debug, Clone)]
pub struct JobLine {
    /// Process id in [`crate::game::factuals::Factuals`].
    pub process: usize,
    /// Iterations sought today.
    ///
    /// `None` means as many as the inputs on hand allow.
    /// `Some(0.0)` means do not run.
    /// `Some(n)` with `n > 0` means run up to `n` iterations.
    pub target: Option<f64>,
    /// Optional inputs this line draws. Empty draws none.
    pub inputs: Vec<usize>,
}

impl JobLine {
    /// # New
    ///
    /// A line for `process`.
    ///
    /// `target` is the day's iterations. `None` runs as many as inputs allow.
    /// `inputs` are the optional goods this line may draw. Required inputs
    /// are always included.
    pub fn new(process: usize, target: Option<f64>, inputs: Vec<usize>) -> Self {
        Self {
            process,
            target,
            inputs,
        }
    }
}

impl Job {
    /// # New
    ///
    /// A job with these lines.
    pub fn new(lines: [JobLine; 2]) -> Self {
        Self { lines }
    }

    /// # None
    ///
    /// A job with no lines.
    pub fn none() -> Self {
        Self::new([
            JobLine::new(0, None, Vec::new()),
            JobLine::new(0, None, Vec::new()),
        ])
    }
}
