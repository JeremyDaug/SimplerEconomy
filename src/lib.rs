pub mod game;

#[cfg(test)]
mod test {
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
            use super::*;

            #[test]
            fn return_empty_result_correctly() {
                let process = make_process();

                let mut available = HashMap::new();

                let mut factuals = make_factuals(vec![]);
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

                let mut factuals = make_factuals(vec![]);
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

                let mut factuals = make_factuals(vec![]);
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

                let mut factuals = make_factuals(vec![make_good(CONSUMED_GOOD, 
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

                let mut factuals = make_factuals(vec![]);
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
            fn correctly_include_factor_bonuses() {
                let process = make_process();

                let mut available = HashMap::new();
                available.insert(REQ_GOOD, 100.0);

                let mut factuals = make_factuals(vec![]);
                let result = process.do_process_leg(&available, None, 
                    (0.5, 1.0, 1.0), &factuals);

                assert_eq!(result.iterations, 200.0);
                assert_eq!(result.changes.len(), 2);
                assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
                assert_eq!(result.changes.get(&OUT_GOOD), Some(&200.0));
                assert_eq!(result.used_inputs.len(), 0);
                assert_eq!(result.effects.len(), 0);
            }
        }

        // ============================================================
        // TESTS — now with explicit goods consumption/production checks
        // ============================================================

        #[test]
        fn basic_process_and_target_plus_capital_and_fixed_good_check() {
            let process = make_process();

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 100.0);

            let mut factuals = make_factuals(vec![]);
            let result = process.do_process(&available, None, &factuals);

            // Check Results
            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 2); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert!(result.used_inputs.is_empty()); // confirm no capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.

            let result = process.do_process(&available, Some(50.0), &factuals);

            // Check Results
            assert_eq!(result.iterations, 50.0);
            assert_eq!(result.changes.len(), 2); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
            assert!(result.used_inputs.is_empty()); // confirm no capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.

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

            let result = process.do_process(&available, Some(50.0), &factuals);

            // Check Results
            assert_eq!(result.iterations, 50.0);
            assert_eq!(result.changes.len(), 2); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
            assert_eq!(result.used_inputs.len(), 1); // confirm capital used
            assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&50.0)); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.

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

            let result = process.do_process(&available, Some(50.0), &factuals);

            // Check Results
            assert_eq!(result.iterations, 50.0);
            assert_eq!(result.changes.len(), 2); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
            assert_eq!(result.used_inputs.len(), 0); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
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
            assert_eq!(result.changes.len(), 5); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&CONSUMED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 1); // confirm capital used
            assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&100.0)); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.

            let result = process.do_process(&available, Some(50.0), &factuals);

            // Check Results
            assert_eq!(result.iterations, 50.0);
            assert_eq!(result.changes.len(), 5); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&CONSUMED_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
            assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&50.0));
            assert_eq!(result.used_inputs.len(), 1); // confirm capital used
            assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&50.0)); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
        }

        #[test]
        fn process_with_optional_with_no_optionals_given() {
            let process = make_process()
                .with_input(make_optional_input(OPTIN_GOOD, 1.0, true, 
                    InputType::Destroyed, vec![OPTIN_EFFECT.clone()]));

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 100.0);
                
            let mut factuals = make_factuals(vec![]);
            let result = process.do_process(&available, None, &factuals);

            // Check Results
            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 2); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 0); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.

            let result = process.do_process(&available, Some(50.0), &factuals);

            // Check Results
            assert_eq!(result.iterations, 50.0);
            assert_eq!(result.changes.len(), 2); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
            assert_eq!(result.used_inputs.len(), 0); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
        }

        #[test]
        fn insufficient_inputs_reduce_output() {
            let process = make_process()
                .with_input(make_input(CAPITAL_GOOD, 1.0, true, InputType::Capital))
                .with_input(make_input(FIXED_GOOD, 1.0, true, InputType::Destroyed));
            let mut available = HashMap::new();

            available.insert(REQ_GOOD, 100.0);
            available.insert(CAPITAL_GOOD, 80.0);
            available.insert(FIXED_GOOD, 130.0);

            let mut factuals = make_factuals(vec![]);
            let result = process.do_process(&available, None, &factuals);

            // Check Results
            assert_eq!(result.iterations, 80.0);
            assert_eq!(result.changes.len(), 3); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-80.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-80.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&80.0));
            assert_eq!(result.used_inputs.len(), 1); // confirm capital used
            assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&80.0)); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.

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
        }

        #[test]
        fn optional_input_gives_proportional_bonus() {
            let process = make_process()
                .with_input(make_optional_input(OPTIN_GOOD, 1.0, true, 
                    InputType::Destroyed, vec![OPTIN_EFFECT.clone()]));
            let mut available = HashMap::new();

            available.insert(REQ_GOOD, 100.0);
            available.insert(OPTIN_GOOD, 50.0); // only half the optional provided

            let mut factuals = make_factuals(vec![]);
            let result = process.do_process(&available, None, &factuals);

            // Check Results
            assert_eq!(result.iterations, 110.0); // should be a 10% boost from the optional
            assert_eq!(result.changes.len(), 3); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&110.0));
            assert_eq!(result.used_inputs.len(), 0); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.

             let result = process.do_process(&available, Some(50.0), &factuals);

             // Check Results
             assert_eq!(result.iterations, 50.0); // should be a 10% boost from the optional
             assert_eq!(result.changes.len(), 3); // only req and out should be changed
             assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
             assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-50.0));
             assert_eq!(result.changes.get(&OUT_GOOD), Some(&55.0));
             assert_eq!(result.used_inputs.len(), 0); // confirm capital used
             assert!(result.effects.is_empty()); // confirm no stray effects.
        }

        #[test]
        fn efficiency_modifiers_alone_and_stacked() {
            let req_good = 10;
            let out_good = 20;
            let optin_good = 30;
            let optin_effect = InputEffect::Input(0.2); // 20% input reduction when fully supplied
            let optout_good = 31;
            let optout_effect = InputEffect::Output(0.3); // 30% output boost when fully supplied
            let optthrough_good = 32;
            let optthrough_effect = InputEffect::Throughput(0.25); // 25% throughput boost when fully supplied
            let mut available = HashMap::new();
            available.insert(req_good, 100.0);
            let factuals = make_factuals(vec![]);

            // Base process (no bonuses)
            let base = Process::new(1, "base", 0)
                .with_input(make_input(req_good, 1.0, false, InputType::Destroyed))
                .with_input(make_optional_input(optin_good, 1.0, false, InputType::Destroyed, vec![optin_effect]))
                .with_input(make_optional_input(optout_good, 1.0, false, InputType::Destroyed, vec![optout_effect]))
                .with_input(make_optional_input(optthrough_good, 1.0, false, InputType::Destroyed, vec![optthrough_effect]))
                .with_output(ProcessOutput::new(out_good, 1.0, false));

            // 1. Input modifier alone (20% reduction)
            available.insert(optin_good, 100.0); // enough for full bonus
            let res_input = base.do_process(&available, None, &factuals);
            assert_eq!(res_input.iterations, 125.0, "Expected 125 iterations"); // 80 for first 100, full for remaining 20.
            assert!((res_input.changes.get(&req_good).unwrap_or(&0.0).abs() - 100.0).abs() < 0.01); // 80% for first 100, full for remaining 20
            assert!((res_input.changes.get(&optin_good).unwrap_or(&0.0).abs() - 100.0).abs() < 0.01); // all of the optional consumed
            assert!((res_input.changes.get(&out_good).unwrap_or(&0.0).abs() - 125.0).abs() < 0.01); // output should match iterations

            // 2. Output modifier alone (+30% output)
            available.remove(&optin_good);
            available.insert(optout_good, 100.0); // enough for full bonus
            let res_output = base.do_process(&available, None, &factuals);
            assert!((res_output.iterations - 100.0).abs() < 0.01); // 100 for all.
            assert!((res_output.changes.get(&req_good).unwrap_or(&0.0).abs() - 100.0).abs() < 0.01); // Full requirement
            assert!((res_output.changes.get(&optout_good).unwrap_or(&0.0).abs() - 100.0).abs() < 0.01); // all of the optional consumed
            assert!((res_output.changes.get(&out_good).unwrap_or(&0.0).abs() - 130.0).abs() < 0.01); // output should be increased.

            // 3. Throughput modifier alone (+25% both sides)
            available.remove(&optout_good);
            available.insert(optthrough_good, 100.0); // enough for full bonus
            let res_throughput = base.do_process(&available, None, &factuals);
            assert!((res_throughput.iterations - 120.0).abs() < 0.01); // 80 for first 100, full for remaining 20.
            assert!((res_throughput.changes.get(&req_good).unwrap_or(&0.0).abs() - 100.0).abs() < 0.01); // 80% for first 100, full for remaining 20
            assert!((res_throughput.changes.get(&optin_good).unwrap_or(&0.0).abs() - 100.0).abs() < 0.01); // all of the optional consumed
            assert!((res_throughput.changes.get(&out_good).unwrap_or(&0.0).abs() - 120.0).abs() < 0.01); // output should match iterations

            // 4. All stacked together on the same base
            // let stacked = base
            //     .with_input(make_optional_input(30, 1.0, false, InputType::Destroyed, vec![InputEffect::Input(0.2)]))
            //     .with_input(make_optional_input(31, 1.0, false, InputType::Destroyed, vec![InputEffect::Output(0.3)]))
            //     .with_input(make_optional_input(32, 1.0, false, InputType::Destroyed, vec![InputEffect::Throughput(0.25)]));
            // //let res_stacked = run(stacked, None);
            // let final_consumed = res_stacked.changes.get(&req_good).copied().unwrap_or(0.0).abs();
            // let final_produced = res_stacked.changes.get(&out_good).copied().unwrap_or(0.0);
            // // Combined multipliers: input = 4*0.8*1.25, output = 2*1.3*1.25
            // assert!((res_stacked.iterations - 50.0).abs() < 0.01);
            // assert!((final_consumed - 4.0 * 0.8 * 1.25 * 50.0).abs() < 0.01);
            // assert!((final_produced - 2.0 * 1.3 * 1.25 * 50.0).abs() < 0.01);
        }

        #[test]
        fn optional_inputs_with_target_do_not_overshoot() {
            let req_good = 10;
            let opt_good = 30;
            let out_good = 20;

            let opt_effect = InputEffect::Throughput(0.5); // big boost if fully supplied
            let process = Process::new(1, "target_test", 0)
                .with_input(make_input(req_good, 2.0, false, InputType::Destroyed))
                .with_input(make_optional_input(opt_good, 1.0, false, InputType::Destroyed, vec![opt_effect]))
                .with_output(ProcessOutput::new(out_good, 1.0, false));

            let mut available = HashMap::new();
            available.insert(req_good, 1000.0); // way more than needed
            available.insert(opt_good, 1000.0); // enough for huge bonus

            let factuals = make_factuals(vec![]);
            let result = process.do_process(&available, Some(7.3), &factuals); // hard target

            assert!((result.iterations - 7.3).abs() < 0.001);

            // Goods exactly match the target (no overshoot despite bonus)
            let consumed_req = result.changes.get(&req_good).copied().unwrap_or(0.0).abs();
            let consumed_opt = result.changes.get(&opt_good).copied().unwrap_or(0.0).abs();
            let produced = result.changes.get(&out_good).copied().unwrap_or(0.0);

            assert!((consumed_req - 2.0 * 7.3).abs() < 0.01); // required still at base (bonus doesn't change target)
            assert!((consumed_opt - 1.0 * 7.3).abs() < 0.01); // optional only for the actual iterations
            assert!((produced - 1.0 * 7.3).abs() < 0.01);
        }

        #[test]
        fn optional_inputs_do_not_affect_fixed_inputs() {
            let req_good = 10;   // variable
            let fixed_good = 40; // capital-style fixed
            let out_good = 20;

            let opt_effect = InputEffect::Input(0.5); // 50% input reduction on variables
            let process = Process::new(1, "fixed_test", 0)
                .with_input(make_input(req_good, 4.0, false, InputType::Destroyed)) // variable
                .with_input(make_input(fixed_good, 3.0, true, InputType::Capital))   // fixed
                .with_input(make_optional_input(30, 1.0, false, InputType::Destroyed, vec![opt_effect]))
                .with_output(ProcessOutput::new(out_good, 2.0, false));

            let mut available = HashMap::new();
            available.insert(req_good, 100.0);
            available.insert(fixed_good, 50.0);
            available.insert(30, 10.0);

            let factuals = make_factuals(vec![]);
            let result = process.do_process(&available, None, &factuals);

            let consumed_var = result.changes.get(&req_good).copied().unwrap_or(0.0).abs();
            let used_fixed = result.used_inputs.get(&fixed_good).copied().unwrap_or(0.0);
            let produced = result.changes.get(&out_good).copied().unwrap_or(0.0);

            assert!(result.iterations > 0.0);
            assert!((consumed_var / result.iterations - 4.0 * 1.5).abs() < 0.01); // variable got the bonus
            assert!((used_fixed / result.iterations - 3.0).abs() < 0.001);       // fixed unchanged by bonus
            assert!((produced / result.iterations - 2.0).abs() < 0.01);          // output unaffected by fixed
        }

        #[test]
        fn capital_is_recorded_but_not_consumed() {
            let capital_good = 50;
            let req_good = 10;
            let output_good = 20;

            let process = Process::new(1, "use_furnace", 0)
                .with_input(make_input(capital_good, 1.0, true, InputType::Capital))
                .with_input(make_input(req_good, 2.0, false, InputType::Destroyed))
                .with_output(ProcessOutput::new(output_good, 1.0, false));

            let mut available = HashMap::new();
            available.insert(capital_good, 5.0);
            available.insert(req_good, 20.0);

            let factuals = make_factuals(vec![]);
            let result = process.do_process(&available, None, &factuals);

            assert!(result.iterations > 0.0);

            // Capital is recorded as used but NOT consumed
            assert_eq!(result.used_inputs.get(&capital_good), Some(&result.iterations));
            assert!(!result.changes.contains_key(&capital_good) || (result.changes[&capital_good]).abs() < 0.001);

            // Required input and output behave normally
            assert!((result.changes.get(&req_good).copied().unwrap_or(0.0) + 2.0 * result.iterations).abs() < 0.001);
            assert!((result.changes.get(&output_good).copied().unwrap_or(0.0) - 1.0 * result.iterations).abs() < 0.001);
        }

        #[test]
        fn consumed_input_produces_decay_products() {
            let wood = 100;
            let ash = 101;

            let mut decay = HashMap::new();
            decay.insert(ash, 0.4); // 40% becomes ash

            let good = make_good(wood, "wood", decay);
            let factuals = make_factuals(vec![good]);

            let process = Process::new(1, "burn_wood", 0)
                .with_input(make_input(wood, 5.0, false, InputType::Consumed))
                .with_output(ProcessOutput::new(200, 1.0, false));

            let mut available = HashMap::new();
            available.insert(wood, 20.0);

            let result = process.do_process(&available, None, &factuals);

            let wood_delta = result.changes.get(&wood).copied().unwrap_or(0.0);
            let ash_delta = result.changes.get(&ash).copied().unwrap_or(0.0);

            assert!(wood_delta < 0.0);
            assert!(ash_delta > 0.0);

            // Exact decay math: ash = 40% of wood consumed
            assert!((ash_delta / wood_delta.abs() - 0.4).abs() < 0.001);
            assert!((wood_delta + 5.0 * result.iterations).abs() < 0.001); // full consumption
        }

        #[test]
        fn effects_are_scaled_by_iterations() {
            let req_good = 10;

            let process = Process::new(1, "research_process", 0)
                .with_input(make_input(req_good, 1.0, false, InputType::Destroyed))
                .with_effect(ProcessEffect::Research(2.0));

            let mut available = HashMap::new();
            available.insert(req_good, 10.0);

            let factuals = make_factuals(vec![]);
            let result = process.do_process(&available, None, &factuals);

            let research = result.effects.iter().find_map(|e| match e {
                ProcessEffect::Research(v) => Some(*v),
                _ => None,
            });

            assert!(research.is_some());
            assert!((research.unwrap() - result.iterations * 2.0).abs() < 0.001);

            // Goods still correct
            assert!((result.changes.get(&req_good).copied().unwrap_or(0.0) + 10.0).abs() < 0.001);
        }
    }
}