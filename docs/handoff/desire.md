# Desires

This summarizes the structure and use of the kinds of desires and how data maps from one level to the next.

## Structure

The structure of the desires has, at the top, Platonic Desires, the kind of abstract, maximalist desires of available to the the system. Platonic Desires are refined down into Demographic Desires, which are 'extant' desires as they exist in a particular game attached to specific demographics. Demographic Desires are refined further into the (Pop) Desire, the working desire of our system and is scaled to match the pop it is attached to. 

From above, Platonic Desires give Demographic desires their platonic_id, to reference back to them. Demographic Desires take a subset of the Platonic bucket and select a tier. The amount can be increased. Effects are rescaled to that amount, so a higher target gives a higher effect. Changing the amount later, through `DemoDesire::set_amount`, rescales the effects to the new amount. Scalar and decay are copied down.

Going from Demographic Desires to Pop Desires scales amount and additive effects to the pop. That runs at day end, after consumption, pop growth, and decay, so this day's desire effects are spent at the old size. Planning then sees the new size and targets. Satisfaction already recorded is scaled by the same ratio. `Desire.decay` is unwritten, and the next morning's reset still clears satisfaction. Birth, mortality, sentiment, and satisfaction effects stay at the demographic values. `Pop::update_desires` still only appends a source the pop does not already have. `Pop::rescale_desires` is the day-end pass, and `update_desires` does not call it. Most effects then only need `tiers_satisfied` and can multiply from there.