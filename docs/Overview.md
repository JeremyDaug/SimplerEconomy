# Overview

Simpler Economy (project name, not final) is a market-oriented grand-strategy game. It takes the long timeframe of a game like Civilization (start from early civilization and go into hypothetical futures), a detailed market economy (akin to Victoria), and a more open-ended construction of civilizations and nations (like Millennia or Stellaris). The keystone of the entire project is the market and its flexible nature. Production, consumption, and logistics are internal factors that players do not directly control in most cases. Players massage those factors, and occasionally bludgeon them, into a useful shape for their own ends.

## Ideas

The keystone of the project is a market-oriented system and a barter-first orientation. There are as few special goods as possible. Any special good is instead a modifier on a good that changes its behavior. Money is not a special good. It is a good that gains a special market status. Time is a good. It is bought, sold, and traded in various forms (relaxation, labor, and so on). Land is a good. Construction and buildings are not limited to slots or arbitrary quantities. They are limited to plots of land and to what that plot can support. Skills are goods. They cannot be traded like normal goods, but they are still priced like goods.

Goods are needed to produce goods. Production is not in abstract resources like hammers, coins, or culture units. It is in units of iron, tools, loaves of bread, and specific coinage. Buildings are not static or magical. They are concrete, and they need specific goods for both construction and maintenance.

Markets and producers are not monoliths. Businesses compete with each other through variable strategies, both within a player's nation, between a player's market regions, and between player nations. Businesses can be started and can operate independently from players.

Decentralized innovation. Research and development is not a magic resource players collect, like Beakers in Civilization. It is generated in a distributed fashion. The benefits are localized and filter outwards from there. Players may accelerate the process and direct it, but they do not have unilateral control.

Multidimensional governance. The player does not run an amorphous state. The player manages a collection of developing, and often competing, institutions. Institutions can be thought of as branches of a nation. They represent both formal power (executive, legislative, judicial, military) and informal power (cartels, religious institutions, powerbrokers, powerful corporations), with only a fuzzy line between the two. Players develop, oversee, and manage these institutions in an attempt to benefit their nation.

Multi-dimensional Man. Pops are not idle resources. They are producers, consumers, owners, and workers. They get paid through profits and wages and use those incomes to purchase goods from the market in an attempt to satisfy their desires. Their status, wealth, and attitudes affect and respond to player actions. They are also broken up by demographic details: species, culture, stratum, and religion. Players can alter and modify these features with some effort, but cannot control them outright. These demographics create effects and desires that apply to the pop and alter the pop's own actions and reactions.

Map and tiles. The map is a hex-based grid with wrap-around, like Civilization. Unlike Civilization, where tiles are unified things, tiles are subdivided into plots of land and into areas with different features and properties. Tiles are collected into regions around settlements. Regions are physical manifestations of local markets. They can be claimed and fought over.

Headless interregional and international trade. Market regions are the highest level of trade. Trade between regions is not itself a market. It is concrete trade routes, moving specific goods at specific costs over concrete distances. Any hierarchy of importance between regions is organic, not pre-defined.

## Versions

- Version 0.1.0 - Pop Tester Alpha, current phase. Focusing work on pops, jobs, goods, processes, and the market. Currently partially complete. Money, skills, and time as market status belong here if they can be tested with pops and jobs. Money may slip later. The emergent selection that picks a money good should be tested early anyway. The day stays on the tester and `Market::market_day`. `PlayState` is not in this cut. This cut ends when there is no more work that can be done with only pops and jobs.
- Version 0.2.0 - Firm Tester Alpha. Focusing on Firms, their logic and reasoning, and their trade. Includes enough wages, owners, and firm strategy to scaffold and test, not the full strategy set. First parts of decentralized innovation live here. Sentiment, migration, and contracts may start, but stay secondary. No `PlayState`.
- Version 0.3.0 - Full Market Alpha. Add in environmental effects, plots/land, terrain, test dynamic friction, and start adding in institutions and a player/state (AI). Stratum starts here. Tech finishes here, with states and institutions. Sentiment, migration, and contracts become meaningful. Migration is not finished. `PlayState` starts here, one market. Save and load of a game starts. The state AI manages the internal market and modifies institutions. A tile is made of plots. A plot is subdivided into units of land. Dynamic friction is a market-size scalar on bulk transport cost. Finer plot and friction detail may change when this cut is tested.
- Version 0.4.0 - Multi-Market Alpha. Markets should be mostly complete at this point, so multiple markets are now the target. This should also be where inter-market firms, institutions, and states are tested as well as trade, travel, and units on the map. Units stay thin. Migration finishes. Save and load finishes.
- Version 0.5.0 - Bevy Alpha and Human-Players. This will be the first 'playable' state. The previous ones are more of testers to ensure everything works out. At this point, we start focusing on graphics and UI, focusing on minimal playability. This cut may move back. Any steps added before it are functional testers and play interfaces, not a feature dump. Those steps are not named yet.
- Version 0.9.0 - Beta Stage. All basic visuals complete and the game is 'playable' but not 'complete'. Goals are Refining Visuals and UI (including replacing any AI art with either procedural art or work from a commissioned artist), creating and balancing the initial factuals. Refining the features of Institutions, Cultures, etc.
- Version 1.0.0 - Release!! Most balancing done, everything needed to be 'public ready' and the game is 'done'. Some balancing and bugfixing will certainly be needed, but the game is effectively "Done".

## Later

These types stay empty until the cut that owns them. Do not fill them early.

- Units (`src/game/unit.rs`). Owned by 0.4. Map actors, including military. Thin until a client can show them. Behavior is not specified yet.
- Tech tree (`src/game/techtree.rs`, nodes in `src/game/tech.rs`). First parts in 0.2, on firms. Finished in 0.3, with states and institutions. It is not a stockpile of beakers.

Buildings and upkeep are goods, not a separate system. They are not a later milestone. They arrive with whatever cut needs them.
