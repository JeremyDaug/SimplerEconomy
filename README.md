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

### Process Time

Processes have a time value, which defines the minimum amount of time it takes for a process to occur. To explain how it is used. A firm can only spend a turn's worth of time in sequential processes. If there are 24 hours in a turn, and a process takes 1 hour, then you can do that process 24 times sequentially. 

Note: Firms also have a 'friction' cost for each step they do, representing the time cost of changing what people are doing. This is likely to be a small, fixed, pre-defined cost (currently 0.1 Units of Time). It could be made more dynamic through the used of a 'similarity' rating between processes, but for now we'll just use a fixed value.

## Pops

Pops are the people of the system. Pops create Time, and are the final consumer of many goods. Pops have a storage for goods they own. These goods they wish to own go to satsifying their desires. Desires are defined by various factors, Species, Culture, Religion, etc.

Along with these desires is the unique desire of Wealth which operates alongside their other desires. This acts as an additional buildup for purchasing and demand.

### Desires

Pop Desires are a list of things the pop wants (Wants, Class, and Specifics). These are created in the various subcomponents of the pop, Species, Culture, Religion, and other factors. These are defined with a starting weight, a step interval, and whether they are finite or infinite.

Desires are organized in a list, organized from lowest starting weight to highest starting weight. Any desires with the same starting weight will be selected, ordered by 

## Firms

Firms are the basic economic unit of the game. They do a number of processes with the intent of producing more value than they consume in inputs. 

Firms come in multiple levels and have different strategies. They can be owned in many different ways.

Firms can own property and the like by various means also, but cannot 'claim' territory like Institutions or States can.

## States

States are what players control. States are organizations with a monopoly on legitimized violence. States have a primary culture (this can be changed or hybridized through various means), which they have influence over and represent roughly.

### Power Centers

States are based around 'buildings' called power centers. All States need at least one, this is the capital of the state, and represents the primary starting point for costs and the like. These can be moved for a price and with the right abilites can be made mobile. 

Each power centers have both linear and non-linear costs. The Linear costs are those things which you 'build' into that power center and allow you access to more of your state abilities and functionality. The non-linear costs represent the cost of connectivity and maintaining control peacefully, these scale with both the size/abilities of the power center, the distance from your capital, and various other factors like unrest.

Power centers are typically the centerpoints of cities or towns by nature of the extra money and resources flowing through them due to your resources.

### Resources

Unity.

Violence.

Development.

## Institutions

Institutions are the subcomponents of a state, but are attached to culture. They represent the various abilities, capabilities, and functions of your state. Each can be developed by the state, giving access to more of it's abilites, but these abilities have a cost in both passive upkeep and a material upkeep when implemented. Abilities may be exclusive to each other, requiring you to abandon the first to gain the other. 

Many of these abilities may also be binary sliders going from an aspect on one side to it's opposite on another, like choosing to make your state Centralized vs Decentralized, or being Offensive or Defensive in your war organization.

### Institution Paths

Authority. The central pillar of your state, it defines how you project control and authority internally, and to a lesser extent, externally. The primary advantages it gives are in efficiency of command and control internally (and externally aslo).

Military. Broken into subgroups based on theatre kind (Army/Navy/Air).

Economic. Broken into Facilitation and Control.

## Markets

Markets are the centerpoint of economic exchange. It oversees the exchanges and records data for local markets. It helps organize the exchanges as well, but is not 'owned' by anyone. It focuses on local business and territory.

## Territory

Territory is the material, pre-existing conditions and resources. Goods can be dumped into it for 'public' access or extracted from it. Raw resources go into it.

## Map

The Map is the overall handler of Territory. It shows how Territories are connected and manages inter-territory (And intermarket) transportation. It also helps/allows for territories and markets to be consolidated.