# SimplerEconomy
A Simplified Economic simulator, both for simpler play and practice.

# Simpler Economy Ideas and Logic

We'll start by looking over Data Entities, what makes them up, and go from there.

## Wants

Wants are more abstract things which pops and Orgs desire. This could be stuff like food, rest, excitement, or what-have-you. Things which could be gotten from any number of things.

Wants don't survive between rounds in the market, and wants can have additional effects tied to them, either directly or indirectly. For direct examples, Rest may grant increased health (reduced mortality), Food increased Birth Rate, and excitement increased mortality. These effects can be either positive or negative. Indirect Examples, would be moving between states costing Transportation, Political Involvement requiring Fame, and so on.

Wants cannot be traded directly.

Special Wants:

- Wealth. Wealth is special because it's satisfaction comes not from specific goods, but instead the AMV value of goods. As such, it needs ot be handled specially.

## Goods

Goods are the thing(s) which are bought, sold, sought after, created, destroyed, spent, and so on. A Good is desigend to be abstract. A Good is also the Atomic Unit of things which can be exchanged. 
Goods can be partially consumed, but must be traded as whole units.

Goods have 4 primary things they can satisfy.

- Wealth. This is the value of the item it represents in the market. Kind of a special Want (see below), but general and useful enough to warrant being built in. This is equivalent to the current Local Market Abstract Market Value (AMV).
- Wants. These have 3 categories of 'gain' Consumption, Use, Own. Each of these have their own want satisfactions that they grant for each. If one is empty, then it cannot be utilized in that fashion. These are in key-value pairs (Want, eff) and Consumption and Use have time costs attached to them.
- Class. This is a larger class of goods that the Good belongs to. A Class is defined by it's Example, a specific Good that all other goods in it's class point to. This could be something like Bread, Cell Phones, Cars, or any number of other things. The base example never has a Variant Name.
- Specific. This is a desire for the specific good. These are relatively rare.

Additionally, Goods have a decay rate, this is how long it takes for the good to be lost if not used or consumed. Decaying goods can decay into other goods.

Quality is another feature, and acts as a rough estimate of the good's value. It should be equal to the sum of all it can satisfy and acts as the price fator for developing new goods.

Goods also have tags that help define them further in the system. These include tags like Service, the good is always consumed at days end, does not decay into another good, and has no time cost.

Goods also have a physical measure in Bulk, it's size, and Mass, how heavy it is.

Special Goods:
- Time. Used in many ways, created each day for pops, and used in almost all processes.
- Skill. This is an abstract skill, it's variations are more important.

## Processes

Processes are the method by which Goods are transformed into other Goods.

Processes are fairly simple. They take in goods or wants, either consuming them or using them, and produce outputs in goods and wants. Outputs have a grace period of a turn (they are produced after goods are decayed).

A number of inputs can be excluded also, allowing for greater flexibility.

Processes also have a complexity rating which is based around how many goods are used, and how many can be excluded. The more complex a process rates as, the more difficult it is to generate it, but also the more efficient the process is.

## Pops

Pops are the people of the system. Pops create Time, and are the final consumer of many goods. Pops have a storage for goods they own. These goods they wish to own go to satsifying their desires. Desires are defined by various factors, Species, Culture, Religion, etc.

Along with these desires is the unique desire of Wealth which operates alongside their other desires. This acts as an additional buildup for purchasing and demand.

### Desires

Pop Desires are a list of things the pop wants (Wants, Class, and Specifics). These are created in the various subcomponents of the pop, Species, Culture, Religion, and other factors. These are defined with a starting weight, a step interval, and whether they are finite or infinite.

Desires are organized in a list, organized from lowest starting weight to highest starting weight. Any desires with the same starting weight will be scattered around each other randomly (this should be randomized on updating) pop desires. The starting weight acts as the internal value of that good. 

## Firms

Firms are the basic economic unit of the game. They do a number of processes with the intent of producing more value than they consume in inputs. 

Firms come in multiple levels and have different strategies. They can be owned in many different ways.

Firms can own property and the like by various means also, but cannot 'claim' territory like Institutions or States can.

## Institutions

Institutions are non-firm organizations, like Churches, social clubs, political groups, and the like. These are not explicitly economic entities, but usually have a firm contained within them for their 'sustaining' activities. They provide services and needs that are considered 'too important' for businesses to do. They have much broader goals and strategies, while also gaining resources through alternative methods like donations or dues. They are also typically made up of multiple firms, which may have their operations overrided by the institution.

Institutions also claim territory, though they are not exclusive in their claims. They also have membership and rules, though they are (mostly) voluntary organizations.

## States

States are an upgraded version of Institutions, they are typically made of one or more Institutions that have gained a Monopoly on Violence in a region. They can claim territory, and these claims are nearly exclusive. States may push out other states or institutions from their claims. States fight over their claims.

States may create membership and rules and enforce them unilaterally. They typically gain their wealth through taxation.

## Markets

Markets are the centerpoint of economic exchange. It oversees the exchanges and records data for local markets. It helps organize the exchanges as well, but is not 'owned' by anyone. It focuses on local business and territory.

## Territory

Territory is the material, pre-existing conditions and resources. Goods can be dumped into it for 'public' access or extracted from it. Raw resources go into it.

## Map

The Map is the overall handler of Territory. It shows how Territories are connected and manages inter-territory (And intermarket) transportation. It also helps/allows for territories and markets to be consolidated.