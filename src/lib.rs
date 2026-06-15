pub mod game;

#[cfg(test)]
mod test {
    use std::collections::HashMap;

use crate::game::good::Good;

    fn make_good(id: usize, name: &str, decay_result: HashMap<usize, f64>) -> Good {
        Good {
            id,
            name: name.to_string(),
            class: None,
            tags: Default::default(),
            decay_rate: 0.0,
            decay_result,
            // add any other fields your Good actually has
        }
    }
        
    mod map {
        mod wrap {
            use crate::game::map::Map;
            use hexx::Hex;

            #[test]
            fn tests() {
                let testmap = Map::new(10, 10);

                let underrow = Hex::new(6, -4);
                let overrow = Hex::new(5, 15);
                let undercol = Hex::new(-7, 3);
                let overcol = Hex::new(12, 2);

                // no wrapping
                assert!(testmap.wrap(underrow).is_none());
                assert!(testmap.wrap(overrow).is_none());
                assert!(testmap.wrap(undercol).is_none());
                assert!(testmap.wrap(overcol).is_none());

                // full wrapping
                let testmap = testmap.with_hwrap().with_vwrap();

                let undershoot = Hex::new(-4, -3);
                let overshoot = Hex::new(15, 13);

                if let Some(underres) = testmap.wrap(undershoot) {
                    assert_eq!(underres.x, 6);
                    assert_eq!(underres.y, 7);
                } else { assert!(false) }

                if let Some(overres) = testmap.wrap(overshoot) {
                    assert_eq!(overres.x, 5);
                    assert_eq!(overres.y, 3);
                } else { assert!(false) }
            }
        }

        mod get_region {
            use crate::game::map::Map;
            use hexx::Hex;

            #[test]
            fn tests() {
                let mut testmap = Map::new(5, 5);
                testmap.tiles[1][1].region = Some(0);

                // check one from each grouping. Row zero should return none.
                assert_eq!(testmap.get_region(Hex {x:0, y: 0}), None);
                assert_eq!(testmap.get_region(Hex {x:1, y: 1}), Some(0));
            }
        }

        mod find_city_hex {
            use crate::game::map::Map;
            use hexx::Hex;

            #[test]
            fn tests() {
                let mut testmap = Map::new(5, 5);

                testmap.regions.push(vec![Hex {x: 2, y: 2}]);
                testmap.tiles[2][2].region = Some(0);
                testmap.regions[0].push(Hex {x: 1, y: 2});
                testmap.tiles[1][2].region = Some(0);
                
                testmap.regions.push(vec![Hex {x: 4, y: 4}]);
                testmap.tiles[4][4].region = Some(1);
                testmap.regions[1].push(Hex {x: 4, y: 3});
                testmap.tiles[4][3].region = Some(1);
                
                testmap.regions.push(vec![Hex {x: 3, y: 1}]);
                testmap.tiles[3][1].region = Some(2);

                // check both exact find, secondary, and failed find
                if let Some(res) = testmap.find_city_hex(Hex { x: 2, y: 2 }) {
                    assert_eq!(res, Hex { x: 2, y: 2 });
                } else { assert!(false); }

                if let Some(res) = testmap.find_city_hex(Hex { x: 4, y: 3 }) {
                    assert_eq!(res, Hex { x: 4, y: 4 });
                } else { assert!(false); }

                assert!(testmap.find_city_hex(Hex {x: 0, y: 0}).is_none());
            }
        }

        mod get_city_hexes {
            use crate::game::map::Map;
            use hexx::Hex;

            #[test]
            fn tests() {
                let mut testmap = Map::new(5, 5);
                testmap.regions.push(vec![Hex {x: 2, y: 2}]);
                testmap.regions[0].push(Hex {x: 1, y: 2});
                
                testmap.regions.push(vec![Hex {x: 4, y: 4}]);
                testmap.regions[1].push(Hex {x: 4, y: 3});
                
                testmap.regions.push(vec![Hex {x: 5, y: 1}]);

                // check one from each grouping. Row zero should return none.
                let res = testmap.get_city_hexes();
                assert_eq!(res.len(), 3);
                assert_eq!(res[0], Hex {x: 2, y: 2});
                assert_eq!(res[1], Hex {x: 4, y: 4});
                assert_eq!(res[2], Hex {x: 5, y: 1});
            }
        }

        mod get_region_neighbors {
            use crate::game::map::Map;
            use hexx::Hex;

            #[test]
            fn tests() {
                let mut testmap = Map::new(5, 5);

                // set up a test region of size 1.
                testmap.regions.push(vec![Hex { x: 3, y: 3}]);
                // check that all neighbors are there by removing matches.
                let neighbors = testmap.get_region_neighbors(0);
                assert_eq!(neighbors.len(), 6);
                assert!(neighbors.contains(&Hex { x: 2, y: 3 }));
                assert!(neighbors.contains(&Hex { x: 4, y: 3 }));
                assert!(neighbors.contains(&Hex { x: 3, y: 2 }));
                assert!(neighbors.contains(&Hex { x: 3, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 2, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 4, y: 2 }));
                // check larger setup.
                testmap.regions[0].push(Hex { x: 2, y: 3 });
                let neighbors = testmap.get_region_neighbors(0);
                assert_eq!(neighbors.len(), 8);
                assert!(neighbors.contains(&Hex { x: 4, y: 3 }));
                assert!(neighbors.contains(&Hex { x: 3, y: 2 }));
                assert!(neighbors.contains(&Hex { x: 3, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 2, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 4, y: 2 }));

                assert!(neighbors.contains(&Hex { x: 1, y: 3 }));
                assert!(neighbors.contains(&Hex { x: 2, y: 2 }));
                assert!(neighbors.contains(&Hex { x: 1, y: 4 }));
                // expand to an edge which doesn't wrap
                testmap.regions[0].push(Hex { x: 4, y: 3 });
                let neighbors = testmap.get_region_neighbors(0);
                assert_eq!(neighbors.len(), 8);
                assert!(neighbors.contains(&Hex { x: 3, y: 2 }));
                assert!(neighbors.contains(&Hex { x: 3, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 2, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 4, y: 2 }));

                assert!(neighbors.contains(&Hex { x: 1, y: 3 }));
                assert!(neighbors.contains(&Hex { x: 2, y: 2 }));
                assert!(neighbors.contains(&Hex { x: 1, y: 4 }));

                assert!(neighbors.contains(&Hex { x: 4, y: 4 }));
                // let wrapping occur
                testmap.hwrap = true;
                let neighbors = testmap.get_region_neighbors(0);
                assert_eq!(neighbors.len(), 10);
                assert!(neighbors.contains(&Hex { x: 3, y: 2 }));
                assert!(neighbors.contains(&Hex { x: 3, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 2, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 4, y: 2 }));

                assert!(neighbors.contains(&Hex { x: 1, y: 3 }));
                assert!(neighbors.contains(&Hex { x: 2, y: 2 }));
                assert!(neighbors.contains(&Hex { x: 1, y: 4 }));

                assert!(neighbors.contains(&Hex { x: 4, y: 4 }));
                assert!(neighbors.contains(&Hex { x: 0, y: 3 }));
                assert!(neighbors.contains(&Hex { x: 0, y: 2 }));
            }
        }

        mod is_region_neighbor {
            use crate::game::map::Map;
            use hexx::Hex;

            #[test]
            fn tests() {
                let mut testmap = Map::new(5, 5);

                // set up a test region.
                testmap.regions.push(vec![Hex { x: 3, y: 3}]);
                testmap.regions[0].push(Hex { x: 2, y: 3 });
                testmap.regions[0].push(Hex { x: 4, y: 3 });

                testmap.tiles[3][3].region = Some(0);
                testmap.tiles[2][3].region = Some(0);
                testmap.tiles[4][3].region = Some(0);

                // within region
                assert!(!testmap.is_region_neighbor(0, Hex { x: 3, y: 3 }));
                // out of bounds neighbor
                assert!(!testmap.is_region_neighbor(0, Hex { x: 5, y: 3 }));
                assert!(!testmap.is_region_neighbor(0, Hex { x: 10, y: 3 }));
                // wrapped neighbor
                testmap.hwrap = true;
                assert!(testmap.is_region_neighbor(0, Hex { x: 5, y: 3 }));
                assert!(testmap.is_region_neighbor(0, Hex { x: 10, y: 3 }));
                // neighbor
                assert!(testmap.is_region_neighbor(0, Hex { x: 4, y: 2 }));
                // not neighbor
                assert!(!testmap.is_region_neighbor(0, Hex { x: 1, y: 1 }));
            }
        }

        mod add_city {
            use crate::game::map::Map;
            use hexx::Hex;

            #[test]
            fn tests() {
                let mut testmap = Map::new(10, 10);

                // in bounds
                let city1 = Hex::new(3, 4);
                assert!(testmap.add_city(city1));
                assert_eq!(testmap.regions.len(), 1);
                assert_eq!(testmap.regions.get(0).unwrap().len(), 1);
                assert_eq!(testmap.regions.get(0).unwrap().get(0).unwrap(), city1);

                // taken
                let city2 = Hex { x: 12, y: 14 };
                assert!(!testmap.add_city(city2));
                assert_eq!(testmap.regions.len(), 1);
                assert_eq!(testmap.regions.get(0).unwrap().len(), 1);
                assert_eq!(testmap.regions.get(0).unwrap().get(0).unwrap(), city1);
                // out of bounds
                testmap.vwrap = true;
                testmap.hwrap = true;
                assert!(testmap.add_city(city2));
                assert_eq!(testmap.regions.len(), 2);
                assert_eq!(testmap.regions.get(0).unwrap().len(), 1);
                assert_eq!(testmap.regions.get(0).unwrap().get(0).unwrap(), city1);
                assert_eq!(testmap.regions.get(1).unwrap().len(), 1);
                assert_eq!(testmap.regions.get(1).unwrap().get(0).unwrap(), Hex {x: 2, y: 4});
            }
        }
    
        mod add_to_region {
            use crate::game::map::Map;
            use hexx::Hex;

            #[test]
            fn tests() {
                let mut testmap = Map::new(5, 5);

                // set up a test region.
                testmap.regions.push(vec![Hex { x: 3, y: 3}]);
                testmap.regions[0].push(Hex { x: 2, y: 3 });
                testmap.regions[0].push(Hex { x: 4, y: 3 });

                testmap.tiles[3][3].region = Some(0);
                testmap.tiles[2][3].region = Some(0);
                testmap.tiles[4][3].region = Some(0);

                testmap.regions.push(vec![Hex::new(1, 3)]);
                testmap.tiles[1][3].region = Some(1);

                // within region
                assert!(!testmap.add_to_region(0, Hex {x: 3, y: 3}));
                // in another region
                assert!(!testmap.add_to_region(0, Hex {x: 1, y: 3}));
                // not neighbor
                assert!(!testmap.add_to_region(0, Hex {x: 0, y: 0}));
                // out of bounds
                assert!(!testmap.add_to_region(0, Hex {x: 10, y: 10}));
                // wrapped neighbor
                testmap.hwrap = true;
                assert!(testmap.add_to_region(0, Hex {x: 8, y: 4}));
                assert_eq!(testmap.regions[0].len(), 4);
                assert_eq!(testmap.regions[0][3], Hex::new(3, 4));
                assert_eq!(testmap.tiles[3][4].region.unwrap(), 0);
                // normal neighbor
                assert!(testmap.add_to_region(0, Hex {x: 3, y: 2}));
                assert_eq!(testmap.regions[0].len(), 5);
                assert_eq!(testmap.regions[0][4], Hex::new(3, 2));
                assert_eq!(testmap.tiles[3][2].region.unwrap(), 0);
            }
        }
    }

    mod processes {
        use crate::game::{factuals::Factuals, good::Good, process::{InputEffect, InputType, Process, ProcessEffect, ProcessInput, ProcessOutput}};
        use std::{collections::HashMap};
        // --- Minimal helpers to keep tests readable ---

        static REQ_GOOD: usize = 10;
        static OUT_GOOD: usize = 20;
        static OPTIN_GOOD: usize = 30;
        static OPTIN_EFFECT: InputEffect = InputEffect::Input(0.2);
        static OPTOUT_GOOD: usize = 31;
        static OPTOUT_EFFECT: InputEffect = InputEffect::Output(0.3);
        static OPTTHROUGH_GOOD: usize = 32;
        static OPTTHROUGH_EFFECT: InputEffect = InputEffect::Throughput(0.25);
        static FIXED_GOOD: usize = 40;
        static FACTOR_GOOD: usize = 99;
        static CONSUMED_GOOD: usize = 100;
        static CAPITAL_GOOD: usize = 50;
        static DECAY_OUTPUT: usize = 200;

        fn make_factuals(goods: Vec<Good>) -> Factuals {
            let mut map = HashMap::new();
            for g in goods {
                map.insert(g.id, g);
            }
            Factuals { goods: map, processes: HashMap::new() }
        }

        fn make_input(good: usize, amount: f64, fixed: bool, itype: InputType) -> ProcessInput {
            ProcessInput::new(good, amount, fixed, itype, false)
        }

        fn make_optional_input(
            good: usize,
            amount: f64,
            fixed: bool,
            itype: InputType,
            effects: Vec<InputEffect>,
        ) -> ProcessInput {
            let mut inp = ProcessInput::new(good, amount, fixed, itype, true);
            for e in effects {
                inp = inp.with_optional(e);
            }
            inp
        }

        fn make_process() -> Process {
            Process::new(0, "Test", 0)
                .with_input(make_input(REQ_GOOD, 1.0, false, InputType::Destroyed))
                .with_output(ProcessOutput::new(OUT_GOOD, 1.0, false))
        }

        mod check_factor {
            use super::*;

            #[test]
            fn test_without_factors() {
                let process = make_process();

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 100.0);
                available.insert(FACTOR_GOOD, 1.0);

                // Initial check, no factors, returns some with 1.0 in all parts.
                let result = process.check_factors(&available);
                if let Some((input, throughput, output, _)) = result {
                    assert_eq!(input, 1.0, "Input Incorrect.");
                    assert_eq!(throughput, 1.0, "Throughput Incorrect.");
                    assert_eq!(output, 1.0, "Output Incorrect.");
                } else {
                    assert!(false, "Did not return Correct value.");
                }
            }

            #[test]
            fn test_with_required_factors() {
                let process = make_process()
                    .with_input(make_input(FACTOR_GOOD, 1.0, false, InputType::Factor));

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 100.0);

                // Initial check, no factors, returns None
                let result = process.check_factors(&available);
                if let Some(_) = result {
                    assert!(false, "Returned Some when it shouldn't have.");
                } else {
                    assert!(true, "Did not return Correct value.");
                }

                // Include factors, expect output.
                available.insert(FACTOR_GOOD, 1.0);

                let result = process.check_factors(&available);
                if let Some((input, throughput, output, _)) = result {
                    assert_eq!(input, 1.0, "Input Incorrect.");
                    assert_eq!(throughput, 1.0, "Throughput Incorrect.");
                    assert_eq!(output, 1.0, "Output Incorrect.");
                } else {
                    assert!(false, "Did not return Correct value.");
                }
            }

            #[test]
            fn test_with_optional_factors() {
                let process = make_process()
                    .with_input(make_optional_input(OPTIN_GOOD, 1.0, false, 
                        InputType::Factor, vec![OPTIN_EFFECT.clone()]))
                    .with_input(make_optional_input(OPTTHROUGH_GOOD, 1.0, false, 
                        InputType::Factor, vec![OPTTHROUGH_EFFECT.clone()]))
                    .with_input(make_optional_input(OPTOUT_GOOD, 1.0, false, 
                        InputType::Factor, vec![OPTOUT_EFFECT.clone()]));

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 100.0);

                // Initial check, no factors, returns baseline.
                let result = process.check_factors(&available);
                if let Some((input, throughput, output, _)) = result {
                    assert_eq!(input, 1.0, "Input Incorrect.");
                    assert_eq!(throughput, 1.0, "Throughput Incorrect.");
                    assert_eq!(output, 1.0, "Output Incorrect.");
                } else {
                    assert!(false, "Did not return Correct value.");
                }

                // Include factors, expect output.
                available.insert(OPTIN_GOOD, 1.0);

                let result = process.check_factors(&available);
                if let Some((input, throughput, output, _)) = result {
                    assert_eq!(input, 0.8, "Input Incorrect.");
                    assert_eq!(throughput, 1.0, "Throughput Incorrect.");
                    assert_eq!(output, 1.0, "Output Incorrect.");
                } else {
                    assert!(false, "Did not return Correct value.");
                }

                available.insert(OPTTHROUGH_GOOD, 1.0);

                let result = process.check_factors(&available);
                if let Some((input, throughput, output, _)) = result {
                    assert_eq!(input, 0.8, "Input Incorrect.");
                    assert_eq!(throughput, 1.25, "Throughput Incorrect.");
                    assert_eq!(output, 1.0, "Output Incorrect.");
                } else {
                    assert!(false, "Did not return Correct value.");
                }
                
                available.insert(OPTOUT_GOOD, 1.0);

                let result = process.check_factors(&available);
                if let Some((input, throughput, output, _)) = result {
                    assert_eq!(input, 0.8, "Input Incorrect.");
                    assert_eq!(throughput, 1.25, "Throughput Incorrect.");
                    assert_eq!(output, 1.3, "Output Incorrect.");
                } else {
                    assert!(false, "Did not return Correct value.");
                }
            }
        }

        mod do_process_leg_should {
            use crate::test::make_good;

use super::*;

            #[test]
            fn return_empty_result_correctly() {
                let process = make_process();

                let available = HashMap::new();

                let factuals = make_factuals(vec![]);
                let result = process.do_process_leg(&available, None, (1.0, 1.0, 1.0), &factuals);

                assert_eq!(result.iterations, 0.0);
                assert_eq!(result.changes.len(), 0);
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);
            }

            #[test]
            fn return_complete_success_on_simple_process() {
                let process = make_process();

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 100.0);

                let factuals = make_factuals(vec![]);
                let result = process.do_process_leg(&available, None, (1.0, 1.0, 1.0), &factuals);

                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 2);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);
            }

            #[test]
            fn return_complete_success_with_capital() {
                let process = make_process()
                    .with_input(make_input(CAPITAL_GOOD, 1.0, true, InputType::Capital));

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 100.0);
                available.insert(CAPITAL_GOOD, 100.0);

                let factuals = make_factuals(vec![]);
                let result = process.do_process_leg(&available, None, (1.0, 1.0, 1.0), &factuals);

                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 2);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
                assert_eq!(result.used_inputs.len(), 1);
                assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&100.0));
                assert_eq!(result.effects.len(), 0);
            }

            #[test]
            fn return_complete_success_with_consumed_input() {
                let process = make_process()
                    .with_input(make_input(CONSUMED_GOOD, 1.0, false, InputType::Consumed));

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 100.0);
                available.insert(CONSUMED_GOOD, 100.0);

                let factuals = make_factuals(vec![make_good(CONSUMED_GOOD, 
                    "Consumed", vec![(DECAY_OUTPUT, 1.0)].into_iter().collect())]);
                let result = process.do_process_leg(&available, None, 
                    (1.0, 1.0, 1.0), &factuals);

                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 4);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
                assert_eq!(result.changes.get(&CONSUMED_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);
            }

            #[test]
            fn return_partial_success() {
                let process = make_process()
                    .with_input(make_input(CAPITAL_GOOD, 1.0, false, InputType::Destroyed));

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 50.0);
                available.insert(CAPITAL_GOOD, 100.0);

                let factuals = make_factuals(vec![]);
                let result = process.do_process_leg(&available, None, (1.0, 1.0, 1.0), &factuals);

                assert_eq!(result.iterations, 50.0);
                assert_eq!(result.changes.len(), 3);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
                assert_eq!(result.changes.get(&CAPITAL_GOOD), Some(&-50.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);
            }

            #[test]
            fn return_success_with_optional_input_but_none_given() {
                let process = make_process()
                    .with_input(make_optional_input(OPTIN_GOOD, 1.0, false, 
                        InputType::Destroyed, vec![OPTIN_EFFECT.clone()]));

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 100.0);

                let factuals = make_factuals(vec![]);
                let result = process.do_process_leg(&available, None, (1.0, 1.0, 1.0), &factuals);

                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 2);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);
            }

            #[test]
            fn correctly_include_factor_bonuses_and_fixed_input() {
                let process = make_process()
                    .with_input(make_input(FIXED_GOOD, 1.0, true, InputType::Destroyed))
                    .with_output(ProcessOutput::new(FACTOR_GOOD, 1.0, true));

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 100.0);
                available.insert(FIXED_GOOD, 100.0);

                let factuals = make_factuals(vec![]);

                // Input Bonus
                let result = process.do_process_leg(&available, None, 
                    (0.5, 1.0, 1.0), &factuals);

                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 4);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
                assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&100.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);

                // Throughput Bonus
                let result = process.do_process_leg(&available, None, 
                    (1.0, 2.0, 1.0), &factuals);

                assert_eq!(result.iterations, 50.0);
                assert_eq!(result.changes.len(), 4);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-50.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
                assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&50.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);

                // Output Bonus
                let result = process.do_process_leg(&available, None, 
                    (1.0, 1.0, 2.0), &factuals);

                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 4);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&200.0));
                assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&100.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);

                // Overlapping Bonii
                let result = process.do_process_leg(&available, None, 
                    (0.5, 4.0, 2.0), &factuals);

                assert_eq!(result.iterations, 50.0);
                assert_eq!(result.changes.len(), 4);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-50.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&400.0));
                assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&50.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);
            }

            #[test]
            fn correctly_use_optional_goods_and_bonuses() {
                let og_process = make_process()
                    .with_input(make_input(FIXED_GOOD, 1.0, true, InputType::Destroyed))
                    .with_output(ProcessOutput::new(FACTOR_GOOD, 1.0, true));

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 100.0);
                available.insert(FIXED_GOOD, 100.0);
                available.insert(OPTIN_GOOD, 100.0);
                available.insert(OPTTHROUGH_GOOD, 100.0);
                available.insert(OPTOUT_GOOD, 100.0);

                let factuals = make_factuals(vec![]);

                // Input Bonus
                let process = og_process.clone()
                    .with_input(make_optional_input(OPTIN_GOOD, 1.0, true, 
                        InputType::Destroyed, vec![OPTIN_EFFECT.clone()]));
                let result = process.do_process_leg(&available, None, 
                    (1.0, 1.0, 1.0), &factuals);

                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 5);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-80.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
                assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&100.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);

                // Throughput Bonus
                let process = og_process.clone()
                    .with_input(make_optional_input(OPTTHROUGH_GOOD, 1.0, true, 
                        InputType::Destroyed, vec![OPTTHROUGH_EFFECT.clone()]));
                let result = process.do_process_leg(&available, None, 
                    (1.0, 1.0, 1.0), &factuals);

                assert_eq!(result.iterations, 80.0);
                assert_eq!(result.changes.len(), 5);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-80.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
                assert_eq!(result.changes.get(&OPTTHROUGH_GOOD), Some(&-80.0));
                assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&80.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);

                // Output Bonus
                let process = og_process.clone()
                    .with_input(make_optional_input(OPTOUT_GOOD, 1.0, true, 
                        InputType::Destroyed, vec![OPTOUT_EFFECT.clone()]));
                let result = process.do_process_leg(&available, None, 
                    (1.0, 1.0, 1.0), &factuals);

                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 5);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&130.0));
                assert_eq!(result.changes.get(&OPTOUT_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&100.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);

                // overlapping Bonii
                let process = og_process.clone()
                    .with_input(make_optional_input(OPTIN_GOOD, 1.0, true, 
                        InputType::Destroyed, vec![OPTIN_EFFECT.clone()]))
                    .with_input(make_optional_input(OPTTHROUGH_GOOD, 1.0, true, 
                        InputType::Destroyed, vec![OPTTHROUGH_EFFECT.clone()]))
                    .with_input(make_optional_input(OPTOUT_GOOD, 1.0, true, 
                        InputType::Destroyed, vec![OPTOUT_EFFECT.clone()]));
                let result = process.do_process_leg(&available, None, 
                    (1.0, 1.0, 1.0), &factuals);
                
                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 7);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OPTTHROUGH_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OPTOUT_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&162.5));
                assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&100.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);
            }
        
            #[test]
            fn process_other_effects_correctly() {
                let process = make_process()
                    .with_input(make_optional_input(CONSUMED_GOOD, 1.0, false, 
                        InputType::Destroyed, vec![InputEffect::ExtraOutput(DECAY_OUTPUT, 1.0)]))
                    .with_input(make_optional_input(FACTOR_GOOD, 1.0, false, 
                        InputType::Destroyed, vec![InputEffect::Growth(0.5)]))
                    .with_effect(ProcessEffect::Research(100.0));

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 100.0);
                available.insert(CONSUMED_GOOD, 100.0);
                available.insert(FACTOR_GOOD, 100.0);

                let factuals = make_factuals(vec![]);
                let result = process.do_process_leg(&available, None, 
                    (1.0, 1.0, 1.0), &factuals);

                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 5);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&CONSUMED_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
                assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 2);
                assert!(result.effects.contains(&ProcessEffect::Research(10000.0)));
                assert!(result.effects.contains(&ProcessEffect::Growth(50.0)));
            }

            #[test]
            fn multistep_process_test() {
                let process = make_process()
                    .with_input(make_input(CAPITAL_GOOD, 1.0, true, InputType::Capital))
                    .with_input(make_input(FIXED_GOOD, 1.0, true, InputType::Destroyed))
                    .with_input(make_optional_input(OPTIN_GOOD, 1.0, true, 
                        InputType::Destroyed, vec![OPTIN_EFFECT.clone()]))
                    .with_input(make_optional_input(OPTTHROUGH_GOOD, 1.0, true, 
                        InputType::Destroyed, vec![OPTTHROUGH_EFFECT.clone()]))
                    .with_input(make_optional_input(OPTOUT_GOOD, 1.0, true, 
                        InputType::Consumed, vec![OPTOUT_EFFECT.clone()]))
                    .with_output(ProcessOutput::new(FACTOR_GOOD, 1.0, true));

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 150.0);
                available.insert(CAPITAL_GOOD, 140.0);
                available.insert(FIXED_GOOD, 190.0);
                available.insert(OPTIN_GOOD, 30.0);
                available.insert(OPTTHROUGH_GOOD, 50.0);
                available.insert(OPTOUT_GOOD, 80.0);

                let factuals = make_factuals(vec![make_good(OPTOUT_GOOD, "Consumed", 
                    vec![(DECAY_OUTPUT, 1.0)].into_iter().collect())]);

                // First Pass
                let result = process.do_process_leg(&available, None, 
                    (1.0, 1.0, 1.0), &factuals);
                
                assert_eq!(result.iterations, 30.0);
                assert_eq!(result.changes.len(), 8);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-30.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-30.0));
                assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-30.0));
                assert_eq!(result.changes.get(&OPTTHROUGH_GOOD), Some(&-30.0));
                assert_eq!(result.changes.get(&OPTOUT_GOOD), Some(&-30.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&48.75));
                assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&30.0));
                assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&30.0));
                assert_eq!(result.used_inputs.len(), 1);
                assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&30.0));
                assert_eq!(result.effects.len(), 0);

                // second pass, remove stuff
                for (good, change) in &result.changes {
                    *available.entry(*good).or_insert(0.0) += *change;
                }
                if let Some(capital_used) = result.used_inputs.get(&CAPITAL_GOOD) {
                    *available.entry(CAPITAL_GOOD).or_insert(0.0) -= *capital_used;
                }

                let result = process.do_process_leg(&available, None, 
                    (1.0, 1.0, 1.0), &factuals);
                
                assert_eq!(result.iterations, 20.0);
                assert_eq!(result.changes.len(), 7);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-25.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-20.0));
                assert_eq!(result.changes.get(&OPTTHROUGH_GOOD), Some(&-20.0));
                assert_eq!(result.changes.get(&OPTOUT_GOOD), Some(&-20.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&32.5));
                assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&20.0));
                assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&20.0));
                assert_eq!(result.used_inputs.len(), 1);
                assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&20.0));
                assert_eq!(result.effects.len(), 0);

                // THird Pass, output bonus good next
                for (good, change) in &result.changes {
                    *available.entry(*good).or_insert(0.0) += *change;
                }
                if let Some(capital_used) = result.used_inputs.get(&CAPITAL_GOOD) {
                    *available.entry(CAPITAL_GOOD).or_insert(0.0) -= *capital_used;
                }

                let result = process.do_process_leg(&available, None, 
                    (1.0, 1.0, 1.0), &factuals);
                
                assert_eq!(result.iterations, 30.0);
                assert_eq!(result.changes.len(), 6);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-30.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-30.0));
                assert_eq!(result.changes.get(&OPTOUT_GOOD), Some(&-30.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&39.0));
                assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&30.0));
                assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&30.0));
                assert_eq!(result.used_inputs.len(), 1);
                assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&30.0));
                assert_eq!(result.effects.len(), 0);

                // Fourth Pass, no bonuses left, one last one.
                for (good, change) in &result.changes {
                    *available.entry(*good).or_insert(0.0) += *change;
                }
                if let Some(capital_used) = result.used_inputs.get(&CAPITAL_GOOD) {
                    *available.entry(CAPITAL_GOOD).or_insert(0.0) -= *capital_used;
                }

                let result = process.do_process_leg(&available, None, 
                    (1.0, 1.0, 1.0), &factuals);
                
                assert_eq!(result.iterations, 60.0);
                assert_eq!(result.changes.len(), 4);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-60.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-60.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&60.0));
                assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&60.0));
                assert_eq!(result.used_inputs.len(), 1);
                assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&60.0));
                assert_eq!(result.effects.len(), 0);

                // Last Pass, should return 0 iterations.
                for (good, change) in &result.changes {
                    *available.entry(*good).or_insert(0.0) += *change;
                }
                if let Some(capital_used) = result.used_inputs.get(&CAPITAL_GOOD) {
                    *available.entry(CAPITAL_GOOD).or_insert(0.0) -= *capital_used;
                }

                let result = process.do_process_leg(&available, None, 
                    (1.0, 1.0, 1.0), &factuals);

                assert_eq!(result.iterations, 0.0);
                assert_eq!(result.changes.len(), 0);
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);

                // Lastly, check that our available goods have been correctly updated for sanity reasons.

                assert_eq!(available.get(&REQ_GOOD), Some(&5.0));
                assert_eq!(available.get(&FIXED_GOOD), Some(&50.0));
                assert_eq!(available.get(&OPTIN_GOOD), Some(&0.0));
                assert_eq!(available.get(&OPTTHROUGH_GOOD), Some(&0.0));
                assert_eq!(available.get(&OPTOUT_GOOD), Some(&0.0));
                assert_eq!(available.get(&OUT_GOOD), Some(&180.25));
                assert_eq!(available.get(&FACTOR_GOOD), Some(&140.0));
                assert_eq!(available.get(&DECAY_OUTPUT), Some(&80.0));
            }
        }

        mod do_process_should {
            use crate::test::make_good;

use super::*;

            #[test]
            fn basic_process_and_target_plus_capital_and_fixed_good_check() {
                let process = make_process();

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 100.0);

                let factuals = make_factuals(vec![]);
                let result = process.do_process(&available, None, &factuals);

                // Check Results
                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 2); // only req and out should be changed
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
                assert!(result.used_inputs.is_empty()); // confirm no capital used
                assert!(result.effects.is_empty()); // confirm no stray effects.
                assert_eq!(result.missing_goods.len(), 1);
                assert!(result.missing_goods.contains(&REQ_GOOD));

                let result = process.do_process(&available, Some(50.0), &factuals);

                // Check Results
                assert_eq!(result.iterations, 50.0);
                assert_eq!(result.changes.len(), 2); // only req and out should be changed
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
                assert!(result.used_inputs.is_empty()); // confirm no capital used
                assert!(result.effects.is_empty()); // confirm no stray effects.
                assert_eq!(result.missing_goods.len(), 0);

                // Repeat, but with Capital good.
                let process = process
                    .with_input(make_input(CAPITAL_GOOD, 1.0, true, InputType::Capital));

                available.insert(CAPITAL_GOOD, 100.0);

                let result = process.do_process(&available, None, &factuals);

                // Check Results
                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 2); // only req and out should be changed
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
                assert_eq!(result.used_inputs.len(), 1); // confirm capital used
                assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&100.0)); // confirm capital used
                assert!(result.effects.is_empty()); // confirm no stray effects.
                assert_eq!(result.missing_goods.len(), 2);
                assert!(result.missing_goods.contains(&REQ_GOOD));
                assert!(result.missing_goods.contains(&CAPITAL_GOOD));

                let result = process.do_process(&available, Some(50.0), &factuals);

                // Check Results
                assert_eq!(result.iterations, 50.0);
                assert_eq!(result.changes.len(), 2); // only req and out should be changed
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
                assert_eq!(result.used_inputs.len(), 1); // confirm capital used
                assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&50.0)); // confirm capital used
                assert!(result.effects.is_empty()); // confirm no stray effects.
                assert_eq!(result.missing_goods.len(), 0);

                // === Include Fixed Good
                let process = process
                    .with_input(make_input(FIXED_GOOD, 1.0, true, InputType::Destroyed));

                available.insert(FIXED_GOOD, 100.0);

                let result = process.do_process(&available, None, &factuals);

                // Check Results
                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 3); // only req and out should be changed
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
                assert_eq!(result.used_inputs.len(), 1); // confirm capital used
                assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&100.0)); // confirm capital used
                assert!(result.effects.is_empty()); // confirm no stray effects.
                assert_eq!(result.missing_goods.len(), 3);
                assert!(result.missing_goods.contains(&REQ_GOOD));
                assert!(result.missing_goods.contains(&FIXED_GOOD));
                assert!(result.missing_goods.contains(&CAPITAL_GOOD));

                let result = process.do_process(&available, Some(50.0), &factuals);

                // Check Results
                assert_eq!(result.iterations, 50.0);
                assert_eq!(result.changes.len(), 3); // only req and out should be changed
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-50.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
                assert_eq!(result.used_inputs.len(), 1); // confirm capital used
                assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&50.0)); // confirm capital used
                assert!(result.effects.is_empty()); // confirm no stray effects.
                assert_eq!(result.missing_goods.len(), 0);
            }

            #[test]
            fn process_with_factor() {
                // === Incluide Factor (and check it's exclusion cause failure)
                let process = make_process()
                    .with_input(make_input(FACTOR_GOOD, 1.0, true, InputType::Factor));

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 100.0);

                let factuals = make_factuals(vec![]);

                let result = process.do_process(&available, None, &factuals);

                // Check Results
                assert_eq!(result.iterations, 0.0);
                assert_eq!(result.changes.len(), 0); // only req and out should be changed
                assert_eq!(result.used_inputs.len(), 0); // confirm capital used
                assert!(result.effects.is_empty()); // confirm no stray effects.
                assert_eq!(result.missing_goods.len(), 1);
                assert!(result.missing_goods.contains(&FACTOR_GOOD), "Factor input should be missing");

                // Actually add the factor in.
                available.insert(FACTOR_GOOD, 1.0);

                let result = process.do_process(&available, None, &factuals);

                // Check Results
                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 2); // only req and out should be changed
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
                assert_eq!(result.used_inputs.len(), 0); // confirm capital used
                assert!(result.effects.is_empty()); // confirm no stray effects.
                assert_eq!(result.missing_goods.len(), 1);
                assert!(result.missing_goods.contains(&REQ_GOOD), "Required Good should be missing");

                let result = process.do_process(&available, Some(50.0), &factuals);

                // Check Results
                assert_eq!(result.iterations, 50.0);
                assert_eq!(result.changes.len(), 2); // only req and out should be changed
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
                assert_eq!(result.used_inputs.len(), 0); // confirm capital used
                assert!(result.effects.is_empty()); // confirm no stray effects.
                assert_eq!(result.missing_goods.len(), 0);
            }

            #[test]
            fn process_with_consumed_good() {
                let process = make_process()
                    .with_input(make_input(CONSUMED_GOOD, 1.0, true, InputType::Consumed));

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 100.0);
                available.insert(CONSUMED_GOOD, 100.0);

                let factuals = make_factuals(vec![
                    make_good(CONSUMED_GOOD, "Consumed", HashMap::from([(DECAY_OUTPUT, 1.0)])),
                ]);
                let result = process.do_process(&available, None, &factuals);

                // Check Results
                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 4); // only req and out should be changed
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&CONSUMED_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
                assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
                assert_eq!(result.used_inputs.len(), 0); // confirm capital used
                assert!(result.effects.is_empty()); // confirm no stray effects.

                let result = process.do_process(&available, Some(50.0), &factuals);

                // Check Results
                assert_eq!(result.iterations, 50.0);
                assert_eq!(result.changes.len(), 4); // only req and out should be changed
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
                assert_eq!(result.changes.get(&CONSUMED_GOOD), Some(&-50.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
                assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&50.0));
                assert_eq!(result.used_inputs.len(), 0); // confirm capital used
                assert!(result.effects.is_empty()); // confirm no stray effects.
            }

            #[test]
            fn process_with_optional_with_no_optionals_given() {
                let process = make_process()
                    .with_input(make_optional_input(OPTIN_GOOD, 1.0, true, 
                        InputType::Destroyed, vec![OPTIN_EFFECT.clone()]));

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 100.0);
                    
                let factuals = make_factuals(vec![]);
                let result = process.do_process(&available, None, &factuals);

                // Check Results
                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 2); // only req and out should be changed
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
                assert_eq!(result.used_inputs.len(), 0); // confirm capital used
                assert!(result.effects.is_empty()); // confirm no stray effects.
                assert_eq!(result.missing_goods.len(), 1);
                assert!(result.missing_goods.contains(&REQ_GOOD), "Missing required good.");

                let result = process.do_process(&available, Some(50.0), &factuals);

                // Check Results
                assert_eq!(result.iterations, 50.0);
                assert_eq!(result.changes.len(), 2); // only req and out should be changed
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
                assert_eq!(result.used_inputs.len(), 0); // confirm capital used
                assert!(result.effects.is_empty()); // confirm no stray effects.
                assert_eq!(result.missing_goods.len(), 0);
            }

            #[test]
            fn insufficient_inputs_reduce_output() {
                let process = make_process()
                    .with_input(make_input(CAPITAL_GOOD, 1.0, true, InputType::Capital))
                    .with_input(make_input(FIXED_GOOD, 1.0, true, InputType::Destroyed));
                let mut available = HashMap::new();

                available.insert(REQ_GOOD, 100.0);
                available.insert(CAPITAL_GOOD, 30.0);
                available.insert(FIXED_GOOD, 130.0);

                let factuals = make_factuals(vec![]);
                let result = process.do_process(&available, None, &factuals);

                // Check Results
                assert_eq!(result.iterations, 30.0);
                assert_eq!(result.changes.len(), 3); // only req and out should be changed
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-30.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-30.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&30.0));
                assert_eq!(result.used_inputs.len(), 1); // confirm capital used
                assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&30.0)); // confirm capital used
                assert!(result.effects.is_empty()); // confirm no stray effects.
                assert_eq!(result.missing_goods.len(), 1);
                assert!(result.missing_goods.contains(&CAPITAL_GOOD), "Missing required good.");

                let result = process.do_process(&available, Some(50.0), &factuals);

                // Check Results
                assert_eq!(result.iterations, 30.0);
                assert_eq!(result.changes.len(), 3); // only req and out should be changed
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-30.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-30.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&30.0));
                assert_eq!(result.used_inputs.len(), 1); // confirm capital used
                assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&30.0)); // confirm capital used
                assert!(result.effects.is_empty()); // confirm no stray effects.
                assert_eq!(result.missing_goods.len(), 1);
                assert!(result.missing_goods.contains(&CAPITAL_GOOD), "Missing required good.");
            }

            #[test]
            fn optional_input_gives_proportional_bonus() {
                let process = make_process()
                    .with_input(make_optional_input(OPTIN_GOOD, 1.0, true, 
                        InputType::Destroyed, vec![OPTIN_EFFECT.clone()]));
                let mut available = HashMap::new();

                available.insert(REQ_GOOD, 100.0);
                available.insert(OPTIN_GOOD, 50.0); // only half the optional provided

                let factuals = make_factuals(vec![]);
                let result = process.do_process(&available, None, &factuals);

                // Check Results
                assert_eq!(result.iterations, 110.0); // should be a 10% boost from the optional
                assert_eq!(result.changes.len(), 3); // only req and out should be changed
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-50.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&110.0));
                assert_eq!(result.used_inputs.len(), 0); // confirm capital used
                assert!(result.effects.is_empty()); // confirm no stray effects.
                assert_eq!(result.missing_goods.len(), 1);
                assert!(result.missing_goods.contains(&REQ_GOOD), "Missing required good.");

                let result = process.do_process(&available, Some(50.0), &factuals);

                // Check Results
                assert_eq!(result.iterations, 50.0); // should be a 10% boost from the optional
                assert_eq!(result.changes.len(), 3); // only req and out should be changed
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-40.0));
                assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-50.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
                assert_eq!(result.used_inputs.len(), 0); // confirm capital used
                assert!(result.effects.is_empty()); // confirm no stray effects.
                assert_eq!(result.missing_goods.len(), 0);
            }

            #[test]
            fn efficiency_modifiers_alone_and_stacked() {
                // extra goods for testing purposes
                let throughput_factor = 33;
                let output_factor = 34;
                let factor_throughput_bonus = 0.25;
                let factor_output_bonus = 0.2;
                // Base process (no bonuses)
                let base = make_process()
                    .with_input(make_input(FIXED_GOOD, 1.0, true, InputType::Destroyed))
                    .with_input(make_optional_input(OPTIN_GOOD, 1.0, false, InputType::Destroyed, vec![OPTIN_EFFECT.clone()]))
                    .with_input(make_optional_input(OPTOUT_GOOD, 1.0, false, InputType::Destroyed, vec![OPTOUT_EFFECT.clone()]))
                    .with_input(make_optional_input(OPTTHROUGH_GOOD, 1.0, false, InputType::Destroyed, vec![OPTTHROUGH_EFFECT.clone()]))

                    .with_input(make_optional_input(FACTOR_GOOD, 1.0, false, InputType::Factor, vec![InputEffect::Input(0.2)]))
                    .with_input(make_optional_input(throughput_factor, 1.0, false, InputType::Factor, vec![InputEffect::Throughput(factor_throughput_bonus)]))
                    .with_input(make_optional_input(output_factor, 1.0, false, InputType::Factor, vec![InputEffect::Output(factor_output_bonus)]))

                    .with_output(ProcessOutput::new(DECAY_OUTPUT, 1.0, true));

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 100.0);
                available.insert(FIXED_GOOD, 100.0);

                let factuals = make_factuals(vec![]);
                
                // baseline, no bonuses
                let result = base.do_process(&available, None, &factuals);

                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 4);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);
                assert_eq!(result.missing_goods.len(), 2);
                assert!(result.missing_goods.contains(&REQ_GOOD));
                assert!(result.missing_goods.contains(&FIXED_GOOD));

                // input bonus factor
                available.insert(FACTOR_GOOD, 1.0);
                let result = base.do_process(&available, None, &factuals);

                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-80.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);
                assert_eq!(result.missing_goods.len(), 1);
                assert!(result.missing_goods.contains(&FIXED_GOOD));

                // plus throughput factor
                available.insert(throughput_factor, 1.0);
                let result = base.do_process(&available, None, &factuals);

                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 4);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&125.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);
                assert_eq!(result.missing_goods.len(), 2);
                assert!(result.missing_goods.contains(&REQ_GOOD));
                assert!(result.missing_goods.contains(&FIXED_GOOD));

                // plus output factor
                available.insert(output_factor, 1.0);
                let result = base.do_process(&available, None, &factuals);

                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 4);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&150.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);
                assert_eq!(result.missing_goods.len(), 2);
                assert!(result.missing_goods.contains(&REQ_GOOD));
                assert!(result.missing_goods.contains(&FIXED_GOOD));

                // plus input bonus optional
                available.insert(OPTIN_GOOD, 100.0);
                let result = base.do_process(&available, None, &factuals);

                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 5);
                assert!((result.changes.get(&REQ_GOOD).unwrap() + 75.0).abs() < 0.01);
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&150.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);
                assert_eq!(result.missing_goods.len(), 1);
                assert!(result.missing_goods.contains(&FIXED_GOOD));

                // plus throughput optional
                available.insert(OPTTHROUGH_GOOD, 100.0);
                let result = base.do_process(&available, None, &factuals);

                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 6);
                assert!((result.changes.get(&REQ_GOOD).unwrap() + 90.0).abs() < 0.01);
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OPTTHROUGH_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
                assert!((result.changes.get(&OUT_GOOD).unwrap() -180.0).abs() < 0.01);
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);
                assert_eq!(result.missing_goods.len(), 1);
                assert!(result.missing_goods.contains(&FIXED_GOOD));

                // plus output optional
                available.insert(OPTOUT_GOOD, 100.0);
                let result = base.do_process(&available, None, &factuals);

                assert_eq!(result.iterations, 100.0);
                assert_eq!(result.changes.len(), 7);
                assert!((result.changes.get(&REQ_GOOD).unwrap() + 90.0).abs() < 0.01);
                assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OPTTHROUGH_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OPTOUT_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&225.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);
                assert_eq!(result.missing_goods.len(), 1);
                assert!(result.missing_goods.contains(&FIXED_GOOD));
            }
        }
    }

    mod firm {
        use super::*;
        use crate::game::factuals::Factuals;
        use crate::game::good::Good; // if you need Good defs
        use crate::game::market::{Market, MarketGood};
        use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput, ProcessEffect};
        use std::collections::HashMap;
        use crate::game::firm::{Firm, FirmPRow, ProductionLine, ProductionReport};

        // Helper to build a minimal Factuals with one process
        fn make_factuals_with_process(process: Process) -> Factuals {
            let mut processes = HashMap::new();
            processes.insert(process.id, process);
            let mut goods = HashMap::new();
            Factuals {
                goods, // goods not strictly needed for do_process in these tests
                processes,
            }
        }

        // Helper to build a Market with AMV data for the goods we care about
        fn make_market_with_amvs(amvs: &[(usize, f64)]) -> Market {
            let mut goods = HashMap::new();
            for &(id, amv) in amvs {
                goods.insert(id, MarketGood {
                    amv,
                    production: 0.0,
                    consumption: 0.0,
                    imported: 0.0,
                    stock: 0.0,
                });
            }
            Market {
                id: 42,
                pops: HashMap::new(),
                goods,
            }
        }

        mod run_production_should {
            use super::*;

            #[test]
            fn test_basic_production_run() {
                // Simple process: 2 wood -> 1 plank (Consumed input, fixed output)
                let process = Process::new(1, "sawmill", 0)
                    .with_input(ProcessInput::new(10, 2.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(20, 1.0, true));

                let mut factuals = make_factuals_with_process(process);
                factuals.goods.insert(10, make_good(10, "wood", HashMap::new()));
                factuals.goods.insert(20, make_good(20, "plank", HashMap::new()));

                let mut firm = Firm::new(1, "Test Sawmill".into(), 42, hexx::Hex::new(0, 0));
                firm.property.insert(10, FirmPRow {
                    quantity: 10.0,
                    rolling_average: 0.0,
                    target: 0.0,
                    reserve: 0.0,
                    average_cost: 0.0,
                    used_capital: 0.0,
                });

                // Add a production line
                firm.production_line.push(ProductionLine {
                    process: 1,
                    target: None,
                    inputs: vec![10],
                    historical_productivity: 0.0,
                    last_success_rate: 0.0,
                    last_iterations: 0.0,
                    last_effects: vec![],
                    last_missing_goods: vec![],
                    last_amv_consumed: 0.0,
                    last_amv_produced: 0.0,
                });

                let market = make_market_with_amvs(&[(10, 5.0), (20, 12.0)]);

                let report = firm.run_production(&factuals, &market);

                // Property should be updated
                assert_eq!(firm.property[&10].quantity, 0.0);
                assert_eq!(firm.property[&20].quantity, 5.0); // 5 iterations * 1.0

                // Report should show what was produced and consumed
                assert_eq!(report.produced.get(&20), Some(&5.0));
                assert_eq!(report.consumed.get(&10), Some(&10.0));
                assert!(report.effects.is_empty());

                // Line should have recorded success + AMV snapshots
                let line = &firm.production_line[0];
                assert_eq!(line.last_success_rate, 1.0);
                assert_eq!(line.last_iterations, 5.0);
                assert_eq!(line.last_amv_consumed, 50.0);
                assert_eq!(line.last_amv_produced, 60.0);
            }

            #[test]
            fn test_partial_run_with_target_and_missing_goods() {
                let process = Process::new(2, "limited_craft", 0)
                    .with_input(ProcessInput::new(30, 3.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(40, 1.0, true));

                let mut factuals = make_factuals_with_process(process);
                factuals.goods.insert(30, make_good(30, "wood", HashMap::new()));
                factuals.goods.insert(40, make_good(40, "plank", HashMap::new()));

                let mut firm = Firm::new(2, "Limited Workshop".into(), 42, hexx::Hex::new(0, 0));
                firm.property.insert(30, FirmPRow {
                    quantity: 6.0, // only enough for 2 iterations (need 3 per iter)
                    ..Default::default() // we'll add used_capital etc. via insert if needed
                });

                firm.production_line.push(ProductionLine {
                    process: 2,
                    target: Some(10.0), // wants 10, will only get ~2
                    inputs: vec![30],
                    historical_productivity: 0.0,
                    last_success_rate: 0.0,
                    last_iterations: 0.0,
                    last_effects: vec![],
                    last_missing_goods: vec![],
                    last_amv_consumed: 0.0,
                    last_amv_produced: 0.0,
                });

                let market = make_market_with_amvs(&[(30, 2.0), (40, 8.0)]);

                let report = firm.run_production(&factuals, &market);

                // check property changes
                assert_eq!(firm.property[&30].quantity, 0.0);
                assert_eq!(firm.property[&40].quantity, 2.0);

                let line = &firm.production_line[0];
                //assert!((line.last_success_rate - 0.233333).abs() < 0.01);
                assert_eq!(line.last_success_rate, 0.2);
                assert_eq!(line.last_iterations, 2.0);
                assert_eq!(line.last_missing_goods, vec![30]);
                assert_eq!(line.last_amv_consumed, 12.0);
                assert_eq!(line.last_amv_produced, 16.0);

                assert_eq!(report.consumed.get(&30), Some(&6.0));
                assert_eq!(report.produced.get(&40), Some(&2.0));
            }

            #[test]
            fn test_capital_goods_not_counted_as_consumed() {
                // Process that uses a Capital good (e.g. saw blade) + consumes wood
                let process = Process::new(3, "capital_test", 0)
                    .with_input(ProcessInput::new(50, 1.0, true, InputType::Capital, false)) // saw
                    .with_input(ProcessInput::new(10, 2.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(20, 1.0, true));

                let mut factuals = make_factuals_with_process(process);
                factuals.goods.insert(10, make_good(10, "wood", HashMap::new()));
                factuals.goods.insert(20, make_good(20, "wood", HashMap::new()));
                factuals.goods.insert(50, make_good(50, "plank", HashMap::new()));

                let mut firm = Firm::new(3, "Capital Test Firm".into(), 42, hexx::Hex::new(0, 0));
                firm.property.insert(10, FirmPRow { quantity: 10.0, ..Default::default() });
                firm.property.insert(50, FirmPRow { quantity: 1.0, ..Default::default() });

                firm.production_line.push(ProductionLine {
                    process: 3,
                    target: None,
                    inputs: vec![50, 10],
                    historical_productivity: 0.0,
                    last_success_rate: 0.0,
                    last_iterations: 0.0,
                    last_effects: vec![],
                    last_missing_goods: vec![],
                    last_amv_consumed: 0.0,
                    last_amv_produced: 0.0,
                });

                let market = make_market_with_amvs(&[(10, 5.0), (20, 12.0), (50, 100.0)]);

                let report = firm.run_production(&factuals, &market);

                // Capital good should be recorded in used_capital, not in report.consumed
                assert_eq!(firm.property[&50].used_capital, 1.0);
                assert_eq!(firm.property[&50].quantity, 0.0);
                assert_eq!(firm.property[&10].quantity, 8.0);

                assert!(report.consumed.get(&50).is_none()); // capital should NOT appear in consumed
                assert_eq!(report.consumed.get(&10), Some(&2.0));
                assert_eq!(report.produced.get(&20), Some(&1.0));
            }

            #[test]
            fn test_effects_and_new_output_good() {
                let process = Process::new(4, "researchy", 0)
                    .with_input(ProcessInput::new(10, 1.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(99, 2.0, true))
                    .with_effect(ProcessEffect::Research(10.0));

                let mut factuals = make_factuals_with_process(process);
                factuals.goods.insert(10, make_good(10, "wood", HashMap::new()));
                factuals.goods.insert(20, make_good(99, "plank", HashMap::new()));

                let mut firm = Firm::new(4, "Research Lab".into(), 42, hexx::Hex::new(0, 0));
                firm.property.insert(10, FirmPRow { quantity: 5.0, ..Default::default() });

                firm.production_line.push(ProductionLine {
                    process: 4,
                    target: None,
                    inputs: vec![10],
                    historical_productivity: 0.0,
                    last_success_rate: 0.0,
                    last_iterations: 0.0,
                    last_effects: vec![],
                    last_missing_goods: vec![],
                    last_amv_consumed: 0.0,
                    last_amv_produced: 0.0,
                });

                let market = make_market_with_amvs(&[(10, 3.0), (99, 50.0)]);

                let report = firm.run_production(&factuals, &market);

                assert_eq!(report.effects.len(), 1);
                assert!(matches!(report.effects[0], ProcessEffect::Research(50.0)));

                // New good 99 should have been created in property
                assert!(firm.property.contains_key(&99));
                assert_eq!(firm.property[&99].quantity, 10.0);
            }

            #[test]
            #[should_panic(expected = "Process not found!")]
            fn test_unknown_process_panics() {
                let factuals = Factuals {
                    goods: HashMap::new(),
                    processes: HashMap::new(),
                };

                let mut firm = Firm::new(5, "Broken Firm".into(), 42, hexx::Hex::new(0, 0));
                firm.production_line.push(ProductionLine {
                    process: 999, // does not exist
                    target: Some(5.0),
                    inputs: vec![],
                    historical_productivity: 0.0,
                    last_success_rate: 0.42,
                    last_iterations: 3.0,
                    last_effects: vec![ProcessEffect::Culture(1.0)],
                    last_missing_goods: vec![1],
                    last_amv_consumed: 10.0,
                    last_amv_produced: 0.0,
                });

                let market = make_market_with_amvs(&[]);

                firm.run_production(&factuals, &market);
            }
        }
    }
}