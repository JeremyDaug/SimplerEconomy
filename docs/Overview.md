# Overview

Simpler Economy (project name, not final) is a market-oriented grand-strategy game. It takes the long timeframe of a game like Civilization (start from early civilization and go into hypothetical futures), a detailed market economy (akin to Victoria), and a more open-ended construction of civilizations and nations (like Millennia or Stellaris). The keystone of the entire project is the market and its flexible nature. Production, consumption, and logistics are internal factors that players do not directly control in most cases. Players massage those factors, and occasionally bludgeon them, into a useful shape for their own ends.

## Ideas

The keystone of the project is a market-oriented system and a barter-first orientation. There are as few special goods as possible. Any special good is instead a modifier on a good that changes its behavior. Money is not a special good. It is a good that gains a special market status. Time is a good. It is bought, sold, and traded in various forms (relaxation, labor, and so on). Land is a good. Construction and buildings are not limited to slots or arbitrary quantities. They are limited to plots of land and to what that plot can support. Skills are goods. They cannot be traded like normal goods, but they are still priced like goods.

Goods are needed to produce goods. Production is not in abstract resources like hammers, coins, or culture units. It is in units of iron, tools, loaves of bread, and specific coinage. Buildings are not static or magical. They are concrete, and they need specific goods for both construction and maintenance.

Markets and producers are not monoliths. Businesses compete with each other through variable strategies, both within a player's nation, between a player's market regions, and between player nations. Businesses can be started and can operate independently from players.

Decentralized innovation. Research and development is not a magic resource players collect, like Beakers in Civilization. It is generated in a distributed fashion. The benefits are localized and filter outwards from there. Players may accelerate the process and direct it, but they do not have unilateral control.

Multidimensional governance. The player does not run an amorphous state. The player manages a collection of developing, and often competing, institutions. Institutions can be thought of as branches of a nation. They represent both formal power (executive, legislative, judicial, military) and informal power (cartels, religious institutions, powerbrokers, powerful corporations), with only a fuzzy line between the two. Players develop, oversee, and manage these institutions in an attempt to benefit their nation.

Multi-dimensional Man. Pops are not idle resources. They are producers, consumers, owners, and workers. They get paid through profits and wages and use those incomes to purchase goods from the market in an attempt to satisfy their desires. Their status, wealth, and attitudes affect and respond to player actions. They are also broken up by demographic details: species, culture, class, and religion. Players can alter and modify these features with some effort, but cannot control them outright. These demographics create effects and desires that apply to the pop and alter the pop's own actions and reactions.

Map and tiles. The map is a hex-based grid with wrap-around, like Civilization. Unlike Civilization, where tiles are unified things, tiles are subdivided into plots of land and into areas with different features and properties. Tiles are collected into regions around settlements. Regions are physical manifestations of local markets. They can be claimed and fought over.

Headless interregional and international trade. Market regions are the highest level of trade. Trade between regions is not itself a market. It is concrete trade routes, moving specific goods at specific costs over concrete distances. Any hierarchy of importance between regions is organic, not pre-defined.

## Versions

Version cuts are not chosen yet. When a cut is decided, write it in this section: what the player can see, what the simulation must already do, and what stays out.

## Later

These are intended features. The types are empty shells. They stay in the tree until a later pass fills them in.

- Units (`src/game/unit.rs`). Map actors, including military. Behavior is not specified yet.
- Tech tree (`src/game/techtree.rs`, nodes in `src/game/tech.rs`). How the tree sits beside decentralized innovation is not specified yet. It is not a stockpile of beakers.
