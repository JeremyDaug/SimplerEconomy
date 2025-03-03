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
pub mod drow;
pub mod species;
pub mod household;

#[cfg(test)]
mod tests {
    mod process_tests {
        mod uses_input_should {
            use crate::process::{InputTag, Process, ProcessInput};

            #[test]
            pub fn sort_inputs_correctly() {
                // These should all be inserted and should end up in reversed order.
                let test = Process::new(0, "Test".to_string(), String::new())
                    .uses_input(ProcessInput::new(2, 10.0)
                        .with_tag(InputTag::Consumed))
                    .uses_input(ProcessInput::new(1, 10.0)
                        .with_tag(InputTag::Consumed))
                    .uses_input(ProcessInput::new(0, 10.0)
                        .with_tag(InputTag::Consumed))
                    .uses_input(ProcessInput::new(2, 10.0)
                        .with_tag(InputTag::Used))
                    .uses_input(ProcessInput::new(1, 10.0)
                        .with_tag(InputTag::Used))
                    .uses_input(ProcessInput::new(0, 10.0)
                        .with_tag(InputTag::Used))
                    .uses_input(ProcessInput::new(2, 10.0))
                    .uses_input(ProcessInput::new(1, 10.0))
                    .uses_input(ProcessInput::new(0, 10.0));

                // test inputs here
                let input = test.inputs.get(0).unwrap();
                assert_eq!(input.good, 0);
                assert_eq!(input.tag, InputTag::None);
                let input = test.inputs.get(1).unwrap();
                assert_eq!(input.good, 1);
                assert_eq!(input.tag, InputTag::None);
                let input = test.inputs.get(2).unwrap();
                assert_eq!(input.good, 2);
                assert_eq!(input.tag, InputTag::None);

                let input = test.inputs.get(3).unwrap();
                assert_eq!(input.good, 0);
                assert_eq!(input.tag, InputTag::Used);
                let input = test.inputs.get(4).unwrap();
                assert_eq!(input.good, 1);
                assert_eq!(input.tag, InputTag::Used);
                let input = test.inputs.get(5).unwrap();
                assert_eq!(input.good, 2);
                assert_eq!(input.tag, InputTag::Used);

                let input = test.inputs.get(6).unwrap();
                assert_eq!(input.good, 0);
                assert_eq!(input.tag, InputTag::Consumed);
                let input = test.inputs.get(7).unwrap();
                assert_eq!(input.good, 1);
                assert_eq!(input.tag, InputTag::Consumed);
                let input = test.inputs.get(8).unwrap();
                assert_eq!(input.good, 2);
                assert_eq!(input.tag, InputTag::Consumed);

                let test = test.uses_input(ProcessInput::new(5, 10.0));
                let input = test.inputs.get(9).unwrap();
                assert_eq!(input.good, 2);
                assert_eq!(input.tag, InputTag::Consumed);

                let input = test.inputs.get(3).unwrap();
                assert_eq!(input.good, 5);
                assert_eq!(input.tag, InputTag::None);
            }
        }

        mod do_process_should {
            use std::collections::HashMap;

            use crate::{data::Data, good::Good, item::Item, markethistory::{GoodRecord, MarketHistory}, process::{InputTag, OutputTag, Process, ProcessInput, ProcessOutput}, want::Want};

            #[test]
            pub fn run_simple_full_process_correctly() {
                let test = Process::new(0, String::from("test"), String::new())
                    .uses_input(ProcessInput::new(0, 1.0))
                    .uses_input(ProcessInput::new(1, 1.0).with_tag(InputTag::Consumed))
                    .uses_input(ProcessInput::new(2, 1.0).with_tag(InputTag::Used))
                    .uses_input(ProcessInput::new(0, 1.0).with_tag(InputTag::Used))
                    .has_output(ProcessOutput::new(Item::Want(5), 3.0))
                    .has_output(ProcessOutput::new(Item::Good(3), 2.0));

                let mut data = Data::new();
                // Initial goods
                data.goods.insert(0, Good::new(0, "0".to_string(), String::new())
                    .decays_to(10, 2.0));
                data.goods.insert(1, Good::new(1, "1".to_string(), String::new())
                    .decays_to(11, 2.0));
                data.goods.insert(2, Good::new(2, "2".to_string(), String::new())
                    .decays_to(12, 2.0));
                data.goods.insert(3, Good::new(3, "3".to_string(), String::new())
                    .decays_to(13, 2.0));
                // Resulting goods, just in case.
                data.goods.insert(10, Good::new(10, "10".to_string(), String::new()));
                data.goods.insert(11, Good::new(11, "11".to_string(), String::new()));
                data.goods.insert(12, Good::new(12, "12".to_string(), String::new()));
                data.goods.insert(13, Good::new(13, "13".to_string(), String::new()));
                // And our output want.
                data.wants.insert(5, Want::new(5, "5".to_string()));

                let mut availables = HashMap::new();
                availables.insert(0, 10.0);
                availables.insert(1, 10.0);
                availables.insert(2, 10.0);
                availables.insert(3, 10.0);
                availables.insert(10, 10.0);
                availables.insert(11, 10.0);
                availables.insert(12, 10.0);
                availables.insert(13, 10.0);

                let market_history = MarketHistory::new()
                    .with_good_record(0, GoodRecord::new().with_price(1.0))
                    .with_good_record(1, GoodRecord::new().with_price(1.0))
                    .with_good_record(2, GoodRecord::new().with_price(1.0))
                    .with_good_record(3, GoodRecord::new().with_price(1.0))
                    .with_good_record(10, GoodRecord::new().with_price(1.0))
                    .with_good_record(11, GoodRecord::new().with_price(1.0))
                    .with_good_record(12, GoodRecord::new().with_price(1.0))
                    .with_good_record(13, GoodRecord::new().with_price(1.0));
                
                let result = test.do_process(&availables, &data, 1.0, &market_history);

                assert_eq!(result.iterations, 1.0);
                assert_eq!(result.consumed.len(), 2);
                assert_eq!(*result.consumed.get(&0).unwrap(), 1.0);
                assert_eq!(*result.consumed.get(&1).unwrap(), 1.0);
                println!("Used");
                for (good, amt) in result.used.iter() {
                    println!("Good {}: {}", good, amt);
                }
                assert_eq!(result.used.len(), 2);
                assert_eq!(*result.used.get(&0).unwrap(), 1.0);
                assert_eq!(*result.used.get(&2).unwrap(), 1.0);
                assert_eq!(result.created.len(), 3);
                assert_eq!(*result.created.get(&Item::Good(11)).unwrap(), 2.0);
                assert_eq!(*result.created.get(&Item::Good(3)).unwrap(), 2.0);
                assert_eq!(*result.created.get(&Item::Want(5)).unwrap(), 3.0);
            }

            #[test]
            pub fn run_full_process_with_optionals_correctly() {
                let test = Process::new(0, String::from("test"), String::new())
                    .uses_input(ProcessInput::new(0, 1.0))
                    .uses_input(ProcessInput::new(0, 1.0).with_tag(InputTag::Used))
                    .uses_input(ProcessInput::new(2, 1.0).with_tag(InputTag::Used))
                    .uses_input(ProcessInput::new(1, 1.0).with_tag(InputTag::Consumed))
                    .has_output(ProcessOutput::new(Item::Want(5), 3.0))
                    .has_output(ProcessOutput::new(Item::Good(3), 2.0))
                    .with_optionals(1.5);

                let mut data = Data::new();
                // Initial goods
                data.goods.insert(0, Good::new(0, "0".to_string(), String::new())
                    .decays_to(10, 2.0));
                data.goods.insert(1, Good::new(1, "1".to_string(), String::new())
                    .decays_to(11, 2.0));
                data.goods.insert(2, Good::new(2, "2".to_string(), String::new())
                    .decays_to(12, 2.0));
                data.goods.insert(3, Good::new(3, "3".to_string(), String::new())
                    .decays_to(13, 2.0));
                // Resulting goods, just in case.
                data.goods.insert(10, Good::new(10, "10".to_string(), String::new()));
                data.goods.insert(11, Good::new(11, "11".to_string(), String::new()));
                data.goods.insert(12, Good::new(12, "12".to_string(), String::new()));
                data.goods.insert(13, Good::new(13, "13".to_string(), String::new()));
                // And our output want.
                data.wants.insert(5, Want::new(5, "5".to_string()));

                let mut availables = HashMap::new();
                availables.insert(0, 10.0);
                availables.insert(1, 10.0);
                availables.insert(2, 10.0);
                availables.insert(3, 10.0);
                availables.insert(10, 10.0);
                availables.insert(11, 10.0);
                availables.insert(12, 10.0);
                availables.insert(13, 10.0);

                let market_history = MarketHistory::new()
                    .with_good_record(0, GoodRecord::new().with_price(1.0))
                    .with_good_record(1, GoodRecord::new().with_price(2.0))
                    .with_good_record(2, GoodRecord::new().with_price(3.0))
                    .with_good_record(3, GoodRecord::new().with_price(4.0))
                    .with_good_record(10, GoodRecord::new().with_price(5.0))
                    .with_good_record(11, GoodRecord::new().with_price(6.0))
                    .with_good_record(12, GoodRecord::new().with_price(7.0))
                    .with_good_record(13, GoodRecord::new().with_price(8.0));
                
                let result = test.do_process(&availables, &data, 1.0, &market_history);

                // The most expensive items should be removed first.
                assert_eq!(result.iterations, 1.0);
                assert_eq!(result.consumed.len(), 2);
                assert_eq!(*result.consumed.get(&0).unwrap(), 1.0);
                assert_eq!(*result.consumed.get(&1).unwrap(), 0.5);
                assert_eq!(result.used.len(), 2);
                assert_eq!(*result.used.get(&0).unwrap(), 1.0);
                assert_eq!(*result.used.get(&2).unwrap(), 0.0);
                assert_eq!(result.created.len(), 3);
                assert_eq!(*result.created.get(&Item::Good(11)).unwrap(), 1.0);
                assert_eq!(*result.created.get(&Item::Good(3)).unwrap(), 2.0);
                assert_eq!(*result.created.get(&Item::Want(5)).unwrap(), 3.0);
            }

            #[test]
            pub fn run_partial_process_with_optionals_correctly() {
                let test = Process::new(0, String::from("test"), String::new())
                    .uses_input(ProcessInput::new(0, 1.0))
                    .uses_input(ProcessInput::new(0, 1.0).with_tag(InputTag::Used))
                    .uses_input(ProcessInput::new(2, 1.0).with_tag(InputTag::Used))
                    .uses_input(ProcessInput::new(1, 1.0).with_tag(InputTag::Consumed))
                    .has_output(ProcessOutput::new(Item::Want(5), 3.0))
                    .has_output(ProcessOutput::new(Item::Good(3), 2.0))
                    .with_optionals(1.5);

                let mut data = Data::new();
                // Initial goods
                data.goods.insert(0, Good::new(0, "0".to_string(), String::new())
                    .decays_to(10, 2.0));
                data.goods.insert(1, Good::new(1, "1".to_string(), String::new())
                    .decays_to(11, 2.0));
                data.goods.insert(2, Good::new(2, "2".to_string(), String::new())
                    .decays_to(12, 2.0));
                data.goods.insert(3, Good::new(3, "3".to_string(), String::new())
                    .decays_to(13, 2.0));
                // Resulting goods, just in case.
                data.goods.insert(10, Good::new(10, "10".to_string(), String::new()));
                data.goods.insert(11, Good::new(11, "11".to_string(), String::new()));
                data.goods.insert(12, Good::new(12, "12".to_string(), String::new()));
                data.goods.insert(13, Good::new(13, "13".to_string(), String::new()));
                // And our output want.
                data.wants.insert(5, Want::new(5, "5".to_string()));

                let mut availables = HashMap::new();
                availables.insert(0, 10.0);
                availables.insert(1, 10.0);
                availables.insert(2, 10.0);
                availables.insert(3, 10.0);
                availables.insert(10, 10.0);
                availables.insert(11, 10.0);
                availables.insert(12, 10.0);
                availables.insert(13, 10.0);

                let market_history = MarketHistory::new()
                    .with_good_record(0, GoodRecord::new().with_price(1.0))
                    .with_good_record(1, GoodRecord::new().with_price(2.0))
                    .with_good_record(2, GoodRecord::new().with_price(3.0))
                    .with_good_record(3, GoodRecord::new().with_price(4.0))
                    .with_good_record(10, GoodRecord::new().with_price(5.0))
                    .with_good_record(11, GoodRecord::new().with_price(6.0))
                    .with_good_record(12, GoodRecord::new().with_price(7.0))
                    .with_good_record(13, GoodRecord::new().with_price(8.0));
                
                let result = test.do_process(&availables, &data, 20.0, &market_history);

                // The most expensive items should be removed first.
                assert_eq!(result.iterations, 12.0);
                assert_eq!(result.consumed.len(), 2);
                assert_eq!(*result.consumed.get(&0).unwrap(), 10.0);
                assert_eq!(*result.consumed.get(&1).unwrap(), 10.0);
                assert_eq!(result.used.len(), 2);
                assert_eq!(*result.used.get(&0).unwrap(), 0.0);
                assert_eq!(*result.used.get(&2).unwrap(), 10.0);
                assert_eq!(result.created.len(), 3);
                assert_eq!(*result.created.get(&Item::Good(11)).unwrap(), 20.0);
                assert_eq!(*result.created.get(&Item::Good(3)).unwrap(), 24.0);
                assert_eq!(*result.created.get(&Item::Want(5)).unwrap(), 36.0);
            }

            #[test]
            pub fn run_partial_process_with_optionals_and_large_excess() {
                let test = Process::new(0, String::from("test"), String::new())
                    .uses_input(ProcessInput::new(0, 1.0))
                    .uses_input(ProcessInput::new(0, 1.0).with_tag(InputTag::Used))
                    .uses_input(ProcessInput::new(2, 1.0).with_tag(InputTag::Used))
                    .uses_input(ProcessInput::new(1, 1.0).with_tag(InputTag::Consumed))
                    .has_output(ProcessOutput::new(Item::Want(5), 3.0))
                    .has_output(ProcessOutput::new(Item::Good(3), 2.0))
                    .with_optionals(1.5);

                let mut data = Data::new();
                // Initial goods
                data.goods.insert(0, Good::new(0, "0".to_string(), String::new())
                    .decays_to(10, 2.0));
                data.goods.insert(1, Good::new(1, "1".to_string(), String::new())
                    .decays_to(11, 2.0));
                data.goods.insert(2, Good::new(2, "2".to_string(), String::new())
                    .decays_to(12, 2.0));
                data.goods.insert(3, Good::new(3, "3".to_string(), String::new())
                    .decays_to(13, 2.0));
                // Resulting goods, just in case.
                data.goods.insert(10, Good::new(10, "10".to_string(), String::new()));
                data.goods.insert(11, Good::new(11, "11".to_string(), String::new()));
                data.goods.insert(12, Good::new(12, "12".to_string(), String::new()));
                data.goods.insert(13, Good::new(13, "13".to_string(), String::new()));
                // And our output want.
                data.wants.insert(5, Want::new(5, "5".to_string()));

                let mut availables = HashMap::new();
                availables.insert(0, 100.0);
                availables.insert(1, 4.0);
                availables.insert(2, 4.0);
                availables.insert(3, 100.0);
                availables.insert(10, 100.0);
                availables.insert(11, 100.0);
                availables.insert(12, 100.0);
                availables.insert(13, 100.0);

                let market_history = MarketHistory::new()
                    .with_good_record(0, GoodRecord::new().with_price(1.0))
                    .with_good_record(1, GoodRecord::new().with_price(2.0))
                    .with_good_record(2, GoodRecord::new().with_price(3.0))
                    .with_good_record(3, GoodRecord::new().with_price(4.0))
                    .with_good_record(10, GoodRecord::new().with_price(5.0))
                    .with_good_record(11, GoodRecord::new().with_price(6.0))
                    .with_good_record(12, GoodRecord::new().with_price(7.0))
                    .with_good_record(13, GoodRecord::new().with_price(8.0));
                
                let result = test.do_process(&availables, &data, 20.0, &market_history);

                // The most expensive items should be removed first.
                assert_eq!(result.iterations, 16.0);
                assert_eq!(result.consumed.len(), 2);
                assert_eq!(*result.consumed.get(&0).unwrap(), 16.0);
                assert_eq!(*result.consumed.get(&1).unwrap(), 4.0);
                assert_eq!(result.used.len(), 2);
                assert_eq!(*result.used.get(&0).unwrap(), 16.0);
                assert_eq!(*result.used.get(&2).unwrap(), 4.0);
                assert_eq!(result.created.len(), 3);
                assert_eq!(*result.created.get(&Item::Good(11)).unwrap(), 8.0);
                assert_eq!(*result.created.get(&Item::Good(3)).unwrap(), 32.0);
                assert_eq!(*result.created.get(&Item::Want(5)).unwrap(), 48.0);
            }

            #[test]
            pub fn run_partial_process_with_optionals_and_unable_to_run_with_one_leg() {
                let test = Process::new(0, String::from("test"), String::new())
                    .uses_input(ProcessInput::new(0, 1.0))
                    .uses_input(ProcessInput::new(0, 1.0).with_tag(InputTag::Used))
                    .uses_input(ProcessInput::new(2, 1.0).with_tag(InputTag::Used))
                    .uses_input(ProcessInput::new(1, 1.0).with_tag(InputTag::Consumed))
                    .has_output(ProcessOutput::new(Item::Want(5), 3.0))
                    .has_output(ProcessOutput::new(Item::Good(3), 2.0))
                    .with_optionals(1.5);

                let mut data = Data::new();
                // Initial goods
                data.goods.insert(0, Good::new(0, "0".to_string(), String::new())
                    .decays_to(10, 2.0));
                data.goods.insert(1, Good::new(1, "1".to_string(), String::new())
                    .decays_to(11, 2.0));
                data.goods.insert(2, Good::new(2, "2".to_string(), String::new())
                    .decays_to(12, 2.0));
                data.goods.insert(3, Good::new(3, "3".to_string(), String::new())
                    .decays_to(13, 2.0));
                // Resulting goods, just in case.
                data.goods.insert(10, Good::new(10, "10".to_string(), String::new()));
                data.goods.insert(11, Good::new(11, "11".to_string(), String::new()));
                data.goods.insert(12, Good::new(12, "12".to_string(), String::new()));
                data.goods.insert(13, Good::new(13, "13".to_string(), String::new()));
                // And our output want.
                data.wants.insert(5, Want::new(5, "5".to_string()));

                let mut availables = HashMap::new();
                availables.insert(0, 100.0);
                availables.insert(1, 0.0);
                availables.insert(2, 0.0);
                availables.insert(3, 100.0);
                availables.insert(10, 100.0);
                availables.insert(11, 100.0);
                availables.insert(12, 100.0);
                availables.insert(13, 100.0);

                let market_history = MarketHistory::new()
                    .with_good_record(0, GoodRecord::new().with_price(1.0))
                    .with_good_record(1, GoodRecord::new().with_price(2.0))
                    .with_good_record(2, GoodRecord::new().with_price(3.0))
                    .with_good_record(3, GoodRecord::new().with_price(4.0))
                    .with_good_record(10, GoodRecord::new().with_price(5.0))
                    .with_good_record(11, GoodRecord::new().with_price(6.0))
                    .with_good_record(12, GoodRecord::new().with_price(7.0))
                    .with_good_record(13, GoodRecord::new().with_price(8.0));
                
                let result = test.do_process(&availables, &data, 5.0, &market_history);

                // The most expensive items should be removed first.
                assert_eq!(result.iterations, 0.0);
                assert_eq!(result.consumed.len(), 0);
                assert_eq!(result.used.len(), 0);
                assert_eq!(result.created.len(), 0);
            }

            #[test]
            pub fn run_partial_process_with_optionals_and_unable_to_run_with_multiple_leg() {
                let test = Process::new(0, String::from("test"), String::new())
                    .uses_input(ProcessInput::new(0, 1.0))
                    .uses_input(ProcessInput::new(0, 1.0).with_tag(InputTag::Used))
                    .uses_input(ProcessInput::new(2, 1.0).with_tag(InputTag::Used))
                    .uses_input(ProcessInput::new(1, 1.0).with_tag(InputTag::Consumed))
                    .has_output(ProcessOutput::new(Item::Want(5), 3.0))
                    .has_output(ProcessOutput::new(Item::Good(3), 2.0))
                    .with_optionals(0.5);

                let mut data = Data::new();
                // Initial goods
                data.goods.insert(0, Good::new(0, "0".to_string(), String::new())
                    .decays_to(10, 2.0));
                data.goods.insert(1, Good::new(1, "1".to_string(), String::new())
                    .decays_to(11, 2.0));
                data.goods.insert(2, Good::new(2, "2".to_string(), String::new())
                    .decays_to(12, 2.0));
                data.goods.insert(3, Good::new(3, "3".to_string(), String::new())
                    .decays_to(13, 2.0));
                // Resulting goods, just in case.
                data.goods.insert(10, Good::new(10, "10".to_string(), String::new()));
                data.goods.insert(11, Good::new(11, "11".to_string(), String::new()));
                data.goods.insert(12, Good::new(12, "12".to_string(), String::new()));
                data.goods.insert(13, Good::new(13, "13".to_string(), String::new()));
                // And our output want.
                data.wants.insert(5, Want::new(5, "5".to_string()));

                let mut availables = HashMap::new();
                availables.insert(0, 100.0);
                availables.insert(1, 3.0);
                availables.insert(2, 0.0);
                availables.insert(3, 100.0);
                availables.insert(10, 100.0);
                availables.insert(11, 100.0);
                availables.insert(12, 100.0);
                availables.insert(13, 100.0);

                let market_history = MarketHistory::new()
                    .with_good_record(0, GoodRecord::new().with_price(1.0))
                    .with_good_record(1, GoodRecord::new().with_price(2.0))
                    .with_good_record(2, GoodRecord::new().with_price(3.0))
                    .with_good_record(3, GoodRecord::new().with_price(4.0))
                    .with_good_record(10, GoodRecord::new().with_price(5.0))
                    .with_good_record(11, GoodRecord::new().with_price(6.0))
                    .with_good_record(12, GoodRecord::new().with_price(7.0))
                    .with_good_record(13, GoodRecord::new().with_price(8.0));
                
                let result = test.do_process(&availables, &data, 5.0, &market_history);

                // The most expensive items should be removed first.
                assert_eq!(result.iterations, 0.0);
                assert_eq!(result.consumed.len(), 0);
                assert_eq!(result.used.len(), 0);
                assert_eq!(result.created.len(), 0);
            }
        
            
            #[test]
            pub fn run_partial_process_with_optionals_first_leg_solution() {
                let test = Process::new(0, String::from("test"), String::new())
                    .uses_input(ProcessInput::new(0, 1.0))
                    .uses_input(ProcessInput::new(0, 1.0).with_tag(InputTag::Used))
                    .uses_input(ProcessInput::new(2, 1.0).with_tag(InputTag::Used))
                    .uses_input(ProcessInput::new(1, 1.0).with_tag(InputTag::Consumed))
                    .has_output(ProcessOutput::new(Item::Want(5), 3.0))
                    .has_output(ProcessOutput::new(Item::Good(3), 2.0))
                    .with_optionals(1.0);

                let mut data = Data::new();
                // Initial goods
                data.goods.insert(0, Good::new(0, "0".to_string(), String::new())
                    .decays_to(10, 2.0));
                data.goods.insert(1, Good::new(1, "1".to_string(), String::new())
                    .decays_to(11, 2.0));
                data.goods.insert(2, Good::new(2, "2".to_string(), String::new())
                    .decays_to(12, 2.0));
                data.goods.insert(3, Good::new(3, "3".to_string(), String::new())
                    .decays_to(13, 2.0));
                // Resulting goods, just in case.
                data.goods.insert(10, Good::new(10, "10".to_string(), String::new()));
                data.goods.insert(11, Good::new(11, "11".to_string(), String::new()));
                data.goods.insert(12, Good::new(12, "12".to_string(), String::new()));
                data.goods.insert(13, Good::new(13, "13".to_string(), String::new()));
                // And our output want.
                data.wants.insert(5, Want::new(5, "5".to_string()));

                let mut availables = HashMap::new();
                availables.insert(0, 100.0);
                availables.insert(1, 4.0);
                availables.insert(2, 0.0);
                availables.insert(3, 100.0);
                availables.insert(10, 100.0);
                availables.insert(11, 100.0);
                availables.insert(12, 100.0);
                availables.insert(13, 100.0);

                let market_history = MarketHistory::new()
                    .with_good_record(0, GoodRecord::new().with_price(1.0))
                    .with_good_record(1, GoodRecord::new().with_price(2.0))
                    .with_good_record(2, GoodRecord::new().with_price(3.0))
                    .with_good_record(3, GoodRecord::new().with_price(4.0))
                    .with_good_record(10, GoodRecord::new().with_price(5.0))
                    .with_good_record(11, GoodRecord::new().with_price(6.0))
                    .with_good_record(12, GoodRecord::new().with_price(7.0))
                    .with_good_record(13, GoodRecord::new().with_price(8.0));
                
                let result = test.do_process(&availables, &data, 20.0, &market_history);

                // The most expensive items should be removed first.
                assert_eq!(result.iterations, 4.0);
                assert_eq!(result.consumed.len(), 2);
                assert_eq!(*result.consumed.get(&0).unwrap(), 4.0);
                assert_eq!(*result.consumed.get(&1).unwrap(), 4.0);
                assert_eq!(result.used.len(), 2);
                assert_eq!(*result.used.get(&0).unwrap(), 4.0);
                assert_eq!(*result.used.get(&2).unwrap(), 0.0);
                assert_eq!(result.created.len(), 3);
                assert_eq!(*result.created.get(&Item::Good(11)).unwrap(), 8.0);
                assert_eq!(*result.created.get(&Item::Good(3)).unwrap(), 8.0);
                assert_eq!(*result.created.get(&Item::Want(5)).unwrap(), 12.0);
            }
        }
    }

    mod desire_tests {
        mod next_step_should {
            use crate::{desire::Desire, item::Item};

            #[test]
            pub fn step_up_when_matching_current_step() {
                let test = Desire::new(Item::Class(0), 1.0, 1.0)
                    .with_interval(2.0, 0);

                let result = test.next_step(2.0).expect("Did not return correctly!");
                assert_eq!(result, 4.0);
            }
        }

        mod assertion_checks {
            use std::mem::discriminant;

            use crate::{desire::{Desire, DesireTag}, household::HouseholdMember, item::Item};

            #[test]
            #[should_panic(expected = "A Desire with the tag LifeNeed must have a finite number of steps.")]
            pub fn fail_when_lifeneed_tag_and_no_end() {
                Desire::new(Item::Good(0), 1.0, 1.0)
                .with_interval(2.0, 0)
                .with_tag(DesireTag::life_need(0.5));
            }

            #[test]
            #[should_panic(expected = "Desire has the LifeNeed tag. It must have a finite number of steps.")]
            pub fn fail_when_endless_interval_and_existing_lifeneed_tag() {
                Desire::new(Item::Good(0), 1.0, 1.0)
                .with_tag(DesireTag::life_need(0.5))
                .with_interval(2.0, 0);
            }

            #[test]
            #[should_panic(expected = "Same Tags, never safe.")]
            pub fn panic_with_duplicate_tags_put_in() {
                Desire::new(Item::Good(0), 1.0, 1.0)
                .with_tag(DesireTag::HouseholdNeed)
                .with_tag(DesireTag::HouseholdNeed);
            }

            #[test]
            #[should_panic(expected = "Household Need cannot be next to a HouseMemberNeed.")]
            pub fn panic_when_inserting_housememberneed_and_householdneed_exists() {
                Desire::new(Item::Good(0), 1.0, 1.0)
                .with_tag(DesireTag::HouseholdNeed)
                .with_tag(DesireTag::HouseMemberNeed(HouseholdMember::Adult));
            }
            
            #[test]
            #[should_panic(expected = "HouseMemberNeed cannot be next to a HouseholdNeed.")]
            pub fn panic_when_inserting_householdrneed_and_housememberneed_exists() {
                Desire::new(Item::Good(0), 1.0, 1.0)
                .with_tag(DesireTag::HouseMemberNeed(HouseholdMember::Adult))
                .with_tag(DesireTag::HouseholdNeed);
            }

            #[test]
            pub fn insert_tags_into_desire_sorted() {
                let test = Desire::new(Item::Good(0), 1.0, 1.0)
                .with_tag(DesireTag::HouseMemberNeed(HouseholdMember::Adult))
                .with_tag(DesireTag::life_need(0.5));

                // check ordering
                assert!(discriminant(test.tags.get(0).unwrap()) == discriminant(&DesireTag::LifeNeed(0.0)));
                assert!(discriminant(test.tags.get(1).unwrap()) == discriminant(&DesireTag::HouseMemberNeed(HouseholdMember::Adult)))
            }
        }
    }

    mod pop_tests {
        mod integrate_desires_should {
            use crate::{desire::{Desire, DesireTag}, drow::DRow, household::{Household, HouseholdMember}, item::Item, pop::Pop};

            #[test]
            pub fn correctly_integrate_desires() {
                let mut row = DRow::new(3.0, 0);
                row.household = Household::new(3.0, 3.0, 2.0, 1.0);

                let source_desires = vec![
                    Desire::new(Item::Good(0), 1.0, 0.3),
                    Desire::new(Item::Good(1), 1.0, 1.0)
                        .with_tag(DesireTag::HouseholdNeed),
                    Desire::new(Item::Good(2), 1.0, 1.0)
                        .with_tag(DesireTag::HouseMemberNeed(HouseholdMember::Adult)),
                    Desire::new(Item::Good(3), 1.0, 1.0)
                        .with_tag(DesireTag::HouseMemberNeed(HouseholdMember::Child)),
                    Desire::new(Item::Good(4), 1.0, 1.0)
                        .with_tag(DesireTag::HouseMemberNeed(HouseholdMember::Elder)),
                ];

                let mut desires: Vec<Desire> = vec![];

                Pop::integrate_desires(&source_desires, &row, &mut desires);
                // check that initials were added in correctly.
                assert_eq!(desires.len(), 5);
                assert_eq!(desires.get(0).unwrap().start, 0.3);
                assert_eq!(desires.get(0).unwrap().amount, 18.0);
                assert_eq!(desires.get(1).unwrap().start, 1.0);
                assert_eq!(desires.get(1).unwrap().amount, 3.0);
                assert_eq!(desires.get(2).unwrap().start, 1.0);
                assert_eq!(desires.get(2).unwrap().amount, 9.0);
                assert_eq!(desires.get(3).unwrap().start, 1.0);
                assert_eq!(desires.get(3).unwrap().amount, 6.0);
                assert_eq!(desires.get(4).unwrap().start, 1.0);
                assert_eq!(desires.get(4).unwrap().amount, 3.0);

                let source_desires = vec![
                    Desire::new(Item::Good(0), 1.0, 0.3), // duplicate, combines with 0
                    Desire::new(Item::Good(1), 1.0, 0.6)
                        .with_tag(DesireTag::HouseholdNeed), // inserted into 1
                    Desire::new(Item::Good(2), 1.0, 1.5) // inserted at end near duplicate
                        .with_tag(DesireTag::HouseMemberNeed(HouseholdMember::Adult)),
                ];

                Pop::integrate_desires(&source_desires, &row, &mut desires);

                assert_eq!(desires.len(), 7);
                assert_eq!(desires.get(0).unwrap().start, 0.3); // added to by 2nd
                assert_eq!(desires.get(0).unwrap().amount, 36.0);
                assert_eq!(desires.get(1).unwrap().start, 0.6); // inserted by 2nd
                assert_eq!(desires.get(1).unwrap().amount, 3.0);
                assert_eq!(desires.get(2).unwrap().start, 1.0);
                assert_eq!(desires.get(2).unwrap().amount, 3.0);
                assert_eq!(desires.get(3).unwrap().start, 1.0);
                assert_eq!(desires.get(3).unwrap().amount, 9.0);
                assert_eq!(desires.get(4).unwrap().start, 1.0);
                assert_eq!(desires.get(4).unwrap().amount, 6.0);
                assert_eq!(desires.get(5).unwrap().start, 1.0);
                assert_eq!(desires.get(5).unwrap().amount, 3.0);
                assert_eq!(desires.get(6).unwrap().start, 1.5); // last insertion.
                assert_eq!(desires.get(6).unwrap().amount, 9.0);
            }
        }

        mod get_desire_multiplier_should {
            use crate::{desire::{Desire, DesireTag}, drow::DRow, household::{Household, HouseholdMember}, item::Item, pop::Pop};

            #[test]
            pub fn calculate_multiplier_correctly() {
                let mut row = DRow::new(3.0, 0);
                row.household = Household::new(3.0, 3.0, 2.0, 1.0);

                let mut desire = Desire {
                    item: Item::Good(0),
                    amount: 1.0,
                    start: 1.0,
                    interval: None,
                    steps: None,
                    tags: vec![],
                    satisfaction: 0.0,
                };

                let mut new_des = desire.clone();
                // default, no tags should multiply by 6.0 (household) * 3.0 count
                Pop::get_desire_multiplier(&desire, &row, &mut new_des);
                assert_eq!(new_des.amount, 18.0);
                
                // householdNeed, 3.0 count.
                desire.tags.push(DesireTag::HouseholdNeed);
                new_des = desire.clone();
                Pop::get_desire_multiplier(&desire, &row, &mut new_des);
                assert_eq!(new_des.amount, 3.0);

                // Member Need, adult 9.0
                *desire.tags.get_mut(0).unwrap() = DesireTag::HouseMemberNeed(HouseholdMember::Adult);
                new_des = desire.clone();
                Pop::get_desire_multiplier(&desire, &row, &mut new_des);
                assert_eq!(new_des.amount, 9.0);

                // Member Need, child 6.0
                *desire.tags.get_mut(0).unwrap() = DesireTag::HouseMemberNeed(HouseholdMember::Child);
                new_des = desire.clone();
                Pop::get_desire_multiplier(&desire, &row, &mut new_des);
                assert_eq!(new_des.amount, 6.0);

                // Member Need, elder 3.0
                *desire.tags.get_mut(0).unwrap() = DesireTag::HouseMemberNeed(HouseholdMember::Elder);
                new_des = desire.clone();
                Pop::get_desire_multiplier(&desire, &row, &mut new_des);
                assert_eq!(new_des.amount, 3.0);
            }
        }
    }
}