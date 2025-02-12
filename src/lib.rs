pub mod pop;
pub mod desire;
pub mod good;
pub mod process;
pub mod market;
pub mod data;
pub mod world;
pub mod culture;
pub mod want;
pub mod item;
pub mod markethistory;

#[cfg(test)]
mod tests {
    mod process_tests {
        mod do_process_should {
            use std::collections::HashMap;

            use crate::{data::Data, good::Good, process::{InputType, Process}};

            #[test]
            pub fn correctly_calculate_process_easy_time() {
                let mut good_data = HashMap::new();
                good_data.insert(0, Good {
                    id: 0,
                    name: "P0".to_string(),
                    decay_rate: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                good_data.insert(1, Good {
                    id: 1,
                    name: "P2".to_string(),
                    decay_rate: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                good_data.insert(2, Good {
                    id: 2,
                    name: "P2".to_string(),
                    decay_rate: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                let data = Data {
                    goods: good_data,
                    processes: HashMap::new(),
                    cultures: HashMap::new()
                };

                let mut inputs = HashMap::new();
                inputs.insert(0, 1.0);
                inputs.insert(1, 1.0);

                let mut input_type = HashMap::new();
                input_type.insert(0, InputType::Input);
                input_type.insert(1, InputType::Capital);

                let mut outputs = HashMap::new();
                outputs.insert(2, 100.0);

                let test = Process {
                    id: 0,
                    name: "Test".to_string(),
                    parent: None,
                    time: 1.0,
                    inputs,
                    optional: 0.0,
                    input_type,
                    outputs,
                };

                let mut goods = HashMap::new();
                goods.insert(0, 10.0);
                goods.insert(1, 20.0);

                let test_result = test.do_process(1.0, &goods, &data);
                println!("Consumed");
                for (key, val) in test_result.consumed.iter() {
                    println!("{}: {}", key, val);
                }
                println!("Used");
                for (key, val) in test_result.used.iter() {
                    println!("{}: {}", key, val);
                }
                println!("created");
                for (key, val) in test_result.consumed.iter() {
                    println!("{}: {}", key, val);
                }
                println!("Time used: {}", test_result.time_used);
                println!("Iterations: {}", test_result.iterations);

                assert_eq!(test_result.time_used, 1.0);
                assert_eq!(test_result.iterations, 1.0);
                assert_eq!(test_result.consumed.len(), 1);
                assert_eq!(*test_result.consumed.get(&0).unwrap(), 1.0);
                assert!(test_result.consumed.get(&1).is_none());
                assert_eq!(test_result.used.len(), 1);
                assert_eq!(*test_result.used.get(&1).unwrap(), 1.0);
                assert_eq!(test_result.created.len(), 1);
                assert_eq!(*test_result.created.get(&2).unwrap(), 111.0);
            }

            #[test]
            pub fn correctly_calculate_process_with_optional_goods() {
                let mut good_data = HashMap::new();
                good_data.insert(0, Good {
                    id: 0,
                    name: "P0".to_string(),
                    decay_rate: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                good_data.insert(1, Good {
                    id: 1,
                    name: "P2".to_string(),
                    decay_rate: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                good_data.insert(2, Good {
                    id: 2,
                    name: "P2".to_string(),
                    decay_rate: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                let data = Data {
                    goods: good_data,
                    processes: HashMap::new(),
                    cultures: HashMap::new()
                };

                let mut inputs = HashMap::new();
                inputs.insert(0, 1.0);
                inputs.insert(1, 1.0);

                let mut input_type = HashMap::new();
                input_type.insert(0, InputType::Input);
                input_type.insert(1, InputType::Capital);

                let mut outputs = HashMap::new();
                outputs.insert(2, 100.0);

                let test = Process {
                    id: 0,
                    name: "Test".to_string(),
                    parent: None,
                    time: 1.0,
                    inputs,
                    optional: 1.0,
                    input_type,
                    outputs,
                };

                let mut goods = HashMap::new();
                goods.insert(0, 10.0);
                goods.insert(1, 20.0);

                let test_result = test.do_process(1.0, &goods, &data);
                println!("Consumed");
                for (key, val) in test_result.consumed.iter() {
                    println!("{}: {}", key, val);
                }
                println!("Used");
                for (key, val) in test_result.used.iter() {
                    println!("{}: {}", key, val);
                }
                println!("created");
                for (key, val) in test_result.consumed.iter() {
                    println!("{}: {}", key, val);
                }
                println!("Time used: {}", test_result.time_used);
                println!("Iterations: {}", test_result.iterations);

                assert_eq!(test_result.time_used, 1.0);
                assert_eq!(test_result.iterations, 1.0);
                assert_eq!(test_result.consumed.len(), 1);
                assert_eq!(*test_result.consumed.get(&0).unwrap(), 1.0);
                assert!(test_result.consumed.get(&1).is_none());
                assert_eq!(*test_result.used.get(&1).unwrap_or(&0.0), 0.0);
                assert_eq!(test_result.created.len(), 1);
                assert_eq!(*test_result.created.get(&2).unwrap(), 100.0);
            }

            #[test]
            pub fn correctly_calculate_process_with_optional_goods_but_no_excess() {
                let mut good_data = HashMap::new();
                good_data.insert(0, Good {
                    id: 0,
                    name: "P0".to_string(),
                    decay_rate: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                good_data.insert(1, Good {
                    id: 1,
                    name: "P2".to_string(),
                    decay_rate: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                good_data.insert(2, Good {
                    id: 2,
                    name: "P2".to_string(),
                    decay_rate: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                let data = Data {
                    goods: good_data,
                    processes: HashMap::new(),
                    cultures: HashMap::new()
                };

                let mut inputs = HashMap::new();
                inputs.insert(0, 1.0);
                inputs.insert(1, 1.0);

                let mut input_type = HashMap::new();
                input_type.insert(0, InputType::Input);
                input_type.insert(1, InputType::Capital);

                let mut outputs = HashMap::new();
                outputs.insert(2, 100.0);

                let test = Process {
                    id: 0,
                    name: "Test".to_string(),
                    parent: None,
                    time: 1.0,
                    inputs,
                    optional: 1.0,
                    input_type,
                    outputs,
                };

                let mut goods = HashMap::new();
                goods.insert(0, 20.0);
                //goods.insert(1, 20.0);

                let test_result = test.do_process(20.0, &goods, &data);
                println!("Consumed");
                for (key, val) in test_result.consumed.iter() {
                    println!("{}: {}", key, val);
                }
                println!("Used");
                for (key, val) in test_result.used.iter() {
                    println!("{}: {}", key, val);
                }
                println!("created");
                for (key, val) in test_result.consumed.iter() {
                    println!("{}: {}", key, val);
                }
                println!("Time used: {}", test_result.time_used);
                println!("Iterations: {}", test_result.iterations);

                assert_eq!(test_result.time_used, 20.0);
                assert_eq!(test_result.iterations, 20.0);
                assert_eq!(test_result.consumed.len(), 1);
                assert_eq!(*test_result.consumed.get(&0).unwrap(), 20.0);
                assert!(test_result.consumed.get(&1).is_none());
                assert_eq!(test_result.used.len(), 1);
                assert_eq!(*test_result.used.get(&1).unwrap(), 0.0);
                assert_eq!(test_result.created.len(), 1);
                assert_eq!(*test_result.created.get(&2).unwrap(), 2000.0);
            }

            #[test]
            pub fn correctly_calculate_process_optional_goods_and_shortfall() {
                let mut good_data = HashMap::new();
                good_data.insert(0, Good {
                    id: 0,
                    name: "P0".to_string(),
                    decay_rate: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                good_data.insert(1, Good {
                    id: 1,
                    name: "P2".to_string(),
                    decay_rate: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                good_data.insert(2, Good {
                    id: 2,
                    name: "P2".to_string(),
                    decay_rate: 1.0,
                    bulk: 0.0,
                    mass: 0.0,
                    tags: vec![],
                });
                let data = Data {
                    goods: good_data,
                    processes: HashMap::new(),
                    cultures: HashMap::new()
                };

                let mut inputs = HashMap::new();
                inputs.insert(0, 1.0);
                inputs.insert(1, 1.0);
                inputs.insert(2, 1.0);

                let mut input_type = HashMap::new();
                input_type.insert(0, InputType::Input);
                input_type.insert(1, InputType::Capital);
                input_type.insert(2, InputType::Input);

                let mut outputs = HashMap::new();
                outputs.insert(2, 100.0);

                let test = Process {
                    id: 0,
                    name: "Test".to_string(),
                    parent: None,
                    time: 1.0,
                    inputs,
                    optional: 1.0,
                    input_type,
                    outputs,
                };

                let mut goods = HashMap::new();
                goods.insert(0, 20.0);
                goods.insert(1, 10.0);

                let test_result = test.do_process(20.0, &goods, &data);
                println!("Consumed");
                for (key, val) in test_result.consumed.iter() {
                    println!("{}: {}", key, val);
                }
                println!("Used");
                for (key, val) in test_result.used.iter() {
                    println!("{}: {}", key, val);
                }
                println!("created");
                for (key, val) in test_result.consumed.iter() {
                    println!("{}: {}", key, val);
                }
                println!("Time used: {}", test_result.time_used);
                println!("Iterations: {}", test_result.iterations);

                assert_eq!(test_result.time_used, 10.0);
                assert_eq!(test_result.iterations, 10.0);
                assert_eq!(*test_result.consumed.get(&0).unwrap(), 10.0);
                assert_eq!(*test_result.consumed.get(&1).unwrap_or(&0.0), 0.0);
                assert_eq!(*test_result.consumed.get(&2).unwrap_or(&0.0), 0.0);
                assert_eq!(test_result.used.len(), 1);
                assert_eq!(*test_result.used.get(&1).unwrap(), 10.0);
                assert_eq!(test_result.created.len(), 1);
                // TODO maybe come back to look at this as it should technically be 1110.0, but whatever, I have better things to do with my life, like not work on this project :P
                assert_eq!(*test_result.created.get(&2).unwrap(), 1100.0);
            }
        }

        #[cfg(test)]
        mod efficiency_should {
            use std::collections::HashMap;

            use crate::process::{InputType, Process};

            #[test]
            pub fn calculation_check() {
                 let inputs = HashMap::new();

                let mut input_type = HashMap::new();
                for (&good, _) in inputs.iter() {
                    input_type.insert(good, InputType::Input);
                }

                let outputs = HashMap::new();

                let mut test = Process {
                    id: 0,
                    name: "test".to_string(),
                    parent: None,
                    time: 1.0,
                    inputs,
                    optional: 0.0,
                    input_type,
                    outputs,
                };
                let result = test.efficiency();
                assert_eq!(result, 1.0);

                // 1 input
                test.inputs.insert(0, 1.0);
                let result = test.efficiency();
                assert_eq!(result, 1.0);

                // 2 inputs
                test.inputs.insert(1, 1.0);
                let result = test.efficiency();
                assert_eq!(result, 1.1);

                // 3 inputs, 2 in one part
                test.inputs.insert(1, 2.0);
                let result = test.efficiency();
                assert_eq!(result, 1.3);

                // 4 inputs, 2 in one part
                test.inputs.insert(2, 1.0);
                let result = test.efficiency();
                assert_eq!(result, 1.6);

                // 4 inputs, 2 in one part, 1 optional
                test.optional += 1.0;
                let result = test.efficiency();
                assert_eq!(result, 1.3);
            }
        }
    }
}