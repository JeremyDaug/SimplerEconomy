# Purpose

This is the new design document for our project. Anything written here takes precident over previous design documents. This is meant to be an overarching view of the design and structure as well as thoughts about how things should operate broadly. This is not meant to be a definition of code, but rather an explanation of intent and design choices.

# Explaining the idea.

## Market Structure

### Tile

Tiles have a few primary pop groups, divided up further by demographics, and in the case of workers, crafts.
- Landowners, these are pops who own at least 1 plot of land, create and develop that land, then rent it out to everyone else (technically including themselves). This is not pure rent, but rent, maintenance, and development.
- Business Owners, these are pops who own firms, either in part or in full. They gain a small fixed wage from the firms they own, as well as a portion of profits, while providing management labor to firms to keep them functional.
- Workers, people who do work. They either do simple, cottage work, sell their skills and labor to firms, or retreat into subsistence work.
- Institutionalists, people who work for institutions, dedicating their time to supporting an institution of some sort, who in turn gives them their wages.
- Governmentals, people who work directly for the government and are paid wages by the government.

As some notes, Institutionalists and Governmentals blend between each other based on how integrated the institution is into the nation. Military soldiers can pull from workers, institutionalists, or governmentals.

The broad strokes trade between these: 
- The Landowners own and maintain land, buildings, and improvements which they rent out to everyone else in the tile.
- Business Owners own firms, giving their labor over to the firm as management, and getting profits from firms and using most of that money to re-invest into other ventures. By proxy of the firms, they buy the inputs for firms, and sell their outputs. They also buy from landowners.
- Workes produce labor or fill in gaps of the market, selling labor and cottage crafts, buying everything else from the market, and renting from landowners.

Institutionalists and Governmentals can produce and sell goods, but much of their labors are directed to supporting institutions and the government, while purchasing goods with their wages.

### Region

Tiles are brought together into regions. Anything a Tile cannot cover itself internally goes to the regional market, where they have to pay for market friction. Friction is normalized cost of moving a good within a market region.

Market regions also act as a second layer, letting the pop types mentioned above interact with other tiles. Land and Business Owners don't need to own locally, Workers can also work distantly, while Institutionalists and Governmentals rarely work locally.

Regions are where the local price is defined, and where supply and demand meet.

### Global

The Global market is not another collection of Regions, but the logical layer where regions interact between each other. There is some management at this level, but it is focused on dealing with inter-regional ownership and interactions. Global prices are calculated here, but it is by aggregating regional prices, not by pushing Prices around by supply and demand.

This region is mostly a flat network of Regions trading between each other to find opportunities for profit and trade.

## Market Trade and Money

A key feature of the Market is that there is no singular currency baked in. We do not use gold Ducats, Pound Sterling, Global Dollars, or Petro-dollars, or any other shared currency, instead the system allows for multiple competing currencies. 

Currencies come in 3 varieties:
- Moneys, which are commodity currencies. 
- Tokens, also known as Feduciary Currency, which is paper currency backed by some other commodity, which need not be a Money also. It's issuance and stability is tied to the issuer being able to meet obligations. 
- Credit, or Fiat money, which has no backing, but is tied to the faith, credit, and management of the issuer.

All currencies can be traded at 1/1,000th of a whole unit, making trade easier to handle, and all currencies are treated as equally valid as forms of payment.

Currencies can have different prices, and their prices move like all other goods, if slightly slower. Tokens are slightly unique in that their price movements are tied to the good they represent, pulling both of them along. The only exceptions to this are legal exchange rates, which fix the exchange ratio between two or more currencies in a market, causing both to snap back into position at the end of the market day.

## Prices and Price Movement

There are 2 fundamental categories of markets, which divide where goods can be sold and how they are priced. 

The Open Market is where most goods go and is for those goods which are often produced and used directly, often with little hard limitations on them. It's where food, steel, and rents are paid, among many other things.

The Stock Market is not just a market for trading company shares, but instead where pops buy and sell much more restrictive goods, like Company shares or plots of land. Instead of operating by pure supply and demand, it operates under a more refined and complicated mechanism.

### The Open Market

Prices for goods in each region are tracked by an Abstract Market Value (AMV). That value effectively represents it's hypothetical value in the market. It's not necissarily accurate or at equilibrium, but represents the current exchange rate from that good to any other good.

The price moves acording to simple supply and demand equilibrium calculations. If there is a shortage, the price goes up, if there's surpluse it goes down.

There is a minimum price at 0.0001, and a maximum price equal to 1/10th of all currency in the market.

### The Stock Market

The stock market is run different from the open market. Rather than dealing with buys and sells at the market price, the stock market deals with buy Maximums and Sell Minimums. Potential buyers create buy offers with a maximum price they would accept and how much they want to buy.

# Actors

## Pops

As mentioned in the Market Structure > Tile section, pops fall into a handful of different types but otherwise act mostly the same.

Pops have property they can own and store. 

Pops have desires which define what they consume. This is based on their demographics, which while having a baseline, can be modified and added to by players to refine their demographic needs. These desires are explained more in the Desire section, but to summarize as Pops care about them.

Desires have one of three Tiers, Basic, Common, and Luxury. Basic are those things which are fundamental to the pop living and typically come from species or ethnic demographics, not meeting basic needs results in starvation and rapid
decline in the pop. Common are the more common needs, meeting these satisfies a pop into contentment, while not meeting them angers the pop. Luxury needs are those things which they view as valuable and nice to have, but not necessary for their life to be 'good'. Not meeting them does nothing, but meeting makes them happier.

Desires are buckets of goods which a pop will try to buy and consume to meet their needs. They seek to consume a minimum of each, but will accept consuming more of one if they have no choice. That just guarantees they'll produce demand for all their bucket of goods.

Between each tier, they also attempt to build up savings. Typically measured in 'Basic Needs AMV Equivalent', which is the AMV value equal to the sum of good prices going into all basic desires, assuming that they are split evenly between goods. That's our Savings Rate.

Purchasing strategy for buying/saving is as follows:
1. Process inputs.
2. Basic Needs.
3. Savings 1 (1 x Savings Rate)
4. Common Needs.
5. Savings 2 (3 x Savings Rate)
6. Luxury Needs.
7. Savings 3 (5 x savings rate)
8. Repeat 6 for 1 more tier
9. repeat 7 for 2 more days of savings.

The last two steps are just clarifying that Luxury Needs and savings 3 repeat until the pop runs out of money to spend or save. Additionally, savings is cumulative, so it's effectively 1 Savings rate for the first savings, then 2 for each of the following levels being added to savings.

When in the savings step, regardless of actor type, they will try to purchase more durable forms of currency. They can do this repeatedly with less durable currencies so long as the currency being traded is less durable. 

IE, Gold is most durable, copper next, grain last. A pop with some copper and grain will try to purchase gold, whatever gold it gets, it will save, then it will try to buy copper. Once it buys copper it saves up copper. Grain is the least durable, so it does not attepmt to purchase it. This can be stopped early if they reached their savings targett.

Demographics are the data about the pop, defining species, ethnicity, language, culture, class, religion, and ideology. These are more similar than different, but each is a different way of defining who an individual belongs to and what they need demographically to be 'happy'.

- Species and Ethnicity are unique in that they can effect Basic needs. 
- Language does not add desires and is more of an assimilationist mid-point. 
- Culture starts as a catch-all for everything that isn't biological, but can split off the others as the game progresses. 
- Class is a sub-component to Culture and defines economic differences. It can be broken up in multiple ways, but most cultures should expect to have unique Land Owner, Business Owner, and Governmental/Institutionalist classes. It may also have sub-classes for workers, defining upper, middle and/or lower classes for workers as well. And it may be tied to multiple other demographic rules, like having a specific priestly class tied to a particular religion.
- Religion typically splits off of culture, but is entirely independent from it in most cases, and often defines more 'ritual' needs, which reduce anger and improve contentment or happiness while also seeking out rarer or more esoteric goods. 
- Ideology is the final good and acts like a second religion in many ways (may even be fused with it to create a combined Worldview demograhpic). It tends to have the least direct effect on desires, but gives lots of ways to manipulate activism and political effects, especially in more democratic societies.

Pops are made of households. Which are divided up by sex and age category, and include many factors to modify the growth rate. Births per woman, Infant mortality (death of a baby before they count as a child), Maternal Mortality (death of the mother in childbirth), Mortality rates for each cohort and sex in each cohort. This full breakdown of the house allows for detailed modification and alteration to the household, though it does build in human biology norms.

The household also allows for more targeted desires and benefits. Extra desires for children, adults, elders, as well as male and female subtypes. For example, extra Development for each existing elder, improved happiness for children, improved military recruitment per adult male, and so on. 

Pops also have a simple mood called Sentiments. It has only 3 categories, Angry, Content, and Happy. Angry occurs when they cant get their needs met and when their Standard of Living declines. Happy occurs when they are able to get their needs met and then some, as well as when their Standard of Living Improves. Angry pops build up resentment, which can produce revolts, and reduce government resource outputs. Happy pops produce more loyal (harder to conquer) regions and increase government resource outputs, while also being more generous in general.

### Workers

Workers have the most complexity, but even then they are simple. They have 3 things they can do, Subsistence, Craft, or Labor and try to do the one that produces the most value for themselves (per household). Subsistence takes a bit of land and produces bost basic needs and a small number of common needs. A Craft is a singular secondary business they do, buying inputs and selling the outputs for both personal use and potential profit they can sell to the market. Labor is converts their time and skills into labor they can offer on the market for a wage.

Each tile has a soft cap on how many crafts it can have at once, there is no cap on labor types per tile, but they only form if there is enough demand. Each tile should have a soft cap of 5 crafts. For every craft after that 5th, the chance of a new craft being added declines by some amount (does not go to 0% chance) and migratory pressure between Workers in the tiles increase. This may be removed, but should help keep every tile from having every possible craft.

Workers dedicate up to their time in 5% increments to each part, preferring whichever is most profitable and stable. Profitability for subsistence is based on the output market value of the goods it produces and consumes directly. Craft and labor profitability also takes into account input expenses, losses from decay, as well as actual product sold, not just raw value produced, using both an expected value and prior values as guides.

### Landowners

Landowners are pops who own at least one plot and their goal is to find the best possible use for that land and rent it out to someone else. Landowners cover Construction, Maintenance, and redevelopment as well as infrastructure and they constantly want to find better ways to put their land to use and maximize their rents.

Plots are effectively buildings in other games, and are treated as goods. While some require no maintenance (open fields, wilderness, etc) most do have at least some amount of maintenance required to upkeep them. Because of the costs and possibilities of over-investing, they will only ever convert a fraction of their owned plots (minimum 1) at a time.

75% of a Landowner's time is always spent on management of their property, though they may purchase more from the market if they need.

### Business Owners

Business Owners are pops who own at least one share of a firm directly. They are responsible for covering the management costs of a firm up to the point at which point they can purchase management labor. If the can't cover it, the firm suffers penalties on dynamism. They always dedicate at 75& of their time to this.

Business Owners do very little directly, instead focusing most of their internal planning efforts on buying and selling stock, as well as giving out direct investments and loans.

Business owners get an additional buy phase for each luxury loop where they may seek out more shares to purchase. If they cannot reach some level of satisfaction (likely unable to reach any luxury desires) they will seek to sell off some shares to make up for the loss.

### Institutionalists and Governmentals

Terrible names, but very explicit in what each does. These are pops that work for institutions and governments. They often provide 80% of their time as whatever labor is needed and specialized for governments and institutions. They can influence where they work politically, but are generally not very complex.

## Firms

Firms are more complex economic entities. Effectively corporations or businesses with some form of internal property distinct from that of the owners. They maintain their own budgets run their own plans, and generally operate independently of others around them, but they have no consumption outside of their inputs. Revenue is paid out in profits, wages, and inputs, which also includes capital maintenance/replacement.

Firms come in one of 4 levels.
1. Firm, the lowest level, it only owns/operates property in a single tile.
2. Companies, Operates in the region and can own and oversee firms.
3. Corporations, Operates between regions, has a HQ in one region, can own companies and firms.
4. Megacorps, Can directly own regions themselves as well as any lower level firms.

When structuring them, it is a tree structure. The separation between 1 and 2 is mostly abotu management and control, rather than necessity. A company can directly operate all of the firm's property within a region, or it can distribute it across multiple smaller firms. In the same firm tree, two firms cannot exist in the same slot. A company has a home tile, and no sub-firm it owns can also be in that tile. Likewise, a corporation has a HQ region, and there can be no company in that region it also owns. Same for megacorp, though they tend to have their HQ in the region they directly control. Whenever a firm is purchased that would clash with this, they are absorbed into the firm in the same location, regardless of that firms level.

### Firm Ownership

At game start, firms have singular, or a small number of owners and basically no shares to trade. As the game progresses, the number of shares a firm can have, and the highest level of firm that can exist in a player's territory is increased. It would be something like 10x the number of shares per certain technologies and 100x per level up of the highest level.

Firms can also own other firms and come in 2 states, partial, and total. Partially controlled firms are treated just like owners. They get a share of the profits and need to give a certain amount of management labor to keep them functional. It's very much just a Business Owner in Firm Form. A higher level firm may want to purposefully restrict itself to partial ownership.

Total ownership occurs when they have at least 50%+1 of the shares of a particular firm, and they want to merge. In doing so, they may buy out the rest of the shares and integrate them fully, turning them into a sub-firm within the larger firm. Any overlap on tile or region is merged immediately.

A subfirm within a firm shares everything with the higher level firm, including stock, profits, and the like. The only barrier is that they must transport between their sub-inventories to make full use of it.

### Shares and Share prices

Shares, like plots, have 2 valuations, their static valuation, and active valuation. Unlike Plots, they only have the expectation of management time being handed over, nothing else. Firms tend to otherwise take care of themselves.

Static Valuation is the estimated liquidation value of the company. Take all of it's stock and capital, sell them off, then divide by shares to get the static valuation.

Active valuation is the rolling average dividend of the shares. 

When buying and selling shares, these values are used to help define the price it should be roughly traded around.

### Budgeting

Firms have much simpler budgets than most. They start with their existing stock of currency and calculate how many iterations they can run with that budget and try to target roughly that value, capped at a wider strategic plan.

With the target set, they buy the various inputs they need as well as the managerial labor. Management labor always comes from owners first, then it can be purchased from the open market. 

Any money leftover after this is then divided. 10% is distributed as a dividend (rounded down for equal payment to each share), the remainder is saved for tommorow up to a point. They try to target a number of iterations in savings. Defaults to 20x their current plan iteration needs. If they reach this 20x target, the remainder is paid out as bonus dividends (again, rounding down for equal payment to each share).

Also a reminder, this savings for later also gets a chance to seek out more durable currency, like pops can.

After all of this, they do their production at alongside pops crafting.

This order is rough, but with the large currency buffer they target, they should have everything they need in general, only when low on savings should they run into a problem which will need some special handling, mostly lowering the targets to ensure they can still buy with what savings they have.

### Standing Up

New firms can be stood up by any pop, it does require a fair bit of starting capital however. A pop can create a firm using it's savings to purchase the first few iterations of what it needs. When it does this, it becomes the sole owner at the start.

### Collapse, Bankruptcy, and illiquidity

When a firm becomes unable to do anything, it liquidates itself, the owner(s) get the value of the liquidated captial and goods, then they move elsewhere, either buying up shares or becoming a different kind of pop. Typically, this should only be considered when a firm has less than 1 day of savings at any given time. At this point, there is a chance it will either immediately sell off and scale back, or liquidate itself entirely.

Buyouts and Mergers are also options when a firm is distressed, but that requires some other actor to be willing to buy it out and possibly even merge it. Pops tend to to buyouts. Firms prefer Mergers, though some (like investment firms) may prefer buyouts instead.

Another sign of a distressed firm is one which is highly illiquid. Lot's of output stock, not much currency. Illiquid companies will often push to find ways to get liquidity to make ends meet.

### Firm Types

Firms come in a few types that direct how they'll operate more than anything.

- Producers, focus on running processes to turn less valuable goods into more valuable goods.
- Banks, lenders who offer alternative forms of savings as well as possible loans to help pops and firms out.
- Investors, a firm who seeks to buy and sell shares in other companies, centralizing and mixing profits for more risk averse investors.
- Merchants, buys goods locally to sell them at a profit locally. They get special wholesale price bonuses and first dibs on purchasing from other firms. They also specialize in storage and maintenance of goods.
- Traders, buys goods locally to sell them in other markets. Has a special phase it operates in (inter-market phase). It can also offer regional transport services.
- Institutional, a special category that denotes them as connected to an institution, and lets them produce unique goods and do unique processes.
- Governmental, a special category that covers everything the government can do, from bureaucracy, to military.

I expect some of these may not be immediately possible or available in early versions, but at minimum, Producers, Merchants, and Traders should be.

## Institutions

Institutions are organizations that are not purely economic, like firms, but still independent, either partially or fully, from the player. 

Institutions are run by Institutionals, pops who expend their time and labor for the institution in return for a wage. Typically, the Institution has some number of attached firms that either produce and sell or otherwise recieve currency. Institutions can produce rare or unusual goods for the market. Most institutions come with a 'tax' of some kind, but some don't and many can be funded by government taxation as well.

### Bonuses

Many of these institutions produce effects that scale with a few different ways.

1. Desire Proxy, the institution's bonuses come in the form of special desires attached to demographics that the institution satisfies. Satisfying those desires gives bonuses.
2. Region Tile Scaling, the effect is a physical effect, covering space and producing effects from that.
3. Per household/person, the effect is an effect on people, and thus requires enough to cover people. It can scale with household, total population, or sub-components of the household.
4. Limited, targets a specific number of actors or locations.

Bonuses often involve making the nation easier to run and manage, but can include many useful things.

Another, common way that institutions give bonuses is by givinsg access to new features or abilities. Migratory organizations, control over firms, restricting the purchase of land, alternative buy/sell paths, profit sharing, and so on.

### Example Institutions

- Government > State, this is the institution form of the player. They have direct control over this, though they may tighten or loosen as they play, giving them more control and feedback. It oversees the concrete parts of the state. Including the taxation and bureaucracy.
- Shamans > Priesthood > Church, Starting as distributed and informal shamans, this is the religious institution that's highly flexible with what bonuses it gives. At it's core, it emphasizes reducing anger and fear by offering religious services. It often takes a small amount from each follower in the form of a tithe. It can also be used to support other institutions.
- Aristocracy, a generic, general institution which focuses on keeping a cadre of special people around for you to draw upon for leadership or special skills as needed. Typically used to manipulate Landowners, Business Owners, and create an elite warrior class of varying kinds.
- Warriors > Army/Navy, Military is one of the few you can have multiples of, with the idea being that each is specialized into a field or kind of war. ARmy and Navy being the 'default' options people are expected to build. 
- Merchant Guild / Corporate Champions / Megacorp, these are variations of the same idea and multiple can also be had to some extend if properly specialized. They give the player more influence and control over their private economy, through mercantile proxies. Often they are still independent, but they can stretch your reach and economic power further than the government alone can.
- Legistlature, a law-making body which helps create rules and refine how you operate and the laws you make while also reducing costs for implementation and maintenance.
- Judiciary, a law-adjudicating body which helps enforce the laws you make, reducing their costs and improving the happiness of your pops.

### Structure

Institutions are much more simply organized than firms, they have a local organization which handles resources local to a region, then the global organization, which organizes between those regions. One region is often considered the Headquarters, like a firm, but that's not manditory (distributed/decentralized institutions). That HQ is where possible centralised control and influence can be produced, and typically requires a greater amonut of support than branch institutions elsewhere.

### Institutions Spreading

Some Institutions can spread on their own to an extent. Most institutions can be defined as spreading either at will, internal, or external. At-Will expands only when dirceted to by the player. Internal can expand, but only within the territory of the player. External can expand between players, though at greater difficulty than moving internally. Spreading has a cost, typically in the form of moving the goods and people, while organizing them to travel successfully. What those goods are, how many people, and what is needed to organize them is dependent on the institution and it's current state.

### Design and Ability Organization

When it comes to what an Institution is and it's abilities, institutions are ability trees. The first level is the root and gives the initial powers and abilities. From there, resources, typically some combination of Development, Authority, Resources, or Institution specific resources (Faith, Trust, etc), but mostly Development. They would also have an upkeep cost, both in literal terms of the institution itself suffering if expanded too quickly, and in costs to the player, often reducing Development.

It branches and recombines like many skill or tech trees do, though some branches can lock out other branches.

Details of what these would look like is too early to define here, beyond saying most would give abilities and bonuses. Bonuses from it's internal support, new desires available to add to your pops, new goods to add to your market, and so on.

## Nation / State / Government / Player

A lot of alternative names to mean the Player. This is where the players control from and distingishes the nations. Regions tend to have an owner, though some can be owned by 'city-state' players, who cannot make more cities. They directly control the Government institution, which acts as their 'physical' mirror in the game, while they deal with higher level resources and abstractions directly. 

Players have 4 resources.

- Unity, the resource to overcome and maintain a unified nation, including keeping both people and institutions happily working for you.
- Development, a resource that is used to alter how the player and those things under his control (institutions, demographics, technology, etc). In general, it's collected passively based on the activity of your nation, the larger, wealthier, and happier they are, the more they produce.
- Legitimacy, a measure of the people's willingness to go along with the player without struggling. You ask through legitimacy, and they volunteer to cover it. This is also generated by happy pops, though it is not infinite, but instead builds up and decays, meaning it has a soft cap. Spending it will decrease the current quantity, but it will build back up again.
- Authority, a measure of your ability to force your will upon your people. Instead of being created by the people, it's created through mechanisms of force: Military units, Police force, Religious officials, etc. It acts similarly to Legitimacy, but guarantees you'll be able to get what you want. While spending too much will leave your pops increasingly angry and unhappy.

Legitimacy and authority are very similar in most ways. Legitimacy comes from pop happiness, and effectively allows you to spend that for additional control. Authority is your ability to force your people to obey. Both of these can be spent into the negative, but doing so results in increases to anger. The Sum of Legitimacy and Authority is a measure of stability, if it's too low, then revolts, unrest, and disobedience are likely to occur.

Unity is mostly a maintenance resource, expended to overcome issues, while Development is meant to improve or expand the capabilities of your nation, demographics, techs, and institutions.

# Units

These are the entities you control on the map and they can represent many things. Above everything, they are a bundle of pop refernces, resources, and traits. They point back to the pops that make them up, often represnting a portion of them (adult men for most things, percentages of others for various odds and ends). Resources that they need, most commonly, food and tools. And traits which define who controls them, what they can do, and in general, what they are for.

All units have a speed, which defines how quickly they can move.

Some traits include:
- Military Unit, They have military factors, strength at minimum, but it could initiative (who goes first), split strength in to offence and defense, range, penetration and hardness, and probably a few others. This should have multiple levels of trait, representative of tech. Ideally, this trait should be designable by the player to give flexibilty and strength to the player interms of what strengths/weaknesses it has, and it's stats, while cerating room to design it to local needs (choosing bronze over Iron, wood over copper, etc.)
- Worker Unit, A specialty unit, designed to do work and modify terrain outside of regions. Mostly meant for roads as well as preparing a tile for expansion into it.
- Trader, a unit which a player can create and control to buy and sell goods at their own leisure.
- Caravans, a support unit, not really controlled, but moves between other units and their supply stations.
- Naval units, a modifier to most others that allows it to travel along water, often includes the ability to transport other units.
- Amphibious, Can travel on both land an see, but much more expensive.
- Settlers, a large group of population that can be formed up and moved out. Settling them on unclaimed territory creates a new settlement, while 
- Explorers, A unit which can go out and discover new tiles and explore them quickly and efficiently.

Units are formed in regions and move out from there. They need support in the form of caravans or supplies which need to be purchased and supplied to them.

# Map

The map is a hex map made of our tiles. Each tile can have claims from multiple sources, and there are multiple levels of claims.
- Unclaimed, the tile is seen, but otherwise has no interaction from the player. Anyone can do anything without consequence.
- Interested, the player has poked at it and put down a small claim just to signal they 'might' do something with it. Other players can claim or interact, but reactions will be muted.
- Pursuing, the player has stated showing interest in the tile. 
- Claimed, The player has actively claimed it and is seeking to own it. Taking it will cause conflict.
- Owned, Only Reachable once a player's region gets it. The player currently owns it in one of it's tiles, trying to claim or take it is a hostile act.
- Occupied, Only achieveable by conquest, a temporary claim state that makes the tile held back from the region it's attached to.

Tiles also have a 'known' factor, with a few values.
- Unknown, no information about it exists because the player has not seen it.
- Uncharted, The basics of the tile is known (type, traversal issues) but other details aren't.
- Explored, All surface resources have been discovered and explored.
- Prospected X, All resources currently available have been explored. The X is the depth of exploration, higher tech lets deeper prosepecting and thus higher chance of resources.

Lastly is the fog of war, but rather than Binary has a few levels.
- Hidden, unit information is completely hidden.
- Scouted, Unit existance and type is known.
- Seen, Unit existance, type, and size is known, but no further details.
- Infiltrated, Unit existance, type, size, and details are visible. This might be ignored and combined with Seen until some kind of spy or espionage system is available.

# Turn Order

Turn order is: (TBR)

1. Start of day (time generation).
2. Environmental phase.
3. Player Action Phase (map actions, unit movements, etc)
4. Apply player demographic changes and bonuses.
5. Actor Reservations
6. Intra Market Day
7. Inter Markte Day
8. Production
9. Consumption
10. Pop Sentiments
11. Growth
    1.  Rebalance if capped, prune households, local migration.
12. Decay,
13. Rescale from scaling.
14. Record keeping
15. Planning

# Definitions

Standard of Living (SOL) - The level of material wealth and safety a pop has. Measured by it's ability to satisfy it's desires and build up it's savings. SOL values should be able to track how many levels of Standard of Living equates to each level of desires.

# Abbreviations and notation

TBR - To Be Refined, current values are subject to change with details.