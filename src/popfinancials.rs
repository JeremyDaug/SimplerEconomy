use circular_buffer::CircularBuffer;

/// # Pop Financials
/// 
/// Pop financials is a helper which stores the financial information of a pop.
/// 
/// Includes a record of their current AMV financial situation, a history of their
/// situation, and plans.
/// 
/// It also includes their financial mood, IE, uncertainty, fear/greed, etc.
/// 
/// # Loading Note
/// 
/// This will likely not be saved or loaded. If it is, it will only be the minimal data necissary for
/// basic functionality. It may be saved eventually, but for now I'm not going to bother. Shouldn't be that difficult.
/// 
/// ## Uncertainty, Risk Tolerance, and Time Preference
/// 
/// These three make up primary economic mood of a Pop. 
/// 
/// Uncertainty is how unsure they are of the market. Risk Tolerance is how willing they are to take
/// losses, and Time Preference is how now vs later oriented a pop is IE, how much they discount the future
/// relative to now.
/// 
/// Uncertainty drives savings up, and the required return on exchanging goods they 
/// currently desire is also driven up.
/// 
/// Risk Tolerance drives savings down, investment up, but also increases the pop's 
/// tolerance to losses. A "Normal" Risk Tolerance is not 0 or 1, but somewhere in 
/// the middle.
/// 
/// Time Preference helps define what return on investment the pop needs to be willing 
/// to invest more. The lower their time preference, the lower Interest Rate they will
/// need to see to invest more or continue investing. Low Time preference also 
/// counters Risk aversion, but much more weakly.
#[derive(Debug, Default, Clone)]
pub struct PopFinancials {
    // TODO: Possibly include a 'metric' section to stabilize values to a predifined good in the market, particularly a currency.

    // Current Section

    /// The pop's starting wealth (in AMV) for the day. Everything that survived from
    /// yesterday to today. This value should not change during the day.
    pub wealth: f64,
    /// The pop's income for the day, gained by the pop from work. Equal to the goods
    /// given to the pop minus what they gave up.
    pub income: f64,
    /// The pop's income for the day in the form of interest, dividends, and the like.
    pub dividends: f64,

    /// The amount of AMV the pop currently has in it's possession and has not reserved 
    /// for specific purposes yet. Also known as the Working Wealth of the pop.
    pub current_wealth: f64,
    /// How much we have consumed or have marked for consumption.
    pub consumed: f64,
    /// How much has not been consumed yet today.
    pub saved: f64,
    /// How much we earmarked for investment.
    pub invested: f64,
    // todo: Maybe include borrowed here?

    /// How much AMV the pop has at the end of day, post consumption.
    /// 
    /// Used to calculate weath change over the day (This is post decay and consumption).
    pub amv_end: f64,
    /// How much AMV was lost to decay at the end of the day.
    pub decay: f64,

    // History Section

    /// History of wealth over the past 30 days.
    /// 
    /// NOTE: May bump size down from 32 to 16.
    pub wealth_history: CircularBuffer::<32, f64>,
    pub income_history: CircularBuffer::<32, f64>,
    pub dividend_history: CircularBuffer::<32, f64>,
    /// The average wealth of the pop for the past few days.
    /// 
    /// Calculated via a rolling average of 30 days.
    pub average_wealth: f64,
    /// The rough direction and magnitude of changes in wealth over time.
    /// This is likely to be a linear regression over the history, or perhaps something simpler.
    pub wealth_inertia: f64,
    /// The average income recieved by the pop for the last 30 days.
    pub average_income: f64,
    /// The rough estimate of income's change over time.
    /// This is likely to be a linear regression over the history, or perhaps something simpler.
    pub income_inertia: f64,
    /// The average dividend recieved by the pop over the past 30 days.
    pub average_dividend: f64,
    /// The rough estimate of the change over time.
    /// This is likely to be a linear regression over the history, or perhaps something simpler.
    pub dividend_inertia: f64,

    // Financial mood

    /// A measure of how certain the pop is about the future. The higher this is
    /// the more they will seek to save. The lower the more willing they are willing to
    /// spend or invest.
    /// 
    /// Cannot be Negative.
    pub uncertainty: f64,
    /// A measure of how willing to take a risk the pop is. The higher this value the
    /// more they are willing to invest in risky ventures, the higher the losses they 
    /// will tolerate, and the more volatilaty they will be willing to withstand on 
    /// their savings. 
    /// 
    /// This can offset Uncertainty.
    /// 
    /// Cannot be Negative.
    pub risk_tolerance: f64,
    /// A measure of how much they seek in the form of returns on their investments.
    /// The higher this vaule, the higher the interest rate they seek.
    /// 
    /// This is roughly how much interest they want on their investments. 
    /// 
    /// Investments are measured on a 100 day basis.
    /// 
    /// Cannot be negative.
    pub time_preference: f64,

    // Plans Section

    /// The baseline savings ratio the pop seeks. Defined primarily by demographics.
    pub base_saving_rate: f64,
    /// The Savings rate Cap, (hard cap of 1.0), this is based on Demographics also
    pub saving_rate_cap: f64,
    /// The current active savings rate of the pop. It can shift up and down over time.
    pub curr_saving_rate: f64,

    /// The base rate of investment from demographics.
    pub base_investment_rate: f64,
    /// The current cap on investment rate. Set by demographics.
    pub investment_rate_cap: f64,
    /// The current rate of investment as has moved over time.
    pub current_investment_rate: f64,

    /// The baseline interest rate for the pop. Primarily defined by demographics.
    pub base_interest_rate: f64,
    /// The current cap on interest rate, this cap is a lower bound rather than an 
    /// upper bound.
    pub interest_rate_cap: f64,
    /// This is the current effective interest rate of the pop, reached by the 
    /// combination of Base Rate, various Mood weights.
    pub current_interest_rate: f64,
}

impl PopFinancials {
    pub fn new() -> Self {
        Self { 
            wealth: 0.0,
            income: 0.0,
            dividends: 0.0,
            current_wealth: 0.0,
            consumed: 0.0,
            saved: 0.0,
            invested: 0.0,
            amv_end: 0.0,
            decay: 0.0,
            wealth_history: CircularBuffer::new(),
            income_history: CircularBuffer::new(),
            dividend_history: CircularBuffer::new(),
            average_wealth: 0.0,
            wealth_inertia: 0.0,
            average_income: 0.0,
            income_inertia: 0.0,
            average_dividend: 0.0,
            dividend_inertia: 0.0,
            uncertainty: 0.0,
            risk_tolerance: 0.0,
            time_preference: 0.0,
            base_saving_rate: 0.0,
            saving_rate_cap: 0.0,
            curr_saving_rate: 0.0,
            base_investment_rate: 0.0,
            investment_rate_cap: 0.0,
            current_investment_rate: 0.0,
            base_interest_rate: 0.0,
            interest_rate_cap: 0.0,
            current_interest_rate: 0.0,
        }
    }

    /// # Current Total Wealth
    /// 
    /// The sum of Current (unreserved) wealth, consumed wealth, saved wealth, and 
    /// invested wealth.
    pub fn current_total_wealth(&self) -> f64 {
        self.current_wealth + self.consumed + self.saved + self.invested
    }

    /// # Update Average Wealth
    /// 
    /// Takes the current wealth history and calculates the new average.
    /// 
    /// Should be called at the start of the day, which should be before goods have
    /// decayed.
    /// 
    /// It should include skills and time.
    pub fn update_average_wealth(&mut self) -> f64 {
        let res = self.wealth_history.iter().sum();
        self.average_wealth = res;
        res
    }

    /// # Update Average Income
    /// 
    /// Updates the average income based on history.
    /// 
    /// Should be called after a pop exchanges it's time and skills for labor.
    /// 
    /// Calculates as the difference between the wage recieved and the time lost.
    /// Skills are not consumed, but instead copied and improved based on pop 
    /// education rate.
    /// 
    /// NOTE: Should take into account wages recieved only every few days eventually.
    pub fn update_average_income(&mut self) -> f64 {
        let res = self.income_history.iter().sum();
        self.average_income = res;
        res
    }

    /// # Update Average Dividend
    /// 
    /// Updates the average dividend based on history.
    /// 
    /// Should be called after dividends are recieved, this should be measured in simple added wealth.
    /// 
    /// NOTE: Should eventually take into account dividends recieved only every few days.
    pub fn update_average_dividend(&mut self) -> f64 {
        let res = self.dividend_history.iter().sum();
        self.average_dividend = res;
        res
    }
}