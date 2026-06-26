pub mod game;
pub mod playstate;

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
            categories: vec![],
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

    mod pop {
        use std::collections::HashMap;

        use crate::game::{desire::{
            Desire, DesireSource, DesireTarget, DesireTargetType
        }, household::HouseholdDef, pop::{DemoRow, Pop, PopPRow}, scalingfactor::ScalingFactor};

        static CONSUMED_GOOD: usize = 100;
        static USED_GOOD: usize = 101;
        static DECAY_GOOD: usize = 200;

        fn make_pop() -> Pop {
            Pop {
                id: 0,
                job: 0,
                property: HashMap::new(),
                desires: vec![vec![]; 3],
                working_desires: vec![],
                demographics: DemoRow {
                    count: 10.0,
                    household: HouseholdDef::default(),
                    species: 0,
                    culture: 0,
                    class: 0,
                    religion: 0,
                },
            }
        }

        fn make_desire(idx: usize, desire_target: DesireTarget, amount: f64) -> Desire {
            // Source doesn't matter for most uses, it's just for tracking purpopses.
            Desire {
                idx,
                source: DesireSource::Religion(0),
                target: vec![desire_target],
                amount,
                satisfaction: 0.0,
                category: None,
                effect: vec![],
                scalar: ScalingFactor::Household,
            }
        }

        fn add_desire(mut pop: Pop, desire: Desire, tier: usize) -> Pop {
            pop.desires[tier].push(desire);
            pop
        }

        mod consume_should {
            use super::*;

            #[test]
            fn correctly_satisfy_desires_across_all_tiers() {
                // make pop
                let mut pop = make_pop();

                // make a bunch of desires across it's tiers.
                // ensure shared good between at least 2 tiers.
                // 2 basic
                let basicdesire1 = make_desire(0, 
                    DesireTarget::new(100, DesireTargetType::Consume, 1.0), 10.0);
                let basicdesire2 = make_desire(1, 
                    DesireTarget::new(101, DesireTargetType::Consume, 1.0), 10.0);
                pop.desires[0].push(basicdesire1);
                pop.desires[0].push(basicdesire2);
                // 2 common
                let commondesire1 = make_desire(0, 
                    DesireTarget::new(200, DesireTargetType::Consume, 1.0), 10.0);
                let commondesire2 = make_desire(1, 
                    DesireTarget::new(101, DesireTargetType::Consume, 1.0), 10.0);
                pop.desires[1].push(commondesire1);
                pop.desires[1].push(commondesire2);
                // 2 luxuries
                let luxurydesire1 = make_desire(0, 
                    DesireTarget::new(300, DesireTargetType::Consume, 1.0), 10.0);
                let luxurydesire2 = make_desire(1, 
                    DesireTarget::new(100, DesireTargetType::Consume, 1.0), 10.0);
                pop.desires[2].push(luxurydesire1);
                pop.desires[2].push(luxurydesire2);

                // fill it's property as needed.
                pop.property.insert(100, 
                    PopPRow::new(100.0).with_reserve(100.0));
                pop.property.insert(101, 
                    PopPRow::new(100.0).with_reserve(20.0));
                pop.property.insert(200, 
                    PopPRow::new(100.0).with_reserve(10.0));
                pop.property.insert(300, 
                    PopPRow::new(100.0).with_reserve(100.0));

                // run test
                pop.consume();

                // check results
                // check property is correct
                assert_eq!(pop.property[&100].quantity, 0.0);
                assert_eq!(pop.property[&100].reserved, 0.0);
                assert_eq!(pop.property[&100].consumed, 100.0);
                assert_eq!(pop.property[&101].quantity, 80.0);
                assert_eq!(pop.property[&101].reserved, 0.0);
                assert_eq!(pop.property[&101].consumed, 20.0);
                assert_eq!(pop.property[&200].quantity, 90.0);
                assert_eq!(pop.property[&200].reserved, 0.0);
                assert_eq!(pop.property[&200].consumed, 10.0);
                assert_eq!(pop.property[&300].quantity, 0.0);
                assert_eq!(pop.property[&300].reserved, 0.0);
                assert_eq!(pop.property[&300].consumed, 100.0);
                // check 
                assert_eq!(pop.desires[0][0].satisfaction, 10.0);
                assert_eq!(pop.desires[0][1].satisfaction, 10.0);
                assert_eq!(pop.desires[1][0].satisfaction, 10.0);
                assert_eq!(pop.desires[1][1].satisfaction, 10.0);
                assert_eq!(pop.desires[2][0].satisfaction, 100.0);
                assert_eq!(pop.desires[2][1].satisfaction, 90.0);
            }
        }

        mod satisfy_tier_should {
            use super::*;

            #[test]
            fn satisfy_multiple_empty_desires() {
                // create Pop
                let mut test_pop = make_pop();

                // new up some simple desires
                let des1 = make_desire(0, DesireTarget::new(100, 
                    DesireTargetType::Consume, 1.0), 10.0);
                let des2 = make_desire(1, DesireTarget::new(101, 
                    DesireTargetType::Consume, 1.0), 10.0);
                let des3 = make_desire(2, DesireTarget::new(100, 
                    DesireTargetType::Consume, 1.0), 10.0);
                let mut test_desires = vec![des1, des2, des3];

                // insert property to be consumed, don't forget the reservations
                test_pop.property.insert(100, PopPRow::new(100.0).with_reserve(20.0));
                test_pop.property.insert(101, PopPRow::new(100.0).with_reserve(20.0));

                // call function
                let result = test_pop.satisfy_tier(&mut test_desires);

                // check outcomes
                assert_eq!(result, 1.0);
                assert_eq!(test_pop.property[&100].quantity, 80.0);
                assert_eq!(test_pop.property[&100].reserved, 0.0);
                assert_eq!(test_pop.property[&100].consumed, 20.0);
                assert_eq!(test_pop.property[&101].quantity, 90.0);
                assert_eq!(test_pop.property[&101].reserved, 10.0);
                assert_eq!(test_pop.property[&101].consumed, 10.0);
                assert_eq!(test_desires[0].satisfaction, 10.0);
                assert_eq!(test_desires[1].satisfaction, 10.0);
                assert_eq!(test_desires[2].satisfaction, 10.0);
            }

            #[test]
            fn satisfy_multiple_after_first_pass_desires() {
                // create Pop
                let mut test_pop = make_pop();

                // new up some simple desires
                let mut des1 = make_desire(0, DesireTarget::new(100, 
                    DesireTargetType::Consume, 1.0), 10.0);
                    des1.satisfaction = 10.0;
                let mut des2 = make_desire(1, DesireTarget::new(101, 
                    DesireTargetType::Consume, 1.0), 10.0);
                    des2.satisfaction = 10.0;
                let mut des3 = make_desire(2, DesireTarget::new(100, 
                    DesireTargetType::Consume, 1.0), 10.0);
                    des3.satisfaction = 10.0;
                let mut test_desires = vec![des1, des2, des3];

                // insert property to be consumed, don't forget the reservations
                test_pop.property.insert(100, PopPRow::new(100.0).with_reserve(20.0));
                test_pop.property.insert(101, PopPRow::new(100.0).with_reserve(20.0));

                // call function
                let result = test_pop.satisfy_tier(&mut test_desires);

                // check outcomes
                assert_eq!(result, 2.0);
                assert_eq!(test_pop.property[&100].quantity, 80.0);
                assert_eq!(test_pop.property[&100].reserved, 0.0);
                assert_eq!(test_pop.property[&100].consumed, 20.0);
                assert_eq!(test_pop.property[&101].quantity, 90.0);
                assert_eq!(test_pop.property[&101].reserved, 10.0);
                assert_eq!(test_pop.property[&101].consumed, 10.0);
                assert_eq!(test_desires[0].satisfaction, 20.0);
                assert_eq!(test_desires[1].satisfaction, 20.0);
                assert_eq!(test_desires[2].satisfaction, 20.0);
            }

            #[test]
            fn return_largest_when_not_equal_satisfactions() {
                // create Pop
                let mut test_pop = make_pop();

                // new up some simple desires
                let des1 = make_desire(0, DesireTarget::new(100, 
                    DesireTargetType::Consume, 1.0), 10.0);
                let des2 = make_desire(1, DesireTarget::new(101, 
                    DesireTargetType::Consume, 1.0), 10.0);
                let des3 = make_desire(2, DesireTarget::new(100, 
                    DesireTargetType::Consume, 1.0), 10.0);
                let mut test_desires = vec![des1, des2, des3];

                // insert property to be consumed, don't forget the reservations
                test_pop.property.insert(100, PopPRow::new(7.0).with_reserve(7.0));
                test_pop.property.insert(101, PopPRow::new(1.0).with_reserve(1.0));

                // call function
                let result = test_pop.satisfy_tier(&mut test_desires);

                // check outcomes
                assert_eq!(result, 0.7);
                assert_eq!(test_pop.property[&100].quantity, 0.0);
                assert_eq!(test_pop.property[&100].reserved, 0.0);
                assert_eq!(test_pop.property[&100].consumed, 7.0);
                assert_eq!(test_pop.property[&101].quantity, 0.0);
                assert_eq!(test_pop.property[&101].reserved, 0.0);
                assert_eq!(test_pop.property[&101].consumed, 1.0);
                assert_eq!(test_desires[0].satisfaction, 7.0);
                assert_eq!(test_desires[1].satisfaction, 1.0);
                assert_eq!(test_desires[2].satisfaction, 0.0);
            }
        }

        mod satisfy_one_desire_should {
            use super::*;

            #[test]
            fn correctly_satisfy_simple_consume_desire() {
                let mut test_pop = make_pop();

                test_pop.property.insert(CONSUMED_GOOD, 
                    PopPRow::new(100.0).with_reserve(10.0));
                
                let mut test_desire = make_desire(0, DesireTarget::new(CONSUMED_GOOD, 
                    DesireTargetType::Consume, 1.0), 10.0);

                let result = test_pop.satisfy_one_desire(&mut test_desire);
                assert_eq!(result, 1.0);
                assert_eq!{test_desire.satisfaction, 10.0};
                assert_eq!(test_pop.property[&CONSUMED_GOOD].quantity, 90.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].reserved, 0.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].consumed, 10.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].used, 0.0);
            }

            #[test]
            fn correctly_satisfy_simple_use_desire() {
                let mut test_pop = make_pop();

                test_pop.property.insert(USED_GOOD, 
                    PopPRow::new(100.0).with_reserve(10.0));
                
                let mut test_desire = make_desire(0, DesireTarget::new(USED_GOOD, 
                    DesireTargetType::Use, 1.0), 10.0);

                let result = test_pop.satisfy_one_desire(&mut test_desire);
                assert_eq!(result, 1.0);
                assert_eq!{test_desire.satisfaction, 10.0};
                assert_eq!(test_pop.property[&USED_GOOD].quantity, 90.0);
                assert_eq!(test_pop.property[&USED_GOOD].reserved, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].consumed, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].used, 10.0);
            }

            #[test]
            fn partially_fill_desire() {
                let mut test_pop = make_pop();

                test_pop.property.insert(CONSUMED_GOOD, 
                    PopPRow::new(5.0).with_reserve(5.0));
                
                let mut test_desire = make_desire(0, DesireTarget::new(CONSUMED_GOOD, 
                    DesireTargetType::Consume, 1.0), 10.0);

                let result = test_pop.satisfy_one_desire(&mut test_desire);
                assert_eq!(result, 0.5);
                assert_eq!{test_desire.satisfaction, 5.0};
                assert_eq!(test_pop.property[&CONSUMED_GOOD].quantity, 0.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].reserved, 0.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].consumed, 5.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].used, 0.0);
            }

            #[test]
            fn not_touch_savings() {
                let mut test_pop = make_pop();

                let prop = PopPRow::new(10.0)
                    .with_saved(5.0)
                    .with_reserve(5.0);
                test_pop.property.insert(CONSUMED_GOOD, prop);
                
                let mut test_desire = make_desire(0, DesireTarget::new(CONSUMED_GOOD, 
                    DesireTargetType::Consume, 1.0), 10.0);

                let result = test_pop.satisfy_one_desire(&mut test_desire);
                assert_eq!(result, 0.5);
                assert_eq!{test_desire.satisfaction, 5.0};
                assert_eq!(test_pop.property[&CONSUMED_GOOD].quantity, 5.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].reserved, 0.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].saved, 5.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].consumed, 5.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].used, 0.0);
            }

            #[test]
            fn correctly_satisfy_complex_desire_same_efficiencies() {
                let mut test_pop = make_pop();
                
                let mut test_desire = make_desire(0, DesireTarget::new(USED_GOOD, 
                    DesireTargetType::Use, 1.0), 10.0);
                test_desire.target.push(
                    DesireTarget::new(CONSUMED_GOOD, DesireTargetType::Consume, 1.0));

                // Split evenly
                test_pop.property.insert(CONSUMED_GOOD, PopPRow::new(5.0)
                    .with_reserve(5.0));
                test_pop.property.insert(USED_GOOD, PopPRow::new(5.0)
                    .with_reserve(5.0));

                let result = test_pop.satisfy_one_desire(&mut test_desire);
                assert_eq!(result, 1.0);
                assert_eq!{test_desire.satisfaction, 10.0};
                assert_eq!(test_pop.property[&CONSUMED_GOOD].quantity, 0.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].reserved, 0.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].consumed, 5.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].used, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].quantity, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].reserved, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].consumed, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].used, 5.0);
            }

            #[test]
            fn correctly_satisfy_complex_desire_different_efficiencies() {
                let mut test_pop = make_pop();
                
                let mut test_desire = make_desire(0, DesireTarget::new(USED_GOOD, 
                    DesireTargetType::Use, 0.5), 10.0);
                test_desire.target.push(
                    DesireTarget::new(CONSUMED_GOOD, DesireTargetType::Consume, 1.0));

                // Split evenly
                test_pop.property.insert(CONSUMED_GOOD, PopPRow::new(5.0)
                    .with_reserve(5.0));
                test_pop.property.insert(USED_GOOD, PopPRow::new(5.0)
                    .with_reserve(5.0));

                let result = test_pop.satisfy_one_desire(&mut test_desire);
                assert_eq!(result, 0.75);
                assert_eq!{test_desire.satisfaction, 7.5};
                assert_eq!(test_pop.property[&CONSUMED_GOOD].quantity, 0.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].reserved, 0.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].consumed, 5.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].used, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].quantity, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].reserved, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].consumed, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].used, 5.0);
            }

            #[test]
            fn correctly_satisfy_complex_desire_capped_inputs() {
                let mut test_pop = make_pop();
                
                let mut test_target = DesireTarget::new(USED_GOOD, DesireTargetType::Use, 1.0);
                test_target.cap = 0.5;
                let mut test_desire = make_desire(0, test_target, 10.0);
                test_desire.target.push(
                    DesireTarget::new(CONSUMED_GOOD, DesireTargetType::Consume, 1.0));

                // Split evenly
                test_pop.property.insert(CONSUMED_GOOD, PopPRow::new(10.0)
                    .with_reserve(5.0));
                test_pop.property.insert(USED_GOOD, PopPRow::new(10.0)
                    .with_reserve(5.0));

                let result = test_pop.satisfy_one_desire(&mut test_desire);
                assert_eq!(result, 1.0);
                assert_eq!{test_desire.satisfaction, 10.0};
                assert_eq!(test_pop.property[&CONSUMED_GOOD].quantity, 5.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].reserved, 0.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].consumed, 5.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].used, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].quantity, 5.0);
                assert_eq!(test_pop.property[&USED_GOOD].reserved, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].consumed, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].used, 5.0);
            }

            #[test]
            fn correctly_satisfy_complex_desire_with_correct_order_priority() {
                let mut test_pop = make_pop();
                
                let used_target = DesireTarget::new(USED_GOOD, DesireTargetType::Use, 1.0);

                let mut test_desire = make_desire(0, used_target, 10.0);
                test_desire.target.push(
                    DesireTarget::new(CONSUMED_GOOD, DesireTargetType::Consume, 1.0));

                // used first, consumed second
                test_pop.property.insert(CONSUMED_GOOD, PopPRow::new(10.0));
                test_pop.property.insert(USED_GOOD, PopPRow::new(10.0)
                    .with_reserve(10.0));

                let result = test_pop.satisfy_one_desire(&mut test_desire);
                assert_eq!(result, 1.0);
                assert_eq!{test_desire.satisfaction, 10.0};
                assert_eq!(test_pop.property[&CONSUMED_GOOD].quantity, 10.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].reserved, 0.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].consumed, 0.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].used, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].quantity, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].reserved, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].consumed, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].used, 10.0);
            }

            #[test]
            fn correctly_satisfy_complex_desire_with_correct_efficiency_priority() {
                let mut test_pop = make_pop();
                
                let test_target = DesireTarget::new(USED_GOOD, DesireTargetType::Use, 1.0);
                let mut test_desire = make_desire(0, test_target, 10.0);

                test_desire.target.push(
                    DesireTarget::new(CONSUMED_GOOD, DesireTargetType::Consume, 1.25));

                // consume first, then used
                test_pop.property.insert(CONSUMED_GOOD, PopPRow::new(10.0)
                    .with_reserve(8.0));
                test_pop.property.insert(USED_GOOD, PopPRow::new(10.0)
                    .with_reserve(10.0));

                let result = test_pop.satisfy_one_desire(&mut test_desire);
                assert_eq!(result, 1.0);
                assert_eq!{test_desire.satisfaction, 10.0};
                assert_eq!(test_pop.property[&CONSUMED_GOOD].quantity, 2.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].reserved, 0.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].consumed, 8.0);
                assert_eq!(test_pop.property[&CONSUMED_GOOD].used, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].quantity, 10.0);
                assert_eq!(test_pop.property[&USED_GOOD].reserved, 10.0);
                assert_eq!(test_pop.property[&USED_GOOD].consumed, 0.0);
                assert_eq!(test_pop.property[&USED_GOOD].used, 0.0);
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

        fn empty_firm_row(quantity: f64) -> FirmPRow {
            FirmPRow {
                quantity,
                rolling_average: 0.0,
                target: 0.0,
                reserve: 0.0,
                average_cost: 0.0,
                used_capital: 0.0,
            }
        }

        fn empty_production_line(process_id: usize) -> ProductionLine {
            ProductionLine {
                process: process_id,
                target: None,
                inputs: vec![],
                historical_productivity: 0.0,
                last_success_rate: 0.0,
                last_iterations: 0.0,
                last_effects: vec![],
                last_missing_goods: vec![],
                last_amv_consumed: 0.0,
                last_amv_produced: 0.0,
            }
        }

        mod run_production_should {
            use crate::game::process::InputEffect;
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
        
            #[test]
            fn test_multi_line_chain_with_shared_capital() {
                // Line 1: wood (Consumed) + saw (Capital) → planks
                // Line 2: planks (Consumed) + saw (Capital) → furniture
                let sawmill = Process::new(10, "sawmill", 0)
                    .with_input(ProcessInput::new(100, 1.0, true, InputType::Destroyed, false)) // wood
                    .with_input(ProcessInput::new(200, 1.0, true, InputType::Capital, false))  // saw
                    .with_output(ProcessOutput::new(110, 1.0, true)); // planks

                let workshop = Process::new(11, "workshop", 0)
                    .with_input(ProcessInput::new(110, 1.0, true, InputType::Destroyed, false)) // planks
                    .with_input(ProcessInput::new(200, 1.0, true, InputType::Capital, false))  // same saw
                    .with_output(ProcessOutput::new(120, 1.0, true)); // furniture

                let mut factuals = make_factuals_with_process(sawmill);
                factuals.processes.insert(11, workshop);
                factuals.goods.insert(100, make_good(100, "wood", HashMap::new()));
                factuals.goods.insert(110, make_good(110, "plank", HashMap::new()));
                factuals.goods.insert(120, make_good(120, "table", HashMap::new()));
                factuals.goods.insert(200, make_good(200, "saw", HashMap::new()));

                let mut firm = Firm::new(1, "Integrated Workshop".into(), 42, hexx::Hex::new(0, 0));
                firm.property.insert(100, empty_firm_row(20.0)); // wood
                firm.property.insert(200, empty_firm_row(20.0));  // saw (shared capital)
                firm.property.insert(110, empty_firm_row(0.0));  // planks (will be produced then consumed)

                // Two lines in priority order
                firm.production_line.push(empty_production_line(10)); // sawmill
                firm.production_line[0].inputs = vec![100, 200];
                firm.production_line[0].target = Some(5.0);

                firm.production_line.push(empty_production_line(11)); // workshop
                firm.production_line[1].inputs = vec![110, 200];
                firm.production_line[1].target = Some(3.0);

                let market = make_market_with_amvs(&[(100, 2.0), (110, 5.0), (120, 15.0), (200, 50.0)]);

                let report = firm.run_production(&factuals, &market);

                // Property assertions
                assert_eq!(firm.property[&100].quantity, 15.0);   // 20 - 5
                assert_eq!(firm.property[&110].quantity, 2.0);    // produced 5, consumed 3, 
                assert_eq!(firm.property[&200].used_capital, 8.0); // 5 + 3
                assert_eq!(firm.property[&200].quantity, 12.0);    // 20- 5 - 3
                // (adjust expected numbers based on exact per-iter costs you want)

                // Report aggregation across both lines
                assert_eq!(report.produced.get(&110), Some(&5.0)); // planks created
                assert_eq!(report.produced.get(&120), Some(&3.0)); // tables created
                assert_eq!(report.consumed.get(&100), Some(&5.0)); // wood
                assert_eq!(report.consumed.get(&110), Some(&3.0));  // planks consumed in line 2
                assert!(report.consumed.get(&200).is_none());       // capital never in consumed

                // Both lines recorded AMV snapshots
                assert_eq!(firm.production_line[0].last_amv_consumed, 10.0);
                assert_eq!(firm.production_line[0].last_amv_produced, 25.0);
                assert_eq!(firm.production_line[1].last_amv_consumed, 15.0);
                assert_eq!(firm.production_line[1].last_amv_produced, 45.0);
            }

            #[test]
            fn test_required_and_optional_factors() {
                // Required factor (water) + optional factor (skilled labor bonus)
                let process = Process::new(20, "factor_test", 0)
                    .with_input(ProcessInput::new(100, 1.0, true, InputType::Destroyed, false))
                    .with_input(ProcessInput::new(110, 1.0, false, InputType::Destroyed, false))
                    .with_input(ProcessInput::new(300, 1.0, true, InputType::Factor, false)) // required water
                    .with_input(ProcessInput::new(301, 1.0, true, InputType::Factor, true)   // optional skilled
                        .with_optional(InputEffect::Throughput(0.5)))
                    .with_output(ProcessOutput::new(120, 1.0, false));

                let mut factuals = make_factuals_with_process(process);
                factuals.goods.insert(100, make_good(100, "wood", HashMap::new()));
                factuals.goods.insert(110, make_good(110, "planks", HashMap::new()));
                factuals.goods.insert(120, make_good(120, "ash", HashMap::new()));
                factuals.goods.insert(300, make_good(300, "sunlight", HashMap::new()));
                factuals.goods.insert(301, make_good(301, "clear skys", HashMap::new()));

                let mut firm = Firm::new(2, "Factor Firm".into(), 42, hexx::Hex::new(0, 0));
                firm.property.insert(100, empty_firm_row(20.0));
                firm.property.insert(110, empty_firm_row(40.0));
                firm.property.insert(300, empty_firm_row(1.0)); // has required factor
                // 301 (skilled) deliberately missing

                firm.production_line.push(empty_production_line(20));
                firm.production_line[0].inputs = vec![100, 110, 300, 301];
                firm.production_line[0].target = None;

                let market = make_market_with_amvs(&[(100, 2.0), (110, 6.0), (120, 20.0)]);

                let report = firm.run_production(&factuals, &market);

                // Should run (required factor present) but without the optional throughput bonus
                assert!(firm.production_line[0].last_success_rate > 0.9);
                assert_eq!(firm.production_line[0].last_iterations, 20.0);
                assert_eq!(firm.production_line[0].last_missing_goods.len(), 1);
                assert!(firm.production_line[0].last_missing_goods.contains(&100));
                assert_eq!(firm.production_line[0].last_amv_consumed, 160.0);
                assert_eq!(firm.production_line[0].last_amv_produced, 400.0);
                assert_eq!(report.consumed.get(&100), Some(&20.0)); // 10 iterations * 2.0
                assert_eq!(report.consumed.get(&110), Some(&20.0)); // 10 iterations * 2.0
                assert_eq!(report.produced.get(&120), Some(&20.0)); // 10 iterations * 2.0

                // test with optional factor included
                firm.property.insert(301, empty_firm_row(1.0));
                firm.property.get_mut(&100).unwrap().quantity += 20.0;
                firm.property.get_mut(&110).unwrap().quantity += 20.0;
                firm.production_line[0].last_amv_consumed = 0.0;
                firm.production_line[0].last_amv_produced = 0.0;
                firm.production_line[0].last_iterations = 0.0;
                firm.production_line[0].last_success_rate = 0.0;

                let report = firm.run_production(&factuals, &market);

                // Should run (required factor present) but without the optional throughput bonus
                assert!(firm.production_line[0].last_success_rate > 0.9);
                assert_eq!(firm.production_line[0].last_iterations, 20.0);
                assert_eq!(firm.production_line[0].last_missing_goods.len(), 1);
                assert!(firm.production_line[0].last_missing_goods.contains(&100));
                assert_eq!(firm.production_line[0].last_amv_consumed, 220.0);
                assert_eq!(firm.production_line[0].last_amv_produced, 600.0);
                assert_eq!(report.consumed.get(&100), Some(&20.0)); // 10 iterations * 2.0
                assert_eq!(report.consumed.get(&110), Some(&30.0)); // 10 iterations * 2.0
                assert_eq!(report.produced.get(&120), Some(&30.0)); // 10 iterations * 2.0
            }

            #[test]
            fn test_optional_inputs_and_bonuses() {
                let process = Process::new(30, "optional_bonus", 0)
                    .with_input(ProcessInput::new(100, 1.0, true, InputType::Destroyed, false))
                    .with_input(ProcessInput::new(400, 1.0, true, InputType::Destroyed, true) // optional catalyst
                        .with_optional(InputEffect::Output(0.25))) // +25% output
                    .with_output(ProcessOutput::new(110, 1.0, false));

                let mut factuals = make_factuals_with_process(process);
                factuals.goods.insert(100, make_good(100, "wood", HashMap::new()));
                factuals.goods.insert(400, make_good(400, "ash", HashMap::new()));
                factuals.goods.insert(110, make_good(110, "treated wood", HashMap::new()));

                let mut firm = Firm::new(3, "Catalyst Tester".into(), 42, hexx::Hex::new(0, 0));
                firm.property.insert(100, empty_firm_row(10.0));
                firm.property.insert(400, empty_firm_row(3.0)); // present → bonus applies

                firm.production_line.push(empty_production_line(30));
                firm.production_line[0].inputs = vec![100, 400];
                firm.production_line[0].target = None;

                let market = make_market_with_amvs(&[(100, 2.0), (110, 7.0), (400, 10.0)]);

                let report = firm.run_production(&factuals, &market);

                // With catalyst bonus we should get more than the base 5 iterations worth of output
                assert_eq!(firm.production_line[0].last_iterations, 10.0);
                assert_eq!(firm.production_line[0].last_amv_consumed, 50.0);
                assert_eq!(firm.production_line[0].last_amv_produced, 75.25);
                assert_eq!(report.consumed[&100], 10.0);
                assert_eq!(report.consumed[&400], 3.0);
                assert_eq!(report.produced[&110], 10.75);
            }

            #[test]
            fn test_decay_results_recorded_in_produced() {
                // Wood (Consumed) decays into sawdust
                let process = Process::new(40, "decay_test", 0)
                    .with_input(ProcessInput::new(100, 1.0, true, InputType::Consumed, false))
                    .with_output(ProcessOutput::new(110, 1.0, true));

                let mut factuals = make_factuals_with_process(process);
                // Add decay info to the good definition (even if goods map is mostly empty)
                let wood = Good {
                    id: 100,
                    name: "Wood".into(),
                    class: None,
                    decay_rate: 0.25,
                    decay_result: HashMap::from([(130, 0.5)]), // 50% becomes sawdust
                    tags: Default::default(),
                    categories: vec![],
                };
                factuals.goods.insert(100, wood);
                factuals.goods.insert(130, make_good(110, "nice wood", HashMap::new()));
                factuals.goods.insert(130, make_good(130, "ash", HashMap::new()));

                let mut firm = Firm::new(4, "Decay Workshop".into(), 42, hexx::Hex::new(0, 0));
                firm.property.insert(100, empty_firm_row(8.0));

                firm.production_line.push(empty_production_line(40));
                firm.production_line[0].inputs = vec![100];
                firm.production_line[0].target = None;

                let market = make_market_with_amvs(&[(100, 2.0), (110, 6.0), (130, 0.5)]);

                let report = firm.run_production(&factuals, &market);

                assert_eq!(report.produced.get(&110), Some(&8.0));  // main output
                assert_eq!(report.produced.get(&130), Some(&4.0));  // decay result (8 iters * 0.5)
                assert_eq!(report.consumed.get(&100), Some(&8.0)); 
                assert_eq!(firm.production_line[0].last_amv_consumed, 16.0);
                assert_eq!(firm.production_line[0].last_amv_produced, 50.0);
                assert_eq!(firm.production_line[0].last_iterations, 8.0);
            }

            #[test]
            fn test_target_with_throughput_bonus_overshoot() {
                // Throughput bonus from optional input should allow more iterations than target
                // (per do_process rules: target is scaled on fixed inputs only)
                let process = Process::new(50, "throughput_target", 0)
                    .with_input(ProcessInput::new(100, 1.0, true, InputType::Destroyed, false))
                    .with_input(ProcessInput::new(110, 1.0, false, InputType::Destroyed, false))
                    .with_input(ProcessInput::new(500, 1.0, true, InputType::Destroyed, true)
                        .with_optional(InputEffect::Throughput(1.0))) // doubles throughput
                    .with_output(ProcessOutput::new(120, 1.0, true))
                    .with_output(ProcessOutput::new(130, 1.0, false));

                let mut factuals = make_factuals_with_process(process);
                factuals.goods.insert(100, make_good(100, "fixed good", HashMap::new()));
                factuals.goods.insert(110, make_good(110, "normal good", HashMap::new()));
                factuals.goods.insert(120, make_good(120, "fixed output", HashMap::new()));
                factuals.goods.insert(130, make_good(130, "normal output", HashMap::new()));
                factuals.goods.insert(500, make_good(500, "bonus good", HashMap::new()));

                let mut firm = Firm::new(5, "Throughput Lab".into(), 42, hexx::Hex::new(0, 0));
                firm.property.insert(100, empty_firm_row(20.0));
                firm.property.insert(110, empty_firm_row(40.0));
                firm.property.insert(500, empty_firm_row(5.0)); // enough for bonus

                firm.production_line.push(empty_production_line(50));
                firm.production_line[0].inputs = vec![100, 110, 500];
                firm.production_line[0].target = Some(8.0); // would be 8 without bonus, more with it

                let market = make_market_with_amvs(&[(100, 2.0), (110, 3.0), (120, 10.0), (130, 5.0), (500, 1.0)]);

                let report = firm.run_production(&factuals, &market);

                assert_eq!(report.produced.len(), 2);
                assert_eq!(report.consumed.len(), 3);
                assert_eq!(report.produced.get(&120), Some(&8.0));  // main output
                assert_eq!(report.produced.get(&130), Some(&13.0));  // decay result (8 iters * 0.5)
                assert_eq!(report.consumed.get(&100), Some(&8.0)); 
                assert_eq!(report.consumed.get(&110), Some(&13.0)); 
                assert_eq!(report.consumed.get(&500), Some(&5.0)); 
                assert_eq!(firm.production_line[0].last_amv_consumed, 2.0*8.0 + 3.0*13.0 + 5.0*1.0);
                assert_eq!(firm.production_line[0].last_amv_produced, 8.0*10.0 + 13.0*5.0);
                assert_eq!(firm.production_line[0].last_iterations, 8.0);
                assert_eq!(firm.property[&100].quantity, 12.0);
                assert_eq!(firm.property[&110].quantity, 27.0);
                assert_eq!(firm.property[&120].quantity, 8.0);
                assert_eq!(firm.property[&130].quantity, 13.0);
                assert_eq!(firm.property[&500].quantity, 0.0);
            }

            #[test]
            fn test_amv_fallback_uses_one_point_zero() {
                // Good 999 is deliberately missing from the Market
                let process = Process::new(60, "missing_good_amv", 0)
                    .with_input(ProcessInput::new(999, 1.0, true, InputType::Consumed, false))
                    .with_output(ProcessOutput::new(110, 1.0, true));

                let mut factuals = make_factuals_with_process(process);
                factuals.goods.insert(999, make_good(999, "missing market good", HashMap::new()));
                factuals.goods.insert(110, make_good(110, "output good", HashMap::new()));

                let mut firm = Firm::new(6, "Mystery Good Firm".into(), 42, hexx::Hex::new(0, 0));
                firm.property.insert(999, empty_firm_row(5.0));

                firm.production_line.push(empty_production_line(60));
                firm.production_line[0].inputs = vec![999];
                firm.production_line[0].target = None;

                // Market does NOT contain good 999
                let market = make_market_with_amvs(&[(110, 4.0)]);

                let _report = firm.run_production(&factuals, &market);

                // Should fall back to the economic default of 1.0
                assert_eq!(
                    firm.production_line[0].last_amv_consumed, 5.0,
                    "Missing goods should default to AMV 1.0"
                );
            }
        }
    }
}