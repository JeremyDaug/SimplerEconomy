# EconCiv V1: The Market Game

## Purpose

The full civilization-scale market game is too big to build in one go. Every attempt so far (V3, then the V4 draft) has run into the same problem: too many systems have to work before we can find out if any of it is fun or stable.

So instead, the plan is to build a series of smaller games that build up and off of each other. Each one is a complete, playable game on its own, and each one becomes the foundation layer for the next.

This document covers V1: a single market, with simulated imports and exports, that plays more like an incremental game. The player runs one firm. Everything bigger (multiple currencies, credit, regions, nations, the full civ layer) is in the Roadmap at the end.

How to read this doc:

- Plain statements are decided.
- **Open question:** marks something that isn't settled yet. Each one has a **Proposal:** with a suggested default. The proposal is a starting point, not a decision.
- Terms in the Glossary are capitalized where they matter, like Pop, AMV, External Market.

---

# The Game in Brief

You are a single firm in a small market. You buy goods, produce goods, and sell goods. The money you earn gets spent on:

- **Expanding the business.** More production lines, more workers, new goods.
- **Developing technology.** Unlocking new processes and goods, or making old ones better.
- **Building Prestige and Influence.** Prestige sways the masses. Influence gives you the king's ear.
- **Stabilizing the market through philanthropy.** Putting money back into the pops' pockets when the market is struggling.

Behind the firm, you are technically a pop too: the owner pop of your firm. That's mostly automatic and you won't manage it directly (see Firm Ownership).

You aren't alone. There are rival firms competing for the same customers, workers and inputs. You can drive them out of business, collude with them, or buy them out. There is also the local government, which is a player in its own right. It isn't something you'd normally drive out or control. You either cooperate with it or appease it, and you pay it regular taxes.

Money in this game is not magic. It is rarely created or destroyed. If you want more of it, it has to come from somewhere, mostly from selling goods outside the market.

It should not be grindingly difficult, but there should be real challenge and risk. Multiple difficulty levels with different starting conditions are planned (see Difficulty).

---

# Core Rules

## Time

- The game runs in **real time with pause**. One tick is one day.
- **Speed controls** are an option the player can set (for example pause, 1x, 2x, 4x).
- The exact length of a day isn't a hard realism goal. One day per tick is the standard because that's what the sim already runs on.
- "Per turn" anywhere in this doc means per day tick.
- Yearly rates, such as births and deaths, are divided by `pop.turn_length`, the turns in one year. It's 365 by default, so each day applies one day's share of a year.

> **Open question:** Can the player queue or issue orders while paused, and do orders apply at the next tick or immediately?
> **Proposal:** Orders can be given while paused and all take effect at the start of the next tick (Player Action step in the Day Tick Order). Nothing changes mid-tick.

## The Market

- V1 has exactly **one market**. All pops, firms and the government live in it.
- Everything else in the world is represented by the **External Market** (see Money and Trade).
- Prices inside the market use the existing AMV supply-and-demand pricing (see Carryover from Existing Code).
- **Prices are quoted in Duckats.** The market's first currency is its principal, and its value is fixed at 1. With one currency there is nothing for it to trade against, so it never moves, and AMV and the Duckat price are the same number. Before the first day, the market counts what one Duckat costs to mint, in time, and sets the value of one unit of time from that. Every opening price is then in Duckats.
- **Supply is what is offered.** Prices move on the units put up for sale that day, not on stock held back. A firm offers what it made on the last day plus `firm.stock_offer_share` (default 0.10) of its held output stock. The rest is held and isn't supply.

## Goods

V1 uses a handful of goods (see Suggested V1 Scope). Goods keep their V3 rules: an id, a name, a decay rate, tags, and the processes that make and use them. Labor is a good that firms buy from pops (see Labor). The trade facilities and longshoremen behind imports and exports also run on pop labor (see Trade Facilities, Transport and Tariffs).

---

# Money and Trade

Money is the heart of V1. V1's one coin is the **Duckat**, a gold coin named as a nod to the ducat. Players may be able to rename the currency later. The core idea: **money is conserved**. It moves between actors, and it is only created or destroyed in a few named ways. Pops in the market are not "magic boxes" that produce or swallow coin.

## The External Market

- The External Market stands in for the rest of the world.
- It is treated as **effectively infinite** in both supply and consumption. There is **no hard cap** on how much can be bought from it or sold to it. What limits trade is the market's own trade capacity (see Trade Facilities, Transport and Tariffs).
- It has its own prices, one per good, and those prices have their own **randomized fluctuations**.
- Imports and exports do move External prices, but much more slowly than the internal market. Roughly **1000 times slower**.
- Because it absorbs far more and barely moves, the External Market is the **primary price stabilizer**:
  - If local prices are inflated, goods get imported and coin flows out of the market, which brings prices down.
  - If local prices are depressed, goods get exported and coin flows in, which brings prices up.

> **Open question:** What does "1000 times slower" mean in mechanical terms?
> **Proposal:** The External price for a good uses the same AMV step as the local market, with its sensitivity to excess supply or demand divided by 1000. Its random fluctuation is a separate small random walk with a mean pull back to a base price, so it doesn't wander off over a long run.

## Trade Facilities, Transport and Tariffs

Shipping in and out isn't free, and it needs people.

- **Transport and tariffs.** Moving goods in or out costs transport, and the government can charge tariffs. Together they open a **band** around the External price: imports cost more than the External price, and exports earn less. Local prices are free to move inside that band. Once a local price leaves it, trading with the outside becomes worth it, and goods and coin start to flow.
- **Trade capacity.** There's no hard cap, but there is a **capacity**, based on the market's ability to staff and maintain its trade facilities (docks, warehouses and the like). More staff and better facilities mean more goods can move per day.
- **Longshoremen.** Local longshoremen are always needed to load and unload. They are local pops hired for wages, the same as any other labor. So a labor shortage can also throttle trade capacity, and with it the External Market's price stabilizing. That is intended risk.
- **Traders.** The traders (ships, in this case) are the actors who act on price gaps. A trader can be **locally run or foreign run**, and its **crew can be local or not**. Traders buy where it's cheap and sell where it's dear, paying transport, tariffs and longshoremen as they go.

Because of this, coin crosses the market's border in more ways than just paying for goods. Anything paid to a foreign actor leaves the market, and anything a foreign actor pays inside the market comes in. See Money Accounting.

> **Open question:** How are trade capacity, transport cost and the tariff band calculated?
> **Proposal:** Each trade facility has a daily throughput in Bulk, scaled by how many of its longshoremen and maintenance staff are filled and by its upkeep being paid. Transport is a per-Bulk cost paid to the trader's crew. Import price = External × (1 + transport + tariff), export price = External × (1 − transport − tariff). Facilities are built and expanded like any other firm investment, by the government or by firms.

> **Open question:** Who runs the traders at the start, and how much of the trade is local versus foreign?
> **Proposal:** Every run starts with a few foreign-run traders with foreign crews, so trade works on day 1. Local firms (including the player) can build or buy ships and hire local crews, which keeps the crew wages and trading profit inside the market. Foreign traders fill whatever capacity local ones leave unused. Difficulty can set the starting mix.

## Where Money Comes From (Sources)

1. **Exports. Always available.** Selling goods to the External Market brings new coin into the market. This is the one source that every run is guaranteed to have. More broadly, any coin a foreign actor pays inside the market (a foreign trader paying local longshoremen, tariffs paid by foreign traders) comes in the same way.
2. **Government deficit printing.** The government can go into debt, money-printer style. When it spends more than its treasury holds, it prints the difference and keeps track of the debt. This is not true government lending: nobody lends it anything, it just tracks its negative balance. The debt still costs the government (see Government Debt).
3. **Minting. Not in every run.** Some runs have a way to mint coin (for example a metal deposit and a minting process). It is certainly a source of new money when it exists, but it is not guaranteed. Duckats a pop crafts on its job are minted coin too.

## Where Money Goes (Sinks)

1. **Imports.** Buying goods from the External Market sends coin out of the market. So does any coin paid to a foreign actor (a foreign crew's wages, a foreign trader's profit carried home, the government's interest and debt repayments to its foreign holders).
2. **Decay (optional).** A tiny daily loss on every coin. The default is 0.001 per day, set as the Duckat's decay rate. It's **optional, with a tunable rate**, which becomes a player option later. It can be turned up, but it should never be a major factor, and it can be removed entirely.
3. **The last pop dying.** A dying pop with no other pop in the market to merge into is removed with its coin (see Pop deaths under Transfers).

## Later Sources and Sinks (Not V1)

These are improvements for later versions, not V1 requirements:

- **A bond market and true government lending.**
- **Fuller central bank policy** beyond deficit printing (deliberate burning, interest rates).
- **Loans and credit.**
- **Alternative or competing monies.**

## Transfers (Not Creation or Destruction)

Everything else just moves coin from one actor to another inside the market:

- **Wages.** Firms, traders and the government pay pops for labor. This includes researchers, builders, longshoremen and local crews.
- **Ownership.** Firms pay their owner pops (see Firm Ownership). Buyouts pay the bought firm's owner pops.
- **Purchases.** Buyers pay sellers for goods inside the market.
- **Philanthropy.** Firms give coin or goods to pops.
- **Government welfare.** The government gives coin to pops.
- **Taxes and tariffs.** Firms (and possibly pops) pay the government. Tariffs paid by local traders are a transfer too.
- **Firm-to-firm deals.** Tech sales, collusion payments.
- **Lobbying, bribes and corruption.** Coin paid to the government's coffers or to the government's pops.
- **Pop deaths.** A pop that falls under one household merges into the most similar other pop in the market and takes all its goods, coin and firm shares with it. Most similar is same species and culture, then same species, then same culture, then any pop. A lower pop id breaks a tie.

Pops get money with which they can buy goods from **wages, ownership, philanthropy and government welfare**. That's what makes them not magic boxes: if nobody pays them, they can't buy.

- **Pops sell their own surplus.** Pops make their basics through subsistence and sell the small excess, and their crafts, into the market. That's a transfer from the buyer, and it stays small next to wages and ownership. Laborers whose work leaves no time to make their basics buy it. Owner pops don't run subsistence: they live off their firm and buy what they need. Firms buy some of the surplus as inputs and sell it on as higher-order goods. Subsistence itself is not reduced to push pops into the market.
- **Pops scale back what doesn't sell.** A pop offers everything it isn't keeping for itself. Every `pop.offer_window_days` (default 3) days it looks at each good it offered: if it sold less than `pop.offer_sold_share` (default 0.5) of what it offered, it multiplies the share it offers by `pop.offer_cut` (default 0.5), down to `pop.offer_scale_min` (default 0.01). A window that sold enough divides the share by the cut, up to all of it. Coin and time are always offered whole.
- **Every pop starts with coin.** On the first day each pop holds at least `pop.starting_coin_days` (default 10) days of its basic desires in Duckats, so it can buy from day 1. That coin is the opening stock, not a source on the ledger.
- **Firms don't compete with subsistence.** A good is subsistence-covered when pops on the market made at least `firm.subsistence_cover_share` (default 0.5) of what they ate of it today by subsistence. A firm idles any line that makes a covered good, unless more of the good was sought than offered for `firm.short_days_to_compete` (default 5) days in a row. A firm that uses a covered good as an input, such as a mill using grain, buys it from pops.

## Spending Pays Someone

Because money is conserved, **every coin a firm spends goes to some actor**. Nothing vanishes into a "cost".

- **Research.** Research means hiring people to do research. The money goes to their wages, plus any goods the research uses up, which are bought from whoever sells them.
- **Lobbying.** The money goes to the government's pops (officials) or into the government's coffers.
- **Expansion.** Hiring builders (wages) and buying materials (purchases). Upkeep on what's built works the same way.
- **Market research.** Hiring pops to do the survey and study work (wages).
- **Prestige.** Earned through philanthropy (straight to pops) and through public works and events, which pay wages and buy goods.
- **Influence.** Earned through lobbying and favors, which go to the government's coffers or its pops, as above.
- **Bribes, corruption and thugs.** Paid to the pops who take them.

## Government Debt

- The government tracks its **treasury balance**, which can go **negative**. The negative part is its **debt**.
- When it has to pay and the treasury doesn't cover it, it **prints** the coin it needs, and the debt goes up by the same amount.
- Since it can always print, it can always pay. Debt still costs it, in three ways.

### 1. Debt-to-GDP and Stability

- The bigger the government's **debt-to-GDP ratio**, the more **unstable** it becomes.
- There's **no strict cap or failure point**. Instability just keeps rising with the ratio until the government breaks (see Regime Changes and Game Overs).
- It **can recover**. Bringing the ratio back down brings stability back up.

### 2. Interest

- The government **pays interest on its debt**, and that eats more and more of its tax revenue.
- If interest payments **eat the whole budget**, that's a **hard crash signal**. The government needs **immediate budget fixes** (raising taxes, cutting spending), or other consequences follow.
- **Nobody inside the market receives the interest.** In V1 the government's debt is always held abroad, so interest is paid in real coin out of the treasury to foreign holders. It leaves the market as a border outflow, and it doesn't reduce the debt.
- If the treasury can't cover the interest, the government prints to pay it. That adds to the debt, and it's exactly the hard crash spiral.

### 3. Inflation (secondary)

- Printed coin is real new coin chasing the same goods, so it pushes prices up. Some of it leaks out through imports as local prices rise into the import band, so the External Market partly absorbs it.
- Inflation **hits poor and fixed-income pops** hardest and makes them **angrier**. That feeds pop mood, then stability and revolution risk.
- **The Cantillon effect.** The first receivers of new money (whoever the government pays with printed coin) get its full value before prices rise. Everyone else gets it after prices have risen. That's effectively a **wealth transfer**, and it can make the problem worse.

> **Open question:** How are GDP, the interest rate and the crash consequences worked out?
> **Proposal:**
> - **GDP** is the rolling yearly value of goods produced in the market at local prices.
> - **The interest rate** is a small base rate that rises with the debt-to-GDP ratio.
> - **Crash consequences.** If interest goes past 100% of revenue for some days, the government enters a budget crisis: a big stability hit each day, welfare and its own wages are cut automatically, and regime change becomes much more likely until it's fixed.

> **Open question:** Does the government ever pay down the principal?
> **Proposal:** When the treasury runs a surplus, the government may pay down debt by paying its foreign holders. That coin leaves the market as a border outflow and the debt falls by the same amount. Whether it bothers is up to its personality.

## Money Accounting and the Invariant

The money supply should balance exactly every day. For each day:

```
total_coin(end of day) = total_coin(start of day)
                       + border_in        (exports, coin paid in by foreign actors, coin brought by immigrants)
                       - border_out       (imports, coin paid to foreign actors, coin taken by emigrants,
                                           government interest and debt repaid to foreign holders)
                       + coin_minted
                       + coin_printed
                       - coin_decayed
                       - coin_pruned      (coin of a dying pop with no other pop to merge into)
```

total_coin is the sum of coin held by every pop, firm, local trader and the government, counting only positive holdings. The government's debt is tracked separately, and a second check holds alongside the first. debt_repaid is the part of border_out that pays down the debt's principal:

```
debt(end of day) = debt(start of day) + coin_printed - debt_repaid
```

Foreign traders, foreign crews and the government's foreign debt holders are outside the market. Coin they hold isn't counted, and every coin that moves between them and a local actor is a border flow.

**Invariant test idea:**

- Run a test scenario for N days (say 1000).
- Each day, sum all coin everywhere and record each flow above.
- Assert that both equations hold within floating-point tolerance (something like 1e-9 relative).
- Run it once with decay, minting, printing and migration all off, where total coin must only change by border_in minus border_out.
- Keep it as a debug check every tick in debug builds too, so any new feature that creates or deletes coin by accident gets caught immediately.

Ledger: each flow is recorded per day so the player (and we) can see them. That ledger also feeds market research and the government's view of the economy.

> **Open question:** Do non-coin goods follow the same conservation rule?
> **Proposal:** No. Goods are made by production and destroyed by consumption and decay, same as V3. Conservation is about coin only. Goods only matter here in that importing and exporting them moves coin.

---

# Pops

Pops are the market's customers, workers and owners. They use the existing pop engine: desires, households, savings, and sentiments.

## What the Player Sees

- **Aggregate demand.** Always visible. What the market as a whole wants and is buying.
- **One pop sample per turn.** The player can look at one pop, picked at random, each turn.
- **Market research.** Seeing more than that costs money. Research buys a deeper look: more pop samples, demand broken down by group, wage expectations, mood, and so on. The money goes to wages for the pops doing the research.

> **Open question:** What exactly does market research reveal, and how long does it last?
> **Proposal:** Research comes in tiers. Each tier is a purchase that gives a report snapshot (more samples, then breakdowns by pop group, then desire details and forecasts). Reports go stale over time and have to be bought again. Rivals can buy research too.

> **Open question:** Is the random sample free and automatic, or an action the player takes?
> **Proposal:** Free and automatic. One random pop's card shows up each tick. The player can pin a sample to compare against later ones.

## Labor

- Firms hire pops as workers **from the same market**. Labor is not abstracted.
- That means you can run into a **labor shortage**. If everyone is employed or nobody will take your wage, you can't produce. That includes longshoremen, so a shortage can slow trade too.
- Wages are the main way coin flows to pops.

> **Open question:** How are wages set?
> **Proposal:** Labor is priced like any other good, by AMV. Firms post jobs with a wage, pops take the best offers for their time, and the labor price moves with supply and demand. Strike breaking (if enabled) lets a firm keep running during a labor dispute at a reputation cost.

## Owner Pops

- Every firm is owned by one or more pops (see Firm Ownership).
- Owner pops **receive dividends out of the profits** of the firms they own. That's a fourth way pops get money, beside wages, philanthropy and welfare.
- An owner pop is still a pop: it has desires, buys goods and has a mood. It **spends its money on its own desires**, so dividends flow back into the market.
- **Idly wealthy pops still spend.** An owner pop that loses its firm (bought out or failed) doesn't just sit on the coin. It keeps buying for its desires. The savings requirements of new pops, as in older versions, keep it from going broke immediately, but it will run out eventually.

## Migration

Migration is **not off** in V1. But it needs to be reworked to be more abstract, so that people coming in and going out isn't as easy as just buying labor from the exchange.

> **Open question:** How does the abstract migration work?
> **Proposal:**
> - Each day the market has an **attractiveness** score for each kind of pop, built from wages, prices of basic goods, pop mood and stability, compared against an External baseline.
> - A positive gap builds **slow pressure**, and immigrants arrive in small batches after a delay. A negative gap does the same for emigrants. Firms can't hire people from outside directly.
> - Government policy (open or closed borders) and trade capacity (people arrive and leave by the same routes) can scale the flow.
> - Immigrants bring a little coin and emigrants take their savings with them. Both count as border flows in the ledger.

## Pop Mood

Pop mood uses the existing five-part sentiment (happiness, contentment, anger, fear, hope). The market's overall mood feeds the government's favor and stability, and the revolution risk (see Government).

---

# Firms

## The Player Firm

The player is one firm. It buys, produces, and sells. Its spending options are: expansion, technology, Prestige, Influence, and philanthropy (see The Game in Brief). Each of those pays someone (see Spending Pays Someone).

## Firm Ownership

- **Firm owners are pops.** One or more pops own each firm, and the firm pays them.
- **The player is technically a pop too**, the owner pop of the player's firm. That's mostly automatic: the player runs the firm, and the owner pop just lives off what the firm pays it.
- Owner pops can **start new firms**. A pop with enough coin can found one.
- **Firm founding is in V1, in a simple form.** It's needed to some extent for turnover and healthy long-term competition as new potential industries form.
- **Owners are paid a wage, then a dividend.** Each owner gets its fraction of both, and the payment comes at the start of the market day, before the firm trades.
  - **The wage** is a multiple of the owner pop's basic day needs: the cost of one day of its basic desires at market prices, for all of its households. A larger owner pop gets a larger wage. The multiple is `firm.owner_wage_basic_days` in the config, default 5.
  - **The wage comes out of a reserve filled by sales.** Each day the firm sets aside `firm.wage_reserve_share` (default 0.20) of the last day's sales, and the wage is paid only up to that reserve. A firm that sells nothing pays no wage, so the wage can't eat the coin the firm needs for inputs.
  - **The dividend** is a share of the firm's profit from the day before. Profit is sales less input cost less the owner wage. The share is `firm.owner_profit_share` in the config, default 0.10. A firm that made no profit pays no dividend, so a failing firm keeps its coin through a downturn.
  - Both values are config defaults for now. Later they follow the firm's personality.

> **Open question:** How does the player control their own owner pay?
> **Proposal:** The player sets their own wage multiple and profit share as sliders. The player's owner pop buys its own goods automatically, and the player doesn't manage its desires.

> **Open question:** When does a pop with coin start a new firm instead of living off it?
> **Proposal:** A pop with coin above a threshold has a daily chance to found a firm, weighted by its personality and by which goods are short in the market (high prices, unmet demand). Otherwise it spends down its savings on its own desires.

## Rival Firms

- There are a few rival firms in the market.
- Rivals have **personalities**, but not strict fixed ones. They are **dynamic**: they shift with what happens to them, and they can be **swayed diplomatically**.
- The player can:
  - **Drive them out of business** (outcompete, undercut, starve them of workers or inputs).
  - **Collude** with them (price fixing, splitting the market).
  - **Buy them out.**

> **Open question:** What are the personality traits?
> **Proposal:** A handful of sliders instead of fixed types, for example Aggression (undercutting, dumping), Caution (savings vs. expansion), Cooperativeness (open to collusion and deals), Scruples (willingness to do bad actor things), and Ambition (growth vs. steady profit). Events and the player's actions move the sliders. Diplomacy (spending Influence, making deals, threats) moves them deliberately.

## Buyouts

- When a firm is bought out, **its owner pop or pops get the money**.
- They can do what they like with it. They could live **idly wealthy** until the money runs out, or they could **start a new firm**.

> **Open question:** How is the buyout price set without a Stock Market?
> **Proposal:** A firm's owners will sell at a price based on its assets plus some multiple of recent profit, adjusted by personality and how desperate it is. A bought firm becomes a second branch of the buyer's firm.

## Going Out of Business

- A firm that goes out of business stops paying its owners. Whatever coin it has left goes to its owner pops.
- Like a buyout, the owners can live off it or start again.

> **Open question:** What happens to the failed firm's workers, stock and facilities?
> **Proposal:** Its workers are released back into the labor pool, and its stock and facilities are sold off into the market over a few days, with the proceeds going to the owner pops. Nothing is destroyed.

## Firm Budgeting

The existing firm budget (input cost, output value, sold value, owner pay) is the starting point. V1 adds player-facing spending categories on top of it.

- **Output follows demand.** A firm keeps an average of what it sold each day, each day moving it one part in `firm.demand_window_days` (default 3). Each line plans no more runs than cover that average plus `firm.demand_headroom` (default 0.10), the room it needs to grow into new demand. A firm never idles every line for lack of sales: when all its other lines would be under `firm.min_line_target` (default 1) runs, its first one runs that many. Lines idled for competing with subsistence stay idle.

---

# The Government

The local government is a player too, but not one you'd normally drive out or control. You cooperate with it or appease it, mostly through regular taxation.

## Personality

- The government has a personality, shaped mainly by its **government type** (monarchy, democracy, and so on) and the **mood of the people**.
- Firms, including you, can tug at it through **lobbying**. Lobbying money goes to the government's pops or its coffers.
- It wants to keep firms from **challenging** it if it can. If it has to, it is able to **overpower or undermine** a firm.
- It looks to keep **bad actors** out of the market. That can include you.

## Favor and Stability

Instead of a satisfaction meter fed by the player, the government tracks **favor** and **stability**. These take into account:

- How well you and the other firms behave.
- The wider mood of the pops.
- Its ability to meet its financial obligations. Since it can print (see Government Debt), this mostly means how much debt it carries and how fast it's growing.

> **Open question:** Is favor one number for the government overall, or one number per firm?
> **Proposal:** Favor is per firm (how the government sees each firm). Stability is one number for the government itself. Low favor brings audits, fines and pressure on that firm. Low stability makes the government unpredictable and raises the odds of a regime change.

## Treasury and Obligations

- The government's obligations are wages for its own pops, welfare, and upkeep on what it runs, such as trade facilities if it owns them.
- It pays them from its treasury, which is filled by taxes, tariffs, fees, fines and lobbying money.
- When the treasury runs short, it **deficit spends** by printing and tracks the debt (see Government Debt).

## Taxes and Tariffs

- Tax rates are **variable and set by the government**.
- What it sets them to is mostly a question of **government personality, inertia, and lobbying**.
- **Tariffs** on imports and exports are one of its levers. They widen the band around the External price.

> **Open question:** What gets taxed besides trade?
> **Proposal:** V1 starts with a profit tax on firms and a sales tax on market trades, plus tariffs. Pops aren't taxed directly in V1. Rates change slowly (inertia), so the player can plan around them.

## Regime Changes and Game Overs

- **Radical changes**, like monarchy to democracy, shake things up: personality, tax policy, and what counts as bad behavior can all shift.
- Some directions the government goes are **game overs** or **special ending effects**:
  - **A communist revolution.** Game over.
  - **Being nationalized.** Game over. There may be an escape hatch where you're put in charge of the business after nationalization. For now that escape hatch is just a **variant game over**.
  - **Being broken up.** Game over.
- **Corporate capture and nationalization are two sides of the same contest.** Firms push on the government, and the government pushes on the firms. If you win, you capture the government (a soft ending). If you lose, you get nationalized (a game over).

> **Open question:** How is the capture-versus-nationalization contest tracked?
> **Proposal:** One tug-of-war meter per firm. Influence, lobbying, the government's debt to and dependence on the firm, and Prestige push it toward capture. Low favor, bad actor flags, the firm being large or essential, and a strong, stable government push it toward nationalization. Each end is reached by holding past a threshold for some days, so neither happens in one bad tick.

> **Open question:** What triggers revolution and breakup, and how much warning does the player get?
> **Proposal:** Each one has a visible pressure meter. Revolution builds from pop anger and fear and low stability. Breakup builds from market dominance plus bad actor flags (especially collusion). Each meter shows warnings at set thresholds so the player can react before it's too late.

> **Open question:** What drives a regime change, and which government types are in V1?
> **Proposal:** V1 ships two or three types (for example monarchy, oligarchy, democracy). Regime change is a rare event triggered by very low stability combined with pop mood. Lobbying and Prestige can push it one way or the other.

---

# Bad Actors

The government watches for bad actors, and the player can be one. Bad actor behavior:

- **Price gouging.**
- **Collusion.**
- **Dumping.**
- **Not paying taxes** (tax evasion).
- **Aggressive activities**, like **corruption** or **hiring thugs**.

Some behaviors are **game settings**:

- **Hiring strike breakers.** On or off.
- **Reverse engineering competitor tech.** Three options:
  - **On.** Available, and treated as bad behavior.
  - **Off.** Not available.
  - **Patent law.** It starts out legal in practice, but the government can make it illegal. Once it's illegal, doing it is bad behavior.

> **Open question:** What makes the government outlaw reverse engineering under Patent law?
> **Proposal:** It's a policy the government can pass like a tax change, driven by personality, lobbying (firms with lots of tech will push for it), and how often reverse engineering has happened. It can be repealed the same way.

> **Open question:** How are these behaviors detected and measured?
> **Proposal:** Each behavior has a simple, readable test. Gouging is selling well above the import price during a shortage. Dumping is selling below cost (or below the export price) for days in a row to hurt rivals. Collusion is any collusion deal, with a chance of detection each day. Evasion, corruption and thugs are actions with a detection chance. Each detection lowers favor and can lead to fines, which go to the treasury.

---

# Prestige and Influence

These are **separate** resources.

- **Prestige** is broad and soft. It sways the masses. It affects pop mood toward your firm, how willing pops are to work for you and buy from you, and the government's view of you through the people.
- **Influence** is targeted and hard. It gives you the king's ear. It is spent on lobbying, swaying rivals diplomatically, and getting specific things from the government.

Both are bought with coin indirectly, and that coin pays someone (see Spending Pays Someone).

> **Open question:** How exactly are they earned and spent, and do they decay?
> **Proposal:** Prestige is earned through philanthropy, public works, good wages and good products, and lost through bad actor flags. Influence is earned through lobbying spend and favors done for the government. Both decay slowly so they need upkeep. Influence is spent as a currency on specific actions. Prestige is a level that gives passive effects.

---

# Technology

- V1 uses a **fixed tech tree**. A semi-random tree would be nice, but it isn't necessary for now.
- Tech is **per firm**. Each firm has its own set of known techs.
- A firm can get a tech by:
  - **Buying it** (from another firm).
  - **Researching it**, at a discount if it's already known somewhere. Research means hiring people to do it, so the cost goes to wages.
  - **Reverse engineering a competitor**, if the game setting allows it (see Bad Actors).
  - **Waiting for it to spread** naturally.
- **Structure:** a number of major branches that are mostly independent, so a firm can drive deep into one without serious investment in the others. There are enough crossovers that if you don't have them, they slow your progress down.
- **Intended balance:** a firm invests in 1 or 2 branches, and the rest spreads to it naturally.

> **Open question:** How does tech spread naturally?
> **Proposal:** Each tech known by at least one firm in the market (or by the outside world) has a daily chance to spread to firms that have its prerequisites. The chance grows with how many firms know it and how long it's been known. Crossover techs a firm is missing make research in that branch slower (a cost multiplier) instead of blocking it.

> **Open question:** What does "research at a discount" mean exactly?
> **Proposal:** Research cost falls the more firms already know the tech, and falls further when it's known in the External Market. Undiscovered techs cost full price.

> **Open question:** What do techs unlock in V1?
> **Proposal:** New processes and goods (as the existing `Technology` node already allows), better efficiency on existing processes, and some unlocks for actions (new market research tiers, trade facility upgrades, new kinds of philanthropy, lobbying options).

---

# Endings

There is **no strict win condition**. There are soft endings, which we can describe as "you could keep playing, but you probably won't really like it":

- **Prestige.**
- **Market dominance.**
- **Wealth level.**
- **Corporate capture of government.** The winning side of the contest whose losing side is nationalization.
- **Government monopoly charter.**
- Possibly a few others.

And game overs (see Government): communist revolution, nationalization (with a variant ending for being put in charge after), and being broken up.

**Bankruptcy** is likely the end of the game. A firm is bankrupt when it has no coin, can't pay wages for some number of days in a row, and has nothing left to sell.

Whether the player's owner pop can use its remaining wealth to start a new firm as an escape hatch is a **game setting**:

- **Restart allowed.** The owner pop's remaining savings can found a new firm, and play continues.
- **Off.** Bankruptcy is a game over.

> **Open question (Jeremy's choice):** Which is the default for the bankruptcy setting, and is it tied to difficulty?
> **Proposal:** Tie it to difficulty. Restart allowed on easy and normal, Off on hard, and either one in a custom game.

> **Open question:** What are the thresholds for each soft ending, and what happens when you hit one?
> **Proposal:** Each soft ending shows an ending screen with a summary, then offers to keep playing. Thresholds are set per difficulty. A few more candidates: "Beloved Patron" (top Prestige with high pop mood), "Last Firm Standing" (all rivals gone or bought out).

---

# Difficulty

The game should not be a grind, but it should have challenge and risk. Risk should come mostly from the market itself: price shocks from the External Market, bad harvests, a rival dumping goods, a government turn.

Difficulty levels change the starting conditions. Candidate knobs:

- Starting capital.
- Number and aggressiveness of rivals.
- Market size (number of pops).
- External price volatility.
- Tax harshness and the starting government type.
- Starting trade capacity and the mix of local and foreign traders.
- Starting government debt and interest rate.
- The bankruptcy setting (Restart allowed or Off).

> **Open question:** How many difficulty levels, and are the knobs exposed as a custom game?
> **Proposal:** Three presets (easy, normal, hard) plus a custom mode exposing the knobs.

---

# Carryover from Existing Code

What exists in the repo now (checked on the `EconCiv-3-The-Reckoning` branch at `21cd54f`, and `EconCiv-v4-for-realsies` at `ec51e6b`). Pop desires, AMV pricing and firm budgeting are the engine for V1. The player action layer, unlocks, rival AI and the government model are new.

## Carries Over (Reckoning branch)

- **Pops** (`pop.rs`, `household.rs`, `desire.rs`, `effects.rs`). Desires from platonic through demographic to the pop's own, households and growth, subsistence, sell offers, savings, consumption, decay, standard of living, planning. This is the customer and worker engine.
- **Sentiment** (`sentiment.rs`). The five-part pop mood (happiness, contentment, anger, fear, hope), which can be blended by population for the whole market. Feeds government favor, stability and revolution risk.
- **AMV pricing** (`market.rs`). Per-good AMV, updated at end of day from supply and demand in `Market::record_keeping`, with a `MIN_PRICE` floor. This becomes the local price step, and with a much smaller sensitivity, the External price step.
- **Currency handling** (`market.rs`, `config.rs`). The market already has a currency list, and payment uses currency goods in `CURRENCY_STEP` units. Coin as a good with a decay rate is how the optional coin decay can work.
- **Firms** (`firm.rs`). `Firm`, `ProductionLine`, `FirmBudget` (input cost, output value, sold value, dividend), production, sell offers, record keeping and planning. `Market::pay_owners` pays the owner wage and dividend (see Firm Ownership) from firm stock. That pays out of goods the firm actually has, so it already respects conservation.
- **Processes and goods** (`process.rs`, `good.rs`), and the data loaders and the `pop_tester` and `firm_tester` examples, which are a good base for headless V1 test runs.
- **Tech node** (`tech.rs`). `Technology` already has a cost, parents, children, a tier, and unlocks for processes and goods. `techtree.rs` is an empty shell.

## Needs Rework

- **Labor and wages.** `workforce.rs` has a wage-row structure (workers, hours, a wage basket, profit share), but morning settlement isn't written. V1 needs labor hiring from pops working end to end.
- **Owner pay.** `Market::pay_owners` already pays pops who own firms, which matches V1. It needs to cover the player's owner pop and buyout payouts.
- **Government.** `nation.rs` has a `Nation` with empty early and late market day hooks. The government's treasury, debt and printing, taxes and tariffs, welfare, favor, stability and personality are all new.
- **Founding.** The current founding code (pops starting new pops and firms) is too involved for V1. A simpler version is in V1 scope so pops with coin can start firms, for turnover and new industries.
- **Pop savings.** The savings requirements of new pops from older versions come back, so idly wealthy pops draw down slowly.
- **Migration.** It comes back, but reworked to be more abstract (see Migration).

## Doesn't Carry Over (for V1)

- **Regions, tiles, map, units, player resources, institutions.** These belong to later versions.

## New for V1

- The **External Market**, trade facilities, longshoremen and traders (local and foreign), transport and tariffs.
- The **money ledger** and invariant check, including border flows and government debt.
- The **player action layer**: orders, the action queue, pause and speed.
- **Unlocks**: the tech tree and its spread.
- **Rival AI** with dynamic personalities and diplomacy.
- **The government model**: personality, taxes, favor, stability, bad actor detection, regime change, game overs.
- **Prestige and Influence.**
- **Market research.**
- A **seeded random number generator**, so runs with the same seed can be repeated for testing.

## Branch State to Keep in Mind

- `EconCiv-3-The-Reckoning` is still being reworked (`21cd54f`, "back to AMV"), so the exact pieces may change.
- `EconCiv-v4-for-realsies` branched from `0f58fde`, before most of the Reckoning work, and doesn't compile as of the V4 review (`src/game.rs` declares modules whose files were deleted).

V1 starts on its own branch, `EconCiv-V1-Market-Game`, taken from the Reckoning tip, because that tip has the working pop, AMV and firm code. Only what V1 uses is kept. The V3 founding, migration, map and multi-market code is removed from the V1 branch rather than flagged, so its assumptions don't carry into the new build. It stays in git history, and the simpler V1 versions are written fresh.

---

# Day Tick Order (V1)

A proposal, built from the V3 and refined V4 orders, cut down to what V1 needs.

1. **Start of day.** Time is generated. External prices take their random step.
2. **Player and AI actions.** Orders queued by the player apply. Rival firms, owner pops and the government make their decisions (hiring, pricing, deals, lobbying, tax and tariff changes, founding new firms).
3. **Labor.** Firms, traders and the government hire for the day, including longshoremen and crews. Pops pick jobs.
4. **Reservations and offers.** Each actor holds back goods it doesn't want to sell and posts sell offers.
5. **Market day.** Trading at today's fixed prices.
6. **External trade.** Traders act on price gaps outside the band, up to today's trade capacity (set by staffed longshoremen and maintained facilities). They pay transport and tariffs. Coin paid to or from foreign traders and crews is recorded as a border flow.
7. **Production.** Firm production, pop subsistence. Minting if the run has it.
8. **Wages, ownership and transfers.** Wages, owner payouts, philanthropy, welfare. Payments to foreign crews are recorded as border flows.
9. **Consumption.** Pops satisfy desires.
10. **Taxes and fees.** The government collects taxes, fines and fees. (Tariffs were collected in step 6.)
11. **Government obligations.** The government pays its own wages, welfare and upkeep. It pays interest on its debt first, to its foreign holders, as a border outflow. If the treasury is short, it prints the difference and its debt goes up. If it has a surplus and its personality wants to, it pays down debt, also as a border outflow.
12. **Pop mood.** Sentiments update, including anger from inflation on poor and fixed-income pops.
13. **Government update.** Favor, stability (including debt-to-GDP and any budget crisis from interest), bad actor detection, policy changes such as Patent law, pressure meters for game overs and the capture-versus-nationalization contest, regime change check.
14. **Growth and migration.** Births and deaths. Migration pressure updates and any arriving or leaving batches move, with their coin recorded as border flows.
15. **Decay.** Goods, desires, and coin decay (if on).
16. **Tech spread.** Daily spread rolls.
17. **Record keeping.** Firm books, the money ledger and both invariant checks, market research data.
18. **End-of-day prices.** Local AMV step, External price step.
19. **Endings check.** Soft endings and game overs.

> **Open question:** Are wages paid before or after the market day? Paying before means pops can spend today's wages today.
> **Proposal:** Pay wages for yesterday's work at the start of the next day (before the market day), so pops always have income to spend and firms have a day of float. Then step 8 only covers owner payouts, philanthropy and welfare.

---

# Suggested V1 Scope

- **One market.**
- **One currency** (Duckats). Grain and every other good are ordinary goods, not tender.
- **A handful of goods**, for example: grain, bread, wood, tools, cloth, and labor. Enough for at least one raw input → finished good chain and one tool good that improves production.
- **2 or 3 rival firms**, plus the **government**. Every firm has an owner pop.
- One External Market with prices for every good.
- **One trade facility** (a dock), staffed by local longshoremen, with a few foreign traders at the start and the option for local ones.
- Government deficit printing and debt tracking, with debt-to-GDP, interest and inflation effects. No bond market.
- Simple firm founding.
- Simple abstract migration.
- A small tech tree: 3 or 4 branches, a few techs each, with a couple of crossovers.
- Two or three government types.
- A headless mode first (run N days from a seed and print the ledger), then the UI.

> **Open question:** What is the UI for V1: terminal, or a graphics front end?
> **Proposal:** Headless and test-driven first. Then a simple terminal UI (like the existing tester examples) to make it playable. A proper front end comes once the game loop is fun.

---

# Build Order and Test Milestones

Each step should pass its milestone before moving on.

1. **Ledger and invariant.** Track all coin and every flow. *Milestone:* a test that sums all coin daily passes over 1000 days.
2. **Pops and one firm with an owner pop, closed market.** No trade, no government, no migration. *Milestone:* total coin never changes. Owner payouts reach the owner pop. Prices settle over 1000 days with a seed.
3. **Labor.** Firms hire pops for wages. *Milestone:* wages are the main pop income. A test shows a labor shortage stops production when demand for workers exceeds pops.
4. **External Market, trade facility, longshoremen and traders.** *Milestone:* coin changes only by border flows. A test pushes local prices up and down and they return to the band. External prices move about 1000 times slower than local ones under the same trade. A test with too few longshoremen shows trade capped by capacity. Foreign crew wages show up as border outflows.
5. **Coin decay and minting** as settings. *Milestone:* the invariant holds with each on and off.
6. **Rival firms with simple AI, buyouts and failures.** *Milestone:* 2 or 3 rivals over 1000 days don't all go bankrupt or all take over. The market doesn't collapse. A buyout and a failure both pay the owner pops, and no coin is lost. Idly wealthy owner pops spend down their savings over time instead of holding them.
7. **Simple firm founding.** *Milestone:* a market that loses a firm gets a new one within some number of days when demand is there. Over a long run, firms turn over and new industries appear.
8. **Government: treasury, taxes, tariffs, welfare, debt and printing.** *Milestone:* the debt equation holds every day. On normal difficulty the debt-to-GDP ratio stays bounded over 1000 days. A forced-deficit test shows prices rise, poor and fixed-income pops get angrier, and some of the new coin leaks out through imports. A runaway-debt test shows interest eating the budget and triggering the crash signal, and a test that cuts the debt shows stability recovering.
9. **Abstract migration.** *Milestone:* better wages and mood pull pops in slowly, worse ones push them out, and migrant coin balances in the ledger.
10. **Player action layer.** Orders, pause, speed. *Milestone:* a scripted player can run a full game from the command line.
11. **Tech tree, research and spread.** *Milestone:* a firm that never researches still picks up most techs over a long run. A firm that focuses gets deep in one branch first. Research spending shows up as wages.
12. **Prestige, Influence, lobbying, diplomacy.** *Milestone:* lobbying moves tax rates and Patent law. Diplomacy moves a rival's personality. Lobbying money lands in the treasury or with government pops.
13. **Favor, stability, bad actors, game overs, soft endings.** *Milestone:* scripted bad actor runs trigger the right game over. A scripted Influence-heavy run reaches capture, and a scripted bad actor run reaches nationalization. A well-behaved run reaches no game over in N days.
14. **Market research and the pop sample.** *Milestone:* the player can only see what they've paid for.
15. **Difficulty presets and balance passes.**
16. **UI.**

---

# Roadmap

V1 is the first layer. Each later version is its own smaller game that builds on the last. The exact order past V2 is a proposal.

- **V2: Money.** Alternative and competing monies. Loans and credit. A bond market and true government lending (interest can then go to bondholders inside the market). Fuller central bank policy. Default and bankruptcy rules.
- **V3: Ownership.** The Stock Market for shares and plots (from V4), pop classes (Landowner, Business Owner, Worker, Institutionalist, Governmental), rent, shared and traded ownership of firms, and the V4 firm ownership tree and budgeting (dividend plus a savings target).
- **V4: Many markets.** Multiple markets and regions, the tiered tile/region/global markets from the V4 draft, friction and transport between markets, full migration between them, and the External Market becoming real neighbors.
- **V5: Institutions and nations.** Institutions and their spread, nations, fuller government finance, and the player shifting from a firm to a nation.
- **V6: The civilization layer.** The map, units, player resources (unity, development, legitimacy, authority, faith), and the full civilization-scale market game.

> **Open question:** Is the roadmap order right? In particular, should Ownership (pop classes, Stock Market) come before Money?
> **Proposal:** Keep Money first, because the money rules are the core of V1 and loans and other monies extend them most directly.

The V4 draft and its review (`Design_V4.md` on `EconCiv-v4-for-realsies`) stay as the reference for the later layers.

---

# Glossary

- **AMV (Abstract Market Value).** A good's market price, moved each day by supply and demand.
- **Aggregate demand.** What the market as a whole wants to buy, shown without detail about individual pops.
- **Bad actor.** A firm the government flags for behavior it wants out of the market.
- **Band.** The range around an External price, set by transport and tariffs, inside which local prices move freely.
- **Border flow.** Coin crossing the market's border: exports, imports, payments to or from foreign actors (including the government's interest and debt repayments), and migrant coin.
- **Coin.** The single V1 currency, the Duckat.
- **Cantillon effect.** The first receivers of new money get its full value before prices rise, which is effectively a wealth transfer to them.
- **Debt (government).** The government's negative treasury balance, created by deficit printing. In V1 it is always held abroad.
- **Decay (coin).** The optional, tunable daily loss of coin.
- **Debt-to-GDP.** The government's debt compared to the market's yearly output. The higher it is, the less stable the government.
- **Deficit printing.** The government printing coin to cover spending beyond its treasury, and adding it to its debt.
- **External Market.** The rest of the world: effectively infinite supply and demand, slowly moving prices with random fluctuation.
- **Favor.** How the government sees a firm.
- **Firm.** A business that buys, produces, and sells. The player is one.
- **Influence.** Targeted, hard power over specific people, especially the government. "The king's ear."
- **Longshoremen.** Local pops hired to load and unload at trade facilities. Always needed for trade.
- **Market research.** Paying pops to give you a better look at the pops.
- **Minting.** Making new coin, available only in some runs.
- **Owner pop.** A pop that owns a firm and receives dividends from its profits. The player is technically one.
- **Patent law.** A reverse engineering setting where it starts out legal in practice and the government can make it illegal.
- **Philanthropy.** Giving coin or goods to pops, to stabilize the market and build Prestige.
- **Pop.** A group of people in the market who buy goods, work for firms, own firms, and have a mood.
- **Prestige.** Broad, soft standing with the masses.
- **Soft ending.** A non-strict win: "you could keep playing, but you probably won't really like it."
- **Stability.** How secure the government is.
- **Tariff.** A government charge on goods crossing the market's border.
- **Tick.** One day.
- **Trade capacity.** How much can be shipped in and out per day, set by staffing and maintaining trade facilities.
- **Trader.** A ship (or other carrier) that buys and sells across the border. It can be locally or foreign run, with a local or foreign crew.

---

# Open Questions (Summary)

1. Can the player queue or issue orders while paused, and do orders apply at the next tick or immediately?
2. What does "1000 times slower" mean in mechanical terms?
3. How are trade capacity, transport cost and the tariff band calculated?
4. Who runs the traders at the start, and how much of the trade is local versus foreign?
5. How are GDP, the interest rate and the crash consequences worked out?
6. Does the government ever pay down the principal?
7. Do non-coin goods follow the same conservation rule?
8. What exactly does market research reveal, and how long does it last?
9. Is the random sample free and automatic, or an action the player takes?
10. How are wages set?
11. How does the abstract migration work?
12. How does the player control their own owner pay?
13. When does a pop with coin start a new firm instead of living off it?
14. What are the personality traits?
15. How is the buyout price set without a Stock Market?
16. What happens to the failed firm's workers, stock and facilities?
17. Is favor one number for the government overall, or one number per firm?
18. What gets taxed besides trade?
19. How is the capture-versus-nationalization contest tracked?
20. What triggers revolution and breakup, and how much warning does the player get?
21. What drives a regime change, and which government types are in V1?
22. What makes the government outlaw reverse engineering under Patent law?
23. How are these behaviors detected and measured?
24. How exactly are they earned and spent, and do they decay?
25. How does tech spread naturally?
26. What does "research at a discount" mean exactly?
27. What do techs unlock in V1?
28. Which is the default for the bankruptcy setting, and is it tied to difficulty?
29. What are the thresholds for each soft ending, and what happens when you hit one?
30. How many difficulty levels, and are the knobs exposed as a custom game?
31. Are wages paid before or after the market day? Paying before means pops can spend today's wages today.
32. What is the UI for V1: terminal, or a graphics front end?
33. Is the roadmap order right? In particular, should Ownership (pop classes, Stock Market) come before Money?
