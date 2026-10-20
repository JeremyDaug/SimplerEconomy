# Desires

This summarizes the structure and use of the kinds of desires and how data maps from one level to the next.

## Structure

The structure of the desires has, at the top, Platonic Desires, the kind of abstract, maximalist desires of available to the the system. Platonic Desires are refined down into Demographic Desires, which are 'extant' desires as they exist in a particular game attached to specific demographics. Demographic Desires are refined further into the (Pop) Desire, the working desire of our system and is scaled to match the pop it is attached to. 

From above, Platonic Desires give Demographic desires their platonic_id, to reference back to them. Demographic Desires take a subset of the Platonic bucket and select a tier. The amount can be increased. Effects are rescaled to that amount, so a higher target gives a higher effect. Changing the amount later, through `DemoDesire::set_amount`, rescales the effects to the new amount. Scalar and decay are copied down.

Going from Demographic Desires to Pop Desires scales amount and additive effects to the pop. That runs at day end, after consumption, sentiment, pop growth, the population-cap fit, the under-one prune, internal migration, and decay, so this day's desire effects are spent at the old size. The fit may already have changed household count, and the rescale uses that size. Planning then sees the new size and targets. Satisfaction already recorded is scaled by the same ratio. The next morning's reset multiplies satisfaction by `Desire.decay` (`0.0` clears it). Birth, mortality, sentiment, and satisfaction effects stay at the demographic values. `Pop::update_desires` still only appends a source the pop does not already have. `Pop::rescale_desires` is the day-end pass, and `update_desires` does not call it. Most effects then only need `tiers_satisfied` and can multiply from there.

## Target type

A target is `Consume` or `Use`. Consume moves the take into `consumed`. Decay then adds that good's decay products. Use moves the take into `used`. Decay returns `used` to `quantity` and then decays the aging part, so the good comes back minus its own decay. Fresh units are spared. The pop tester sets `use = true` on an owned good. An omitted `use` is consumed. Satisfy reserves the owned pile before a job can claim it as capital.