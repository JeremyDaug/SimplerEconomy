# Desires

This summarizes the structure and use of the kinds of desires and how data maps from one level to the next.

## Structure

The structure of the desires has, at the top, Platonic Desires, the kind of abstract, maximalist desires of available to the the system. Platonic Desires are refined down into Demographic Desires, which are 'extant' desires as they exist in a particular game attached to specific demographics. Demographic Desires are refined further into the (Pop) Desire, the working desire of our system and is scaled to match the pop it is attached to. 

From above, Platonic Desires give Demographic desires their platonic_id, to reference back to them. Demographic Desires take a subset of the Platonic Desires Bucket, no scaling is applied here. Demographic desire also selects a tier for the desire to go to. Most other data is a direct copy down.

Going from Demographic Desires to Pop Desires, this refines it further, scaling effects and amount appropriately, so that it matches the size of the pop. This is all done once near either near the start of the day, or after pop growth near it's end. This means that most effects and modifier should only need to get the desire.tiers_satisfied() to get the factor of success and multiply the other effects as needed from there.