/// # Demand Curves
/// 
/// Defines how valuation of a unit of desire changes over time.
/// Value *SHOULD* always  declines in value.
/// The value of a unit is done stepwise. In 1 unit (step) increments,
/// partial steps give partial value (multiply step by amonut below full step).
/// 
/// Start is given by the Desire, as such the value at 0 should ALWAYS be equal to
/// start.
/// 
/// Steps are fenceposted at 0, so if a desire has 5 steps, it reaches step 4.
/// 
/// Values can be negative, allowing for desires to become
/// 
/// ## Available Curves.
/// 
/// Linear: Slope is the rate of decline, a fixed quantity between steps.
/// Start + slope * N
/// 
/// Geometric: Factor is the multiplier that is applied repeatedly. Factor must be 
/// between 0.0 and 1.0. Because of it's form, it will never cross 0, and will always 
/// result in a positive or negative value for any particular curve.
/// While this does mean that a Negative start value produces an 'increasing' curve,
/// as it is always negative, we consider this to be acceptable.
/// Start * Factor^N
/// 
/// Logarithmic: Factor is the base of the logarithm. Base must be greater than 1,
/// and it is advised to keep the value between 1.0 and 10.0.
/// Start - log_factor (N)
/// 
/// Root: Factor is the multiplier outside of our squore root. It should be a positive
/// value.
/// Start - (factor * sqrt(n))
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DemandCurve {
    /// Linear Value Function. 
    /// start + slope * n
    /// Slope should be Negative Value.
    /// 
    /// n =  start / slope
    Linear{slope: f64},
    /// Geometric Value Function
    /// (Factor) ^ (n) - 1 + start
    /// 
    /// Factor should be between 0.0 and 1.0 exclusive.
    /// 
    /// Never reaches 0.
    /// 
    /// ## Note: Due to the realities of math, this cannot go below 0, and will
    /// result in an increasing value, if the start is negative.
    Geometric{factor: f64},
    /// Logarithmic Value Function
    /// start - Log_factor (n)
    /// 
    /// Factor must be greater than 1.0. Advised to keep value between 1.0 and 10.0, 
    /// with 2.0-4.0 being ideal I think.
    /// 
    /// 0 when n = factor ^ start
    Logarithmic{factor: f64},
    /// Square root Value Function
    /// start - factor * x^(1/2)
    /// 
    /// Factor must be a Positive Value.
    /// 
    /// 0 = (start / factor)^2
    Root{factor: f64},
}

impl DemandCurve {
    /// # Linear
    /// 
    /// Safely creates linear DemandCurve.
    pub fn linear(slope: f64) -> Self {
        assert!(slope < 0.0, "Slope must be a Negative value!");
        Self::Linear{slope}
    }

    /// # Root
    /// 
    /// Safely creates a Root Demand Curve.
    pub fn root(factor: f64) -> Self {
        assert!(factor > 0.0, "Factor must be a positive value!");
        Self::Root{factor}
    }

    /// # Geometric
    /// 
    /// Safely creates Geometric Value Function.
    pub fn geometric(factor: f64) -> Self {
        assert!(1.0 > factor && factor > 0.0, "Factor must be between 0.0 and 1.0 exclusive! value!");
        Self::Geometric{factor}
    }

    /// # Logarithmic
    /// 
    /// Safely creates a Logarithmic Value Function.
    pub fn logarithmic(factor: f64) -> Self {
        assert!(factor > 1.0, "Factor must be greater than 1.0");
        Self::Logarithmic { factor }
    }

    /// # Value
    /// 
    /// Calculates and returns the current priority value of the desire
    /// 
    /// F(n-1) + start. (Step 1 should be at the 'start' value.)
    /// 
    /// n is the step the function is currently on.
    pub fn value(&self, start: f64, n: f64) -> f64 {
        match self {
            DemandCurve::Linear { slope } => {
                start + slope * n
            },
            DemandCurve::Root { factor} => {
                start - factor * (n).powf(0.5)
            },
            DemandCurve::Geometric { factor } => {
                start * (factor).powf(n)
            },
            DemandCurve::Logarithmic { factor } => {
                start - (n + 1.0).log(*factor)
            }
        }
    }

    /// # Total Value
    /// 
    /// Total Valuation function.
    /// 
    /// Gets the total value by summing over the steps, with a last fractional step 
    /// being of fractional value.
    /// 
    /// This works from a left handed approximation. 
    /// If we have 5 steps, then we sum the values of 0-4. If the 
    /// upper end is incomplete, then we take the final partial step and multiply it
    /// by the amount satisfied. eg, 5.5 sums 0-4 and 0.5 of 5.
    /// 
    /// ## Note
    /// 
    /// Root and Asymptotic are not tested, because that's complicated.
    pub fn total_value(&self, start: f64, steps: f64) -> f64 {
        let mut remainder = steps;
        let mut current_step = 0.0;
        let mut acc = 0.0;
        loop {
            // get the value produced at that step.
            if remainder < 1.0 { // if below 1, reduce by that factor.
                acc += self.value(start, current_step) * remainder;
            } else { // otherwise, only do 1.0.
                acc += self.value(start, current_step);
            }
            // get the next step (always go a whole step)
            current_step += 1.0;
            // with value gotten, reduce and check we got below 0.0
            remainder -= 1.0;
            if remainder < 0.0 {
                break;
            }
        }
        // return the accumulation
        acc
    }

    /// # Inverse
    /// 
    /// Given a priority, it returns the step it's on.
    pub fn inverse(&self, start: f64, value: f64) -> f64 {
        match self {
            DemandCurve::Linear { slope } => {
                (value - start) / slope
            },
            DemandCurve::Root { factor } => {
                (start - value).powf(2.0) / factor
            },
            DemandCurve::Geometric { factor } => {
                (value / start).log(*factor)
            },
            DemandCurve::Logarithmic { factor } => {
                factor.powf(start - value) - 1.0
            },
        }
    }

    /// # Derivative
    /// 
    /// Calculates the derivative/slope of the function at a particular point.
    /// 
    /// NOTE: Not used currently. No real use for it.
    pub fn derivative(&self, step: f64) -> f64 {
        match self {
            DemandCurve::Linear { slope } => {
                *slope
            },
            DemandCurve::Root { factor: accel } => {
                2.0 * accel * step
            },
            DemandCurve::Geometric { factor } => {
                factor * (1.0 + step).ln() * (1.0 * step).powf(factor * step)
            },
            DemandCurve::Logarithmic { factor } => todo!(),
        }
    }

    /// # Arc Length
    /// 
    /// Arc Length Calculator.
    /// 
    /// Takes in the endpoint we are calculating to.
    /// 
    /// This uses a simple 8 step approximation (2 end points plus 6 evenly 
    /// spaced steps between), for quadratic and exponential formulas due to
    /// their bonkers arc length integrals.
    /// 
    /// NOTE: Currently not used, arc length priority calculation is as cool as it is stupid.
    pub fn arc_length(&self, start: f64, end: f64) -> f64 {
        if start == end { // if start and end are the same, then there's no length, jsut leave.
            return 0.0;
        }
        match self {
            DemandCurve::Linear { slope } => {
                let diffx = end - start;
                let diffy = slope * diffx;
                ((diffx).powf(2.0) + (diffy).powf(2.0)).sqrt()
            },
            DemandCurve::Root { .. } => {
                let diff = end - start; // get distance between start and endof the interval
                let step_size = diff / 8.0; // divide it up
                let mut acc = 0.0; // distance accumulator
                for cl in 0..8 { // step 7 times (8 points)
                    // get our end point steps
                    let lower_step = cl as f64 * step_size;
                    let upper_step = (cl + 1) as f64 * step_size;
                    // get our end point ys.
                    let lowery = self.value(start, lower_step);
                    let uppery = self.value(start, upper_step);
                    // add distance to our accumulator
                    acc += (step_size.powf(2.0) + (uppery - lowery).powf(2.0)).sqrt();
                }
                acc
            },
            DemandCurve::Geometric { .. } => {
                let diff = end - start; // get distance between start and endof the interval
                let step_size = diff / 8.0; // divide it up
                let mut acc = 0.0; // distance accumulator
                for cl in 0..8 { // step 7 times (8 points)
                    // get our end point steps
                    let lower_step = cl as f64 * step_size;
                    let upper_step = (cl + 1) as f64 * step_size;
                    // get our end point ys.
                    let lowery = self.value(start, lower_step);
                    let uppery = self.value(start, upper_step);
                    // add distance to our accumulator
                    acc += (step_size.powf(2.0) + (uppery - lowery).powf(2.0)).sqrt();
                }
                acc
            },
            DemandCurve::Logarithmic { factor } => todo!(),
        }
    }
}