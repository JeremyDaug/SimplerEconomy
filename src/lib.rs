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
pub mod constants;
pub mod offerresult;

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

            use crate::{data::Data, good::Good, item::Item, markethistory::{GoodRecord, MarketHistory}, process::{InputTag, Process, ProcessInput, ProcessOutput}, want::Want};

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
        mod current_priority_should {
            use crate::{desire::{Desire, PriorityFn}, item::Item};

            #[test]
            pub fn calculate_priority_update() {
                let mut test_linear = Desire::new(Item::Good(0), 1.0, 2.0, 
                    PriorityFn::linear(2.0))
                    .with_steps(0);
            }
        }

        mod end_should {
            use crate::{desire::{Desire, PriorityFn}, item::Item};

            #[test]
            pub fn correctly_calculate_end_value() {
                // Base (1) step
                let d = Desire::new(Item::Want(0), 1.0, 0.0,
                    PriorityFn::linear(1.0));
                assert_eq!(d.end(), Some(1.0));

                // Dictated ending step.
                let d = Desire::new(Item::Want(0), 1.0, 0.0,
                    PriorityFn::linear(1.0))
                    .with_steps(20);
                assert_eq!(d.end(), Some(20.0));

                // Unending
                let d = Desire::new(Item::Want(0), 1.0, 0.0,
                    PriorityFn::linear(1.0))
                    .with_steps(0);
                assert_eq!(d.end(), None);
            }
        }

        mod current_valuation_should {
            use crate::{desire::{Desire, PriorityFn}, item::Item};

            #[test]
            pub fn calculate_single_step_value_correctly() {
                let mut stepless = Desire::new(Item::Good(0), 2.0, 2.0, 
                    PriorityFn::linear(4.0 / 3.0))
                    .with_steps(0);
                let unit_len = 1.0 - 5.0 / 3.0;

                stepless.satisfaction = 1.0;
                // partial satisfaction
                let val = stepless.current_valuation();
                let steps = val.0;
                let value = val.1;
                assert_eq!(steps, 0.5);
                assert!((unit_len / 2.0) + 0.000000001 > value && 
                    value > (unit_len / 2.0) - 0.000000001);
                
                // full step satisfaction
                stepless.satisfaction = 2.0;
                let val = stepless.current_valuation();
                let steps = val.0;
                let value = val.1;
                assert_eq!(steps, 1.0);
                assert!((unit_len) + 0.000000001 > value && 
                    value > (unit_len) - 0.000000001);

                // extra step satisfaction
                stepless.satisfaction = 3.0;
                let val = stepless.current_valuation();
                let steps = val.0;
                let value = val.1;
                assert_eq!(steps, 1.5);
                assert!((unit_len * 1.5) + 0.000000001 > value && 
                    value > (unit_len * 1.5) - 0.000000001);

                // extra step satisfaction
                stepless.satisfaction = 4.5;
                let val = stepless.current_valuation();
                let steps = val.0;
                let value = val.1;
                assert_eq!(steps, 2.25);
                assert!((unit_len * 2.25) + 0.000000001 > value && 
                    value > (unit_len * 2.25) - 0.000000001);
                
                                // extra step satisfaction
                stepless.satisfaction = 6.0;
                let val = stepless.current_valuation();
                let steps = val.0;
                let value = val.1;
                assert_eq!(steps, 3.0);
                assert_eq!(value, unit_len * 3.0);
            }
        }

        mod expected_value_should {
            use crate::{desire::{Desire, PriorityFn}, item::Item};

            #[test]
            pub fn return_positive_and_correct_value_when_positive_satisfaction() {
                let mut test = Desire::new(Item::Good(0), 1.0, 1.0, 
                    PriorityFn::linear(4.0 / 3.0))
                    .with_steps(0);
                test.satisfaction = 2.0;
                let unit_len = 1.0 - 5.0 / 3.0;

                let result = test.expected_value(3.0);
                assert_eq!(result, unit_len * 3.0);
            }

            #[test]
            pub fn return_negative_and_correct_value_when_negative_satisfaction() {
                let mut test = Desire::new(Item::Good(0), 1.0, 1.0, 
                    PriorityFn::linear(4.0 / 3.0))
                    .with_steps(0);
                test.satisfaction = 6.0;
                let unit_len = 1.0 - 5.0 / 3.0;

                let result = test.expected_value(-3.0);
                assert_eq!(result, -unit_len * 3.0);
            }

            #[test]
            pub fn return_negative_and_correctly_capped_value_when_big_negative_satisfaction() {
                let mut test = Desire::new(Item::Good(0), 1.0, 1.0, 
                    PriorityFn::linear(4.0 / 3.0))
                    .with_steps(0);
                test.satisfaction = 3.0;
                let unit_len = 1.0 - 5.0 / 3.0;

                let result = test.expected_value(-4.0);
                assert_eq!(result, unit_len * -3.0);
            }

            #[test]
            pub fn return_positive_and_correctly_capped_value_when_big_positive_satisfaction() {
                let mut test = Desire::new(Item::Good(0), 1.0, 1.0, 
                    PriorityFn::linear(4.0 / 3.0))
                    .with_steps(3);
                test.satisfaction = 2.0;
                let unit_len = 1.0 - 5.0 / 3.0;

                let result = test.expected_value(1.0);
                assert_eq!(result, unit_len);
            }
        }

        mod assertion_checks {
            use std::mem::discriminant;

            use crate::{desire::{Desire, DesireTag, PriorityFn}, household::HouseholdMember, item::Item};

            #[test]
            #[should_panic(expected = "A Desire with the tag LifeNeed must have a finite number of steps.")]
            pub fn fail_when_lifeneed_tag_and_no_end() {
                Desire::new(Item::Good(0), 1.0, 1.0,
                    PriorityFn::linear(1.0))
                .with_steps(0)
                .with_tag(DesireTag::life_need(0.5));
            }

            #[test]
            #[should_panic(expected = "Desire has the LifeNeed tag. It must have a finite number of steps.")]
            pub fn fail_when_endless_interval_and_existing_lifeneed_tag() {
                Desire::new(Item::Good(0), 1.0, 1.0,
                    PriorityFn::linear(1.0))
                .with_tag(DesireTag::life_need(0.5))
                .with_steps(0);
            }

            #[test]
            #[should_panic(expected = "Same Tags, never safe.")]
            pub fn panic_with_duplicate_tags_put_in() {
                Desire::new(Item::Good(0), 1.0, 1.0,
                    PriorityFn::linear(1.0))
                .with_tag(DesireTag::HouseholdNeed)
                .with_tag(DesireTag::HouseholdNeed);
            }

            #[test]
            #[should_panic(expected = "Household Need cannot be next to a HouseMemberNeed.")]
            pub fn panic_when_inserting_housememberneed_and_householdneed_exists() {
                Desire::new(Item::Good(0), 1.0, 1.0,
                    PriorityFn::linear(1.0))
                .with_tag(DesireTag::HouseholdNeed)
                .with_tag(DesireTag::HouseMemberNeed(HouseholdMember::Adult));
            }
            
            #[test]
            #[should_panic(expected = "HouseMemberNeed cannot be next to a HouseholdNeed.")]
            pub fn panic_when_inserting_householdrneed_and_housememberneed_exists() {
                Desire::new(Item::Good(0), 1.0, 1.0,
                    PriorityFn::linear(1.0))
                .with_tag(DesireTag::HouseMemberNeed(HouseholdMember::Adult))
                .with_tag(DesireTag::HouseholdNeed);
            }

            #[test]
            pub fn insert_tags_into_desire_sorted() {
                let test = Desire::new(Item::Good(0), 1.0, 1.0,
                    PriorityFn::linear(1.0))
                .with_tag(DesireTag::HouseMemberNeed(HouseholdMember::Adult))
                .with_tag(DesireTag::life_need(0.5));

                // check ordering
                assert!(discriminant(test.tags.get(0).unwrap()) == discriminant(&DesireTag::LifeNeed(0.0)));
                assert!(discriminant(test.tags.get(1).unwrap()) == discriminant(&DesireTag::HouseMemberNeed(HouseholdMember::Adult)))
            }
        }
    }

    mod pop_tests {
        mod satisfaction_from_multiple_amvs_should {
            use crate::{data::Data, desire::{Desire, PriorityFn}, good::Good, item::Item, markethistory::{GoodRecord, MarketHistory}, pop::Pop};

            #[test]
            pub fn correctly_predict_gain_from_amv_complex() {
                //println!("Starting Function");
                // set up product data.
                let mut data = Data::new();
                data.goods.insert(2, Good::new(2, "2".to_string(), String::from("")));
                data.goods.insert(3, Good::new(3, "3".to_string(), String::from("")));
                data.goods.insert(4, Good::new(4, "4".to_string(), String::from("")));
                data.goods.insert(5, Good::new(5, "5".to_string(), String::from("")));
                data.goods.insert(6, Good::new(6, "6".to_string(), String::from("")));
                data.goods.insert(7, Good::new(7, "7".to_string(), String::from("")));

                // set up market data for goods.
                let mut market = MarketHistory::new();
                market.good_records.insert(2, GoodRecord::new().with_price(1.0));
                market.good_records.insert(3, GoodRecord::new().with_price(1.0));
                market.good_records.insert(4, GoodRecord::new().with_price(2.0));
                market.good_records.insert(5, GoodRecord::new().with_price(1.0));
                market.good_records.insert(6, GoodRecord::new().with_price(0.5));
                market.good_records.insert(7, GoodRecord::new().with_price(0.25));
                // set up pop with empty desires
                let mut test_pop = Pop::new(0, 0, 0);
                test_pop.desires.push_back(Desire::new(Item::Good(4), 1.0, 20.0,
                    PriorityFn::linear(1.0)));
                test_pop.desires.push_back(Desire::new(Item::Good(5), 0.5, 3.0,
                    PriorityFn::linear(1.0))
                    .with_steps(2));
                test_pop.desires.push_back(Desire::new(Item::Good(6), 1.0, 8.0,
                    PriorityFn::linear(1.0))
                    .with_steps(0));
                // then check the gain when given X amv
                let results = test_pop.satisfaction_from_multiple_amvs(vec![10.0, 10.0], &market);

                assert_eq!(results.len(), 3);

                let (levels, sat) = results.get(0).unwrap();
                assert_eq!(*levels, 17.0);
                assert!(*sat > 35.99 && *sat < 36.0);

                let (levels, sat) = results.get(1).unwrap();
                assert_eq!(*levels, 20.0);
                assert!(*sat > 35.99 && *sat < 36.0);

                let (levels, sat) = results.get(3).unwrap();
                assert_eq!(*levels, 37.0);
                assert!(*sat > 35.99 && *sat < 36.0);
            }
        }

        mod satisfaction_from_amv_should {
            use crate::{data::Data, desire::{Desire, PriorityFn}, good::Good, item::Item, markethistory::{GoodRecord, MarketHistory}, pop::Pop};

            #[test]
            pub fn correctly_predict_gain_from_amv_simple() {
                //println!("Starting Function");
                // set up product data.
                let mut data = Data::new();
                data.goods.insert(2, Good::new(2, "2".to_string(), String::from("")));
                data.goods.insert(3, Good::new(3, "3".to_string(), String::from("")));
                data.goods.insert(4, Good::new(4, "4".to_string(), String::from("")));
                data.goods.insert(5, Good::new(5, "5".to_string(), String::from("")));
                data.goods.insert(6, Good::new(6, "6".to_string(), String::from("")));
                data.goods.insert(7, Good::new(7, "7".to_string(), String::from("")));

                // set up market data for goods.
                let mut market = MarketHistory::new();
                market.good_records.insert(2, GoodRecord::new().with_price(1.0));
                market.good_records.insert(3, GoodRecord::new().with_price(1.0));
                market.good_records.insert(4, GoodRecord::new().with_price(1.0));
                market.good_records.insert(5, GoodRecord::new().with_price(1.0));
                market.good_records.insert(6, GoodRecord::new().with_price(0.5));
                market.good_records.insert(7, GoodRecord::new().with_price(0.25));
                // set up pop with empty desires
                let mut test_pop = Pop::new(0, 0, 0);
                test_pop.desires.push_back(Desire::new(Item::Good(5), 1.0, 1.0,
                    PriorityFn::linear(1.0))
                    .with_steps(0));
                // then check the gain when given X amv
                let (levels, sat) = test_pop.satisfaction_from_amv(4.0, &market);

                assert_eq!(levels, 4.0);
                assert_eq!(sat, 1.875);
            }

            #[test]
            pub fn correctly_predict_gain_from_amv_complex() {
                //println!("Starting Function");
                // set up product data.
                let mut data = Data::new();
                data.goods.insert(2, Good::new(2, "2".to_string(), String::from("")));
                data.goods.insert(3, Good::new(3, "3".to_string(), String::from("")));
                data.goods.insert(4, Good::new(4, "4".to_string(), String::from("")));
                data.goods.insert(5, Good::new(5, "5".to_string(), String::from("")));
                data.goods.insert(6, Good::new(6, "6".to_string(), String::from("")));
                data.goods.insert(7, Good::new(7, "7".to_string(), String::from("")));

                // set up market data for goods.
                let mut market = MarketHistory::new();
                market.good_records.insert(2, GoodRecord::new().with_price(1.0));
                market.good_records.insert(3, GoodRecord::new().with_price(1.0));
                market.good_records.insert(4, GoodRecord::new().with_price(2.0));
                market.good_records.insert(5, GoodRecord::new().with_price(1.0));
                market.good_records.insert(6, GoodRecord::new().with_price(0.5));
                market.good_records.insert(7, GoodRecord::new().with_price(0.25));
                // set up pop with empty desires
                let mut test_pop = Pop::new(0, 0, 0);
                test_pop.desires.push_back(Desire::new(Item::Good(4), 1.0, 20.0,
                    PriorityFn::linear(1.0)));
                test_pop.desires.push_back(Desire::new(Item::Good(5), 0.5, 3.0,
                    PriorityFn::linear(1.0))
                    .with_steps(2));
                test_pop.desires.push_back(Desire::new(Item::Good(6), 1.0, 8.0,
                    PriorityFn::linear(1.0))
                    .with_steps(0));
                // then check the gain when given X amv
                let (levels, sat) = test_pop.satisfaction_from_amv(20.0, &market);

                assert_eq!(levels, 37.0);
                assert!(sat > 35.99 && sat < 36.0);
            }
        }

        mod make_offer_should {
            use std::collections::HashMap;

            use crate::{data::Data, desire::{Desire, PriorityFn}, good::Good, item::Item, markethistory::{GoodRecord, MarketHistory}, pop::{Pop, PropertyRecord}};

            /// Tests when sat_lost == 0.0. This includes testing out money.
            #[test]
            pub fn correctly_make_offer_no_satisfaction_loss_and_money() {
                //println!("Starting Function");
                // set up product data.
                let mut data = Data::new();
                data.goods.insert(5, Good::new(5, "5".to_string(), String::from("")));
                data.goods.insert(6, Good::new(6, "6".to_string(), String::from("")));
                data.goods.insert(7, Good::new(7, "7".to_string(), String::from("")));

                // set up market data for goods.
                let mut market = MarketHistory::new();
                market.good_records.insert(5, GoodRecord::new().with_price(1.0));
                market.good_records.insert(6, GoodRecord::new().with_price(0.5));
                market.good_records.insert(7, GoodRecord::new().with_price(0.25));
                // set up currencies
                market.currencies.insert(6);
                market.currencies.insert(7);
                // set up the pop
                //println!("Making pop.");
                let mut test_pop = Pop::new(0, 0, 0);
                // set up desires, only one, good 5, no cap.
                test_pop.desires.push_back(Desire::new(Item::Good(5), 1.0, 1.0,
                    PriorityFn::linear(1.0))
                    .with_steps(0));
                // Add in 
                test_pop.property.insert(5, PropertyRecord::new(3.0));
                test_pop.property.insert(6, PropertyRecord::new(2.0));
                test_pop.property.insert(7, PropertyRecord::new(5.0));
                test_pop.satisfy_desires(&data);

                // setup what we want to buy.
                let mut request = HashMap::new();
                request.insert(5, 2.0);

                // setup the price hint (not used in this test)
                let mut price_hint: HashMap<usize, f64> = HashMap::new();
                price_hint.insert(6, 2.0);
                price_hint.insert(7, 4.0);

                // now, do the test
                //println!("Starting Function");
                let result = test_pop.make_offer(&request, &data, &market, &price_hint);

                assert_eq!(*result.get(&6).unwrap(), 2.0);
                assert_eq!(*result.get(&7).unwrap(), 4.0);
            }

            /// Tests when sat_lost > 0.0 but always < sat_gained
            #[test]
            pub fn correctly_make_offer_enough_money_not_satisfying_hint() {
                println!("Starting Function");
                // set up product data.
                let mut data = Data::new();
                data.goods.insert(5, Good::new(5, "5".to_string(), String::from("")));
                data.goods.insert(6, Good::new(6, "6".to_string(), String::from("")));
                data.goods.insert(7, Good::new(7, "7".to_string(), String::from("")));
                data.goods.insert(8, Good::new(8, "8".to_string(), String::from("")));

                // set up market data for goods.
                let mut market = MarketHistory::new();
                market.good_records.insert(5, GoodRecord::new().with_price(1.0));
                market.good_records.insert(6, GoodRecord::new().with_price(0.5));
                market.good_records.insert(7, GoodRecord::new().with_price(0.25));
                market.good_records.insert(8, GoodRecord::new().with_price(0.1));
                // set up currencies
                market.currencies.insert(6);
                market.currencies.insert(7);
                market.currencies.insert(8);
                // set up the pop
                println!("Making pop.");
                let mut test_pop = Pop::new(0, 0, 0);
                // set up desires, only one, good 5, no cap.
                test_pop.desires.push_back(Desire::new(Item::Good(5), 1.0, 1.0,
                    PriorityFn::linear(1.0))
                    .with_steps(0));
                // Add in 
                test_pop.property.insert(5, PropertyRecord::new(3.0));
                test_pop.property.insert(6, PropertyRecord::new(2.0));
                test_pop.property.insert(7, PropertyRecord::new(3.0));
                test_pop.property.insert(8, PropertyRecord::new(10.0));
                test_pop.satisfy_desires(&data);

                // setup what we want to buy.
                let mut request = HashMap::new();
                request.insert(5, 2.0);

                // setup the price hint (not used in this test)
                let mut price_hint: HashMap<usize, f64> = HashMap::new();
                price_hint.insert(6, 2.0);
                price_hint.insert(7, 4.0);

                // now, do the test
                println!("Starting Function");
                let result = test_pop.make_offer(&request, &data, &market, &price_hint);

                assert_eq!(*result.get(&6).unwrap(), 2.0);
                assert_eq!(*result.get(&7).unwrap(), 3.0);
                assert_eq!(*result.get(&8).unwrap(), 3.0);
            }

            /// Tests when sat_loss > 0.0, and some steps result in loss > sat_gained
            #[test]
            pub fn correctly_make_offer_no_hint_just_money() {
                println!("Starting Function");
                // set up product data.
                let mut data = Data::new();
                data.goods.insert(5, Good::new(5, "5".to_string(), String::from("")));
                data.goods.insert(6, Good::new(6, "6".to_string(), String::from("")));
                data.goods.insert(7, Good::new(7, "7".to_string(), String::from("")));
                data.goods.insert(8, Good::new(8, "8".to_string(), String::from("")));

                // set up market data for goods.
                let mut market = MarketHistory::new();
                market.good_records.insert(5, GoodRecord::new().with_price(1.0));
                market.good_records.insert(6, GoodRecord::new().with_price(0.5));
                market.good_records.insert(7, GoodRecord::new().with_price(0.25));
                market.good_records.insert(8, GoodRecord::new().with_price(0.1));
                // set up currencies
                market.currencies.insert(6);
                market.currencies.insert(7);
                market.currencies.insert(8);
                // set up the pop
                println!("Making pop.");
                let mut test_pop = Pop::new(0, 0, 0);
                // set up desires, only one, good 5, no cap.
                test_pop.desires.push_back(Desire::new(Item::Good(5), 1.0, 1.0,
                    PriorityFn::linear(1.0))
                    .with_steps(0));
                // Add in 
                test_pop.property.insert(5, PropertyRecord::new(3.0));
                test_pop.property.insert(6, PropertyRecord::new(2.0));
                test_pop.property.insert(7, PropertyRecord::new(3.0));
                test_pop.property.insert(8, PropertyRecord::new(10.0));
                test_pop.satisfy_desires(&data);

                // setup what we want to buy.
                let mut request = HashMap::new();
                request.insert(5, 2.0);

                // setup the price hint (not used in this test)
                let price_hint: HashMap<usize, f64> = HashMap::new();

                // now, do the test
                println!("Starting Function");
                let result = test_pop.make_offer(&request, &data, &market, &price_hint);

                assert_eq!(*result.get(&6).unwrap(), 2.0);
                assert_eq!(*result.get(&7).unwrap(), 3.0);
                assert_eq!(*result.get(&8).unwrap(), 3.0);
            }

            #[test]
            pub fn correctly_make_offer_no_hint_no_money_just_barter() {
                println!("Starting Function");
                // set up product data.
                let mut data = Data::new();
                data.goods.insert(5, Good::new(5, "5".to_string(), String::from("")));
                data.goods.insert(6, Good::new(6, "6".to_string(), String::from("")));
                data.goods.insert(7, Good::new(7, "7".to_string(), String::from("")));
                data.goods.insert(8, Good::new(8, "8".to_string(), String::from("")));

                // set up market data for goods.
                let mut market = MarketHistory::new();
                market.good_records.insert(5, GoodRecord::new().with_price(1.0));
                market.good_records.insert(6, GoodRecord::new().with_price(0.5));
                market.good_records.insert(7, GoodRecord::new().with_price(0.25));
                market.good_records.insert(8, GoodRecord::new().with_price(0.1));
                // set up currencies
                market.currencies.insert(6);
                market.currencies.insert(7);
                // set up the pop
                println!("Making pop.");
                let mut test_pop = Pop::new(0, 0, 0);
                // set up desires, only one, good 5, no cap.
                test_pop.desires.push_back(Desire::new(Item::Good(5), 1.0, 1.0,
                    PriorityFn::linear(1.0))
                    .with_steps(0));
                // Add in 
                test_pop.property.insert(5, PropertyRecord::new(3.0));
                //test_pop.property.insert(6, PropertyRecord::new(2.0));
                //test_pop.property.insert(7, PropertyRecord::new(3.0));
                test_pop.property.insert(8, PropertyRecord::new(100.0));
                test_pop.satisfy_desires(&data);

                // setup what we want to buy.
                let mut request = HashMap::new();
                request.insert(5, 2.0);

                // setup the price hint (not used in this test)
                let price_hint: HashMap<usize, f64> = HashMap::new();

                // now, do the test
                println!("Starting Function");
                let result = test_pop.make_offer(&request, &data, &market, &price_hint);

                assert_eq!(*result.get(&8).unwrap(), 20.0);
            }
        }

        mod satisfaction_gain_should {
            use std::collections::HashMap;

            use crate::{data::Data, desire::{Desire, PriorityFn}, good::Good, item::Item, pop::{Pop, PropertyRecord}};

            #[test]
            pub fn correctly_return_satisfaction_gained() {
                // set up product data.
                let mut data = Data::new();
                data.goods.insert(5, Good::new(5, "5".to_string(), String::from("")));
                data.goods.insert(6, Good::new(6, "6".to_string(), String::from("")));
                data.goods.insert(7, Good::new(7, "7".to_string(), String::from("")));

                let mut test_pop = Pop::new(0, 0, 0);
                // set up desires, only one, good 5, no cap.
                test_pop.desires.push_back(Desire::new(Item::Good(5), 1.0, 1.0,
                    PriorityFn::linear(1.0))
                    .with_steps(0));
                // Add in 
                test_pop.property.insert(5, PropertyRecord::new(1.0));
                test_pop.satisfy_desires(&data);

                let mut new_goods = HashMap::new();
                new_goods.insert(5, 1.0);
                let result = test_pop.satisfaction_gain(&new_goods, &data);

                assert_eq!(result.0, 1.0);
                assert_eq!(result.1, 0.5);
            }
        }

        mod integrate_desires_should {
            use crate::{desire::{Desire, DesireTag, PriorityFn}, drow::DRow, household::{Household, HouseholdMember}, item::Item, pop::Pop};

            #[test]
            pub fn correctly_integrate_desires() {
                let mut row = DRow::new(3.0, 0);
                row.household = Household::new(3.0, 3.0, 2.0, 1.0);

                let source_desires = vec![
                    Desire::new(Item::Good(0), 1.0, 0.3,
                        PriorityFn::linear(1.0)),
                    Desire::new(Item::Good(1), 1.0, 1.0,
                        PriorityFn::linear(1.0))
                        .with_tag(DesireTag::HouseholdNeed),
                    Desire::new(Item::Good(2), 1.0, 1.0,
                        PriorityFn::linear(1.0))
                        .with_tag(DesireTag::HouseMemberNeed(HouseholdMember::Adult)),
                    Desire::new(Item::Good(3), 1.0, 1.0,
                        PriorityFn::linear(1.0))
                        .with_tag(DesireTag::HouseMemberNeed(HouseholdMember::Child)),
                    Desire::new(Item::Good(4), 1.0, 1.0,
                        PriorityFn::linear(1.0))
                        .with_tag(DesireTag::HouseMemberNeed(HouseholdMember::Elder)),
                ];

                let mut desires: Vec<Desire> = vec![];

                Pop::integrate_desires(&source_desires, &row, &mut desires);
                // check that initials were added in correctly.
                assert_eq!(desires.len(), 5);
                assert_eq!(desires.get(0).unwrap().start_priority, 1.0);
                assert_eq!(desires.get(0).unwrap().amount, 3.0);
                assert_eq!(desires.get(0).unwrap().item, Item::Good(1));
                assert_eq!(desires.get(1).unwrap().start_priority, 1.0);
                assert_eq!(desires.get(1).unwrap().amount, 9.0);
                assert_eq!(desires.get(1).unwrap().item, Item::Good(2));
                assert_eq!(desires.get(2).unwrap().start_priority, 1.0);
                assert_eq!(desires.get(2).unwrap().amount, 6.0);
                assert_eq!(desires.get(2).unwrap().item, Item::Good(3));
                assert_eq!(desires.get(3).unwrap().start_priority, 1.0);
                assert_eq!(desires.get(3).unwrap().amount, 3.0);
                assert_eq!(desires.get(3).unwrap().item, Item::Good(4));
                assert_eq!(desires.get(4).unwrap().start_priority, 0.3);
                assert_eq!(desires.get(4).unwrap().amount, 18.0);
                assert_eq!(desires.get(4).unwrap().item, Item::Good(0));

                let source_desires = vec![
                    Desire::new(Item::Good(0), 1.0, 0.3,
                        PriorityFn::linear(1.0)), // duplicate, combines with 0
                    Desire::new(Item::Good(1), 1.0, 0.6,
                        PriorityFn::linear(1.0))
                        .with_tag(DesireTag::HouseholdNeed), // inserted into 1
                    Desire::new(Item::Good(2), 1.0, 1.5,
                        PriorityFn::linear(1.0)) // inserted at end near duplicate
                        .with_tag(DesireTag::HouseMemberNeed(HouseholdMember::Adult)),
                ];

                Pop::integrate_desires(&source_desires, &row, &mut desires);

                assert_eq!(desires.len(), 7);
                assert_eq!(desires.get(0).unwrap().start_priority, 1.5); // last insertion.
                assert_eq!(desires.get(0).unwrap().amount, 9.0);
                assert_eq!(desires.get(0).unwrap().item, Item::Good(2));
                assert_eq!(desires.get(1).unwrap().start_priority, 1.0);
                assert_eq!(desires.get(1).unwrap().amount, 3.0);
                assert_eq!(desires.get(1).unwrap().item, Item::Good(1));
                assert_eq!(desires.get(2).unwrap().start_priority, 1.0);
                assert_eq!(desires.get(2).unwrap().amount, 9.0);
                assert_eq!(desires.get(2).unwrap().item, Item::Good(2));
                assert_eq!(desires.get(3).unwrap().start_priority, 1.0);
                assert_eq!(desires.get(3).unwrap().amount, 6.0);
                assert_eq!(desires.get(3).unwrap().item, Item::Good(3));
                assert_eq!(desires.get(4).unwrap().start_priority, 1.0);
                assert_eq!(desires.get(4).unwrap().amount, 3.0);
                assert_eq!(desires.get(4).unwrap().item, Item::Good(4));
                assert_eq!(desires.get(5).unwrap().start_priority, 0.6); // inserted by 2nd
                assert_eq!(desires.get(5).unwrap().amount, 3.0);
                assert_eq!(desires.get(5).unwrap().item, Item::Good(1));
                assert_eq!(desires.get(6).unwrap().start_priority, 0.3); // added to by 2nd
                assert_eq!(desires.get(6).unwrap().amount, 36.0);
                assert_eq!(desires.get(6).unwrap().item, Item::Good(0));
            }
        }

        mod get_desire_multiplier_should {
            use crate::{desire::{Desire, DesireTag, PriorityFn}, drow::DRow, household::{Household, HouseholdMember}, item::Item, pop::Pop};

            #[test]
            pub fn calculate_multiplier_correctly() {
                let mut row = DRow::new(3.0, 0);
                row.household = Household::new(3.0, 3.0, 2.0, 1.0);

                let mut desire = Desire {
                    item: Item::Good(0),
                    amount: 1.0,
                    start_priority: 1.0,
                    priority_fn: PriorityFn::linear(1.0),
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
        
        mod satisfy_desires_should {
            use std::collections::HashMap;

            use crate::{data::Data, desire::{Desire, PriorityFn}, good::Good, item::Item, pop::{Pop, PropertyRecord, WantRecord}, want::Want};

            #[test]
            pub fn satisfy_good_correctly() {
                let mut data = Data::new();
                data.add_time();
                data.add_good(Good::new(4, String::from("testGood"), String::new()));

                let desire = Desire::new(Item::Good(4), 1.0, 1.0,
                        PriorityFn::linear(1.0))
                    .with_steps(0);

                let mut test = Pop::new(0, 0, 0);

                test.desires.push_back(desire);
                test.property.insert(4, PropertyRecord::new(100.0)); 

                test.satisfy_desires(&data);

                assert_eq!(test.desires.get(0).unwrap().satisfaction, 100.0);
                assert_eq!(test.property.get(&4).unwrap().reserved, 100.0);
            }

            #[test]
            pub fn satisfy_class_correctly() {
                let mut data = Data::new();
                data.add_time();
                data.add_good(Good::new(4, String::from("testGood1"), String::new())
                .in_class(4));
                data.add_good(Good::new(5, String::from("testGood2"), String::new())
                .in_class(4));

                let desire = Desire::new(Item::Class(4), 1.0, 1.0,
                        PriorityFn::linear(1.0))
                    .with_steps(0);

                let mut test = Pop::new(0, 0, 0);

                test.desires.push_back(desire);
                test.property.insert(4, PropertyRecord::new(10.0)); 
                test.property.insert(5, PropertyRecord::new(10.0)); 

                test.satisfy_desires(&data);

                assert_eq!(test.desires.get(0).unwrap().satisfaction, 20.0);
                assert_eq!(test.property.get(&4).unwrap().reserved, 10.0);
                assert_eq!(test.property.get(&5).unwrap().reserved, 10.0);
            }

            #[test]
            pub fn satisfy_want_correctly() {
                let mut data = Data::new();
                data.add_time();
                data.wants.insert(4, Want::new(4, String::from("testWant1")));
                data.wants.insert(5, Want::new(5, String::from("testWant2")));
                data.wants.insert(6, Want::new(6, String::from("testWant3")));
                let mut wants = HashMap::new();
                wants.insert(4, 1.0);
                wants.insert(5, 2.0);
                wants.insert(6, 0.5);
                data.add_good(Good::new(4, String::from("testGood1"), String::new())
                    .with_ownership(wants.clone()));
                data.add_good(Good::new(5, String::from("testGood2"), String::new())
                    .with_uses(2.0, wants.clone()));
                data.add_good(Good::new(6, String::from("testGood3"), String::new())
                    .with_consumption(1.0, wants.clone()));

                let desire = Desire::new(Item::Want(4), 10.0, 1.0,
                        PriorityFn::linear(1.0))
                    .with_steps(0);

                let mut test = Pop::new(0, 0, 0);

                test.desires.push_back(desire);
                test.wants.insert(4, WantRecord {
                    owned: 10.0,
                    reserved: 0.0,
                    expected: 0.0,
                    expended: 0.0,
                });
                test.property.insert(0, PropertyRecord::new(100.0)); 
                test.property.insert(4, PropertyRecord::new(10.0)); 
                test.property.insert(5, PropertyRecord::new(10.0)); 
                test.property.insert(6, PropertyRecord::new(10.0)); 

                test.satisfy_desires(&data);

                assert_eq!(test.property.get(&4).unwrap().reserved, 10.0);
                assert_eq!(test.property.get(&5).unwrap().reserved, 10.0);
                assert_eq!(test.property.get(&6).unwrap().reserved, 10.0);
                assert_eq!(test.wants.get(&4).unwrap().expected, 30.0);
                assert_eq!(test.wants.get(&4).unwrap().owned, 10.0);
                assert_eq!(test.wants.get(&4).unwrap().reserved, 40.0);
                assert_eq!(test.wants.get(&5).unwrap().expected, 60.0);
                assert_eq!(test.wants.get(&6).unwrap().expected, 15.0);
                assert_eq!(test.desires.get(0).unwrap().satisfaction, 40.0);
            }
        }

        mod satisfy_next_desire_should {
            use std::collections::{HashMap, VecDeque};

            use crate::{data::Data, desire::{Desire, PriorityFn}, good::Good, item::Item, pop::{Pop, PropertyRecord, WantRecord}, want::Want};

            #[test]
            pub fn satisfy_good_correctly() {
                let mut data = Data::new();
                data.add_time();
                data.add_good(Good::new(4, String::from("testGood"), String::new()));

                let desire = Desire::new(Item::Good(4), 2.0, 1.0,
                        PriorityFn::linear(1.0))
                    .with_steps(0);

                let mut test = Pop::new(0, 0, 0);

                test.property.insert(4, PropertyRecord::new(3.0)); 

                let mut working_desires = VecDeque::new();

                working_desires.push_front((desire.start_priority, desire));
                let result = test.satisfy_next_desire(&mut working_desires, &data);

                assert!(result.is_none());
                assert_eq!(working_desires.front().unwrap().0, 0.5);
                assert_eq!(working_desires.front().unwrap().1.satisfaction, 2.0);
                assert_eq!(test.property.get(&4).unwrap().reserved, 2.0);

                let result = test.satisfy_next_desire(&mut working_desires, &data);

                if let Some(result) = result {
                    assert_eq!(result.0, 0.5);
                    assert_eq!(result.1.satisfaction, 3.0);
                    assert_eq!(test.property.get(&4).unwrap().reserved, 3.0);
                } else {
                    assert!(false, "Did not return unsatisfied desire as expected.");
                }
            }

            #[test]
            pub fn satisfy_class_correctly() {
                let mut data = Data::new();
                data.add_time();
                data.add_good(Good::new(4, String::from("testGood1"), String::new())
                .in_class(4));
                data.add_good(Good::new(5, String::from("testGood2"), String::new())
                .in_class(4));

                let desire = Desire::new(Item::Class(4), 10.0, 1.0,
                        PriorityFn::linear(1.0))
                    .with_steps(0);

                let mut working_desires = VecDeque::new();
                working_desires.push_front((1.0, desire));

                let mut test = Pop::new(0, 0, 0);

                test.property.insert(4, PropertyRecord::new(10.0)); 
                test.property.insert(5, PropertyRecord::new(5.0)); 

                let result = test.satisfy_next_desire(&mut working_desires, &data);

                assert!(result.is_none());
                assert_eq!(test.property.get(&4).unwrap().reserved, 10.0);
                assert_eq!(test.property.get(&5).unwrap().reserved, 0.0);
                assert_eq!(working_desires.get(0).unwrap().0, 0.5);
                assert_eq!(working_desires.get(0).unwrap().1.satisfaction, 10.0);

                let result = test.satisfy_next_desire(&mut working_desires, &data);

                if let Some(result) = result {
                    assert_eq!(result.0, 0.5);
                    assert_eq!(result.1.satisfaction, 15.0);
                    assert_eq!(test.property.get(&4).unwrap().reserved, 10.0);
                    assert_eq!(test.property.get(&5).unwrap().reserved, 5.0);
                    assert_eq!(working_desires.len(), 0);
                } else {
                    assert!(false, "Did not return as expected.");
                }
            }

            #[test]
            pub fn satisfy_want_correctly() {
                let mut data = Data::new();
                data.add_time();
                data.wants.insert(4, Want::new(4, String::from("testWant1")));
                data.wants.insert(5, Want::new(5, String::from("testWant2")));
                data.wants.insert(6, Want::new(6, String::from("testWant3")));
                let mut wants = HashMap::new();
                wants.insert(4, 1.0);
                wants.insert(5, 2.0);
                wants.insert(6, 0.5);
                data.add_good(Good::new(4, String::from("testGood1"), String::new())
                    .with_ownership(wants.clone()));
                data.add_good(Good::new(5, String::from("testGood2"), String::new())
                    .with_uses(2.0, wants.clone()));
                data.add_good(Good::new(6, String::from("testGood3"), String::new())
                    .with_consumption(1.0, wants.clone()));

                let desire = Desire::new(Item::Want(4), 15.0, 1.0,
                        PriorityFn::linear(1.0))
                    .with_steps(0);

                let mut working_desires = VecDeque::new();
                working_desires.push_front((1.0, desire));

                let mut test = Pop::new(0, 0, 0);

                test.wants.insert(4, WantRecord {
                    owned: 10.0,
                    reserved: 0.0,
                    expected: 0.0,
                    expended: 0.0,
                });
                test.property.insert(0, PropertyRecord::new(100.0)); 
                test.property.insert(4, PropertyRecord::new(10.0)); 
                test.property.insert(5, PropertyRecord::new(10.0)); 
                test.property.insert(6, PropertyRecord::new(10.0)); 

                let result = test.satisfy_next_desire(&mut working_desires, &data);

                // first pass.
                assert!(result.is_none());
                assert_eq!(working_desires.get(0).unwrap().0, 0.5);
                assert_eq!(working_desires.get(0).unwrap().1.satisfaction, 15.0);
                assert_eq!(test.wants.get(&4).unwrap().reserved, 15.0);
                assert_eq!(test.wants.get(&4).unwrap().expected, 5.0);
                assert_eq!(test.property.get(&0).unwrap().reserved, 0.0);
                assert_eq!(test.property.get(&4).unwrap().reserved, 5.0);
                assert_eq!(test.property.get(&5).unwrap().reserved, 0.0);
                assert_eq!(test.property.get(&6).unwrap().reserved, 0.0);

                // Second pass
                let result = test.satisfy_next_desire(&mut working_desires, &data);
                assert!(result.is_none());
                assert_eq!(working_desires.get(0).unwrap().0, 0.25);
                assert_eq!(working_desires.get(0).unwrap().1.satisfaction, 30.0);
                assert_eq!(test.wants.get(&4).unwrap().reserved, 30.0);
                assert_eq!(test.wants.get(&4).unwrap().expected, 20.0);
                assert_eq!(test.property.get(&0).unwrap().reserved, 20.0);
                assert_eq!(test.property.get(&4).unwrap().reserved, 10.0);
                assert_eq!(test.property.get(&5).unwrap().reserved, 10.0);
                assert_eq!(test.property.get(&6).unwrap().reserved, 0.0);

                // third pass
                if let Some(result) =  test.satisfy_next_desire(&mut working_desires, &data) {
                    assert_eq!(result.0, 0.25);
                    assert_eq!(result.1.satisfaction, 40.0);
                    assert_eq!(working_desires.len(), 0);
                    assert_eq!(test.wants.get(&4).unwrap().reserved, 40.0);
                    assert_eq!(test.wants.get(&4).unwrap().expected, 30.0);
                    assert_eq!(test.property.get(&0).unwrap().reserved, 30.0);
                    assert_eq!(test.property.get(&4).unwrap().reserved, 10.0);
                    assert_eq!(test.property.get(&5).unwrap().reserved, 10.0);
                    assert_eq!(test.property.get(&6).unwrap().reserved, 10.0);
                } else {
                    assert!(false, "Did not end as expected.");
                }
            }
        }
    
        mod satisfy_until_incomplete_should {
            use std::collections::VecDeque;

            use crate::{data::Data, desire::{Desire, PriorityFn}, good::Good, item::Item, pop::{Pop, PropertyRecord}};

            #[test]
            pub fn correctly_stop_when_finished_incomplete() {
                let mut data = Data::new();
                data.add_time();
                data.add_good(Good::new(4, String::from("testGood4"), String::new()));
                data.add_good(Good::new(5, String::from("testGood5"), String::new()));
                data.add_good(Good::new(6, String::from("testGood6"), String::new()));
                data.add_good(Good::new(7, String::from("testGood7"), String::new()));

                let desire1 = Desire::new(Item::Good(4), 10.0, 1.0,
                        PriorityFn::linear(1.0))
                    .with_steps(0);
                let desire2 = Desire::new(Item::Good(5), 10.0, 1.0,
                        PriorityFn::linear(1.0))
                    .with_steps(0);
                let desire3 = Desire::new(Item::Good(6), 10.0, 1.0,
                        PriorityFn::linear(1.0))
                    .with_steps(0);
                let desire4 = Desire::new(Item::Good(7), 10.0, 1.0,
                        PriorityFn::linear(1.0))
                    .with_steps(0);

                let mut working_desires = VecDeque::new();
                working_desires.push_back((1.0, desire1));
                working_desires.push_back((1.0, desire2));
                working_desires.push_back((1.0, desire3));
                working_desires.push_back((1.0, desire4));

                let mut test = Pop::new(0, 0, 0);
                test.property.insert(0, PropertyRecord::new(100.0));
                test.property.insert(4, PropertyRecord::new(100.0));
                test.property.insert(5, PropertyRecord::new(100.0));
                test.property.insert(6, PropertyRecord::new(30.0));
                test.property.insert(7, PropertyRecord::new(100.0));

                let result = test.satisfy_until_incomplete(&mut working_desires, &data);

                if let Some((value, desire)) = result {
                    assert_eq!(value, 0.125); // 3 levels down
                    assert_eq!(desire.satisfaction, 30.0);
                    assert_eq!(desire.item, Item::Good(6));
                    assert_eq!(working_desires.len(), 3);
                    assert_eq!(working_desires.get(0).unwrap().0, 0.125);
                    assert_eq!(working_desires.get(0).unwrap().1.satisfaction, 30.0);
                    assert_eq!(working_desires.get(0).unwrap().1.item, Item::Good(7));
                    assert_eq!(working_desires.get(1).unwrap().0, 0.0625);
                    assert_eq!(working_desires.get(1).unwrap().1.satisfaction, 40.0);
                    assert_eq!(working_desires.get(1).unwrap().1.item, Item::Good(4));
                    assert_eq!(working_desires.get(2).unwrap().0, 0.0625);
                    assert_eq!(working_desires.get(2).unwrap().1.satisfaction, 40.0);
                    assert_eq!(working_desires.get(2).unwrap().1.item, Item::Good(5));
                }
            }
        }

        mod ordered_desire_insert_should {
            use std::collections::VecDeque;

            use crate::{desire::{Desire, PriorityFn}, item::Item, pop::Pop};

            #[test]
            pub fn insert_correctly() {
                let mut working_desires = VecDeque::new();

                let desire0 = Desire::new(Item::Good(0), 1.0, 10.0,
                    PriorityFn::linear(1.0));
                let desire1 = Desire::new(Item::Good(1), 1.0, 9.0,
                    PriorityFn::linear(1.0));
                let desire2 = Desire::new(Item::Good(2), 1.0, 1.0,
                    PriorityFn::linear(1.0));
                let desire3 = Desire::new(Item::Good(3), 1.0, 15.0,
                    PriorityFn::linear(1.0));
                let desire4 = Desire::new(Item::Good(4), 1.0, 10.0,
                    PriorityFn::linear(1.0));
                let desire5 = Desire::new(Item::Good(5), 1.0, 10.0,
                    PriorityFn::linear(1.0));

                Pop::ordered_desire_insert(&mut working_desires, desire0, 10.0);
                Pop::ordered_desire_insert(&mut working_desires, desire1, 9.0);

                assert_eq!(working_desires.get(0).unwrap().0, 10.0);
                assert_eq!(working_desires.get(0).unwrap().1.item, Item::Good(0));
                assert_eq!(working_desires.get(1).unwrap().0, 9.0);
                assert_eq!(working_desires.get(1).unwrap().1.item, Item::Good(1));

                Pop::ordered_desire_insert(&mut working_desires, desire2, 1.0);

                assert_eq!(working_desires.get(0).unwrap().0, 10.0);
                assert_eq!(working_desires.get(0).unwrap().1.item, Item::Good(0));
                assert_eq!(working_desires.get(1).unwrap().0, 9.0);
                assert_eq!(working_desires.get(1).unwrap().1.item, Item::Good(1));
                assert_eq!(working_desires.get(2).unwrap().0, 1.0);
                assert_eq!(working_desires.get(2).unwrap().1.item, Item::Good(2));

                Pop::ordered_desire_insert(&mut working_desires, desire3, 15.0);

                assert_eq!(working_desires.get(0).unwrap().0, 15.0);
                assert_eq!(working_desires.get(0).unwrap().1.item, Item::Good(3));
                assert_eq!(working_desires.get(1).unwrap().0, 10.0);
                assert_eq!(working_desires.get(1).unwrap().1.item, Item::Good(0));
                assert_eq!(working_desires.get(2).unwrap().0, 9.0);
                assert_eq!(working_desires.get(2).unwrap().1.item, Item::Good(1));
                assert_eq!(working_desires.get(3).unwrap().0, 1.0);
                assert_eq!(working_desires.get(3).unwrap().1.item, Item::Good(2));

                Pop::ordered_desire_insert(&mut working_desires, desire4, 10.0);

                assert_eq!(working_desires.get(0).unwrap().0, 15.0);
                assert_eq!(working_desires.get(0).unwrap().1.item, Item::Good(3));
                assert_eq!(working_desires.get(1).unwrap().0, 10.0);
                assert_eq!(working_desires.get(1).unwrap().1.item, Item::Good(0));
                assert_eq!(working_desires.get(2).unwrap().0, 10.0);
                assert_eq!(working_desires.get(2).unwrap().1.item, Item::Good(4));
                assert_eq!(working_desires.get(3).unwrap().0, 9.0);
                assert_eq!(working_desires.get(3).unwrap().1.item, Item::Good(1));
                assert_eq!(working_desires.get(4).unwrap().0, 1.0);
                assert_eq!(working_desires.get(4).unwrap().1.item, Item::Good(2));

                Pop::ordered_desire_insert(&mut working_desires, desire5, 10.0);

                assert_eq!(working_desires.get(0).unwrap().0, 15.0);
                assert_eq!(working_desires.get(0).unwrap().1.item, Item::Good(3));
                assert_eq!(working_desires.get(1).unwrap().0, 10.0);
                assert_eq!(working_desires.get(1).unwrap().1.item, Item::Good(0));
                assert_eq!(working_desires.get(2).unwrap().0, 10.0);
                assert_eq!(working_desires.get(2).unwrap().1.item, Item::Good(4));
                assert_eq!(working_desires.get(3).unwrap().0, 10.0);
                assert_eq!(working_desires.get(3).unwrap().1.item, Item::Good(5));
                assert_eq!(working_desires.get(4).unwrap().0, 9.0);
                assert_eq!(working_desires.get(4).unwrap().1.item, Item::Good(1));
                assert_eq!(working_desires.get(5).unwrap().0, 1.0);
                assert_eq!(working_desires.get(5).unwrap().1.item, Item::Good(2));
            }
        }

        mod consume_desire_should {
            use std::collections::HashMap;

            use crate::{data::Data, desire::{Desire, PriorityFn}, good::Good, item::Item, pop::{Pop, PropertyRecord, WantRecord}, want::Want};

            #[test]
            pub fn satisfy_good_correctly() {
                let mut data = Data::new();
                data.add_time();
                data.add_good(Good::new(4, String::from("testGood"), String::new()));

                let desire = Desire::new(Item::Good(4), 2.0, 1.0,
                    PriorityFn::linear(1.0))
                    .with_steps(0);

                let mut test = Pop::new(0, 0, 0);
                test.desires.push_back(desire);

                test.property.insert(4, PropertyRecord::new(3.0)); 

                test.satisfy_desires(&data);

                let mut current_desire = test.desires.remove(0).unwrap();
                current_desire.satisfaction = 0.0; // reset desire's satisfaction.
                let result = test.consume_desire(&mut current_desire, &data);

                assert!(result);
                assert_eq!(current_desire.satisfaction, 2.0);
                assert_eq!(test.property.get(&4).unwrap().owned, 1.0);
                assert_eq!(test.property.get(&4).unwrap().expended, 2.0);
                assert_eq!(test.property.get(&4).unwrap().reserved, 3.0);

                let result = test.consume_desire(&mut current_desire, &data);
                assert!(!result);
                assert_eq!(current_desire.satisfaction, 3.0);
                assert_eq!(test.property.get(&4).unwrap().owned, 0.0);
                assert_eq!(test.property.get(&4).unwrap().expended, 3.0);
                assert_eq!(test.property.get(&4).unwrap().reserved, 3.0);
            }

            #[test]
            pub fn satisfy_class_correctly() {
                let mut data = Data::new();
                data.add_time();
                data.add_good(Good::new(4, String::from("testGood1"), String::new())
                .in_class(4));
                data.add_good(Good::new(5, String::from("testGood2"), String::new())
                .in_class(4));

                let desire = Desire::new(Item::Class(4), 10.0, 1.0,
                    PriorityFn::linear(1.0))
                    .with_steps(0);

                let mut test = Pop::new(0, 0, 0);
                test.desires.push_back(desire);

                test.property.insert(4, PropertyRecord::new(7.0)); 
                test.property.insert(5, PropertyRecord::new(5.0)); 

                test.satisfy_desires(&data);

                let mut current_desire = test.desires.remove(0).unwrap();
                current_desire.satisfaction = 0.0;

                let result = test.consume_desire(&mut current_desire, &data);

                assert!(result);
                assert_eq!(current_desire.satisfaction, 10.0);
                assert_eq!(test.property.get(&4).unwrap().owned, 0.0);
                assert_eq!(test.property.get(&4).unwrap().reserved, 7.0);
                assert_eq!(test.property.get(&4).unwrap().expended, 7.0);
                assert_eq!(test.property.get(&5).unwrap().owned, 2.0);
                assert_eq!(test.property.get(&5).unwrap().reserved, 5.0);
                assert_eq!(test.property.get(&5).unwrap().expended, 3.0);

                let result = test.consume_desire(&mut current_desire, &data);

                assert!(!result);
                assert_eq!(current_desire.satisfaction, 12.0);
                assert_eq!(test.property.get(&4).unwrap().owned, 0.0);
                assert_eq!(test.property.get(&4).unwrap().reserved, 7.0);
                assert_eq!(test.property.get(&4).unwrap().expended, 7.0);
                assert_eq!(test.property.get(&5).unwrap().owned, 0.0);
                assert_eq!(test.property.get(&5).unwrap().reserved, 5.0);
                assert_eq!(test.property.get(&5).unwrap().expended, 5.0);
            }

            #[test]
            pub fn satisfy_want_correctly() {
                let mut data = Data::new();
                data.add_time();
                data.wants.insert(4, Want::new(4, String::from("testWant1")));
                data.wants.insert(5, Want::new(5, String::from("testWant2")));
                data.wants.insert(6, Want::new(6, String::from("testWant3")));
                let mut wants = HashMap::new();
                wants.insert(4, 1.0);
                wants.insert(5, 2.0);
                wants.insert(6, 0.5);
                data.add_good(Good::new(4, String::from("testGood1"), String::new())
                    .with_ownership(wants.clone()));
                data.add_good(Good::new(5, String::from("testGood2"), String::new())
                    .with_uses(2.0, wants.clone()));
                data.add_good(Good::new(6, String::from("testGood3"), String::new())
                    .with_consumption(1.0, wants.clone()));

                let desire = Desire::new(Item::Want(4), 15.0, 1.0,
                    PriorityFn::linear(1.0))
                    .with_steps(0);

                let mut test = Pop::new(0, 0, 0);
                test.desires.push_front(desire);

                test.wants.insert(4, WantRecord {
                    owned: 10.0,
                    reserved: 0.0,
                    expected: 0.0,
                    expended: 0.0,
                });
                test.property.insert(0, PropertyRecord::new(100.0)); 
                test.property.insert(4, PropertyRecord::new(10.0)); 
                test.property.insert(5, PropertyRecord::new(10.0)); 
                test.property.insert(6, PropertyRecord::new(10.0)); 

                test.satisfy_desires(&data);

                let mut current_desire = test.desires.remove(0).unwrap();
                current_desire.satisfaction = 0.0;

                // first pass
                let result = test.consume_desire(&mut current_desire, &data);
                assert!(result);
                assert_eq!(current_desire.satisfaction, 15.0);
                assert_eq!(test.wants.get(&4).unwrap().owned, 0.0);
                assert_eq!(test.wants.get(&4).unwrap().expended, 15.0);
                assert_eq!(test.wants.get(&4).unwrap().reserved, 40.0);
                assert_eq!(test.wants.get(&5).unwrap().owned, 10.0);
                assert_eq!(test.wants.get(&5).unwrap().expended, 0.0);
                assert_eq!(test.wants.get(&5).unwrap().reserved, 0.0);
                assert_eq!(test.wants.get(&6).unwrap().owned, 2.5);
                assert_eq!(test.wants.get(&6).unwrap().expended, 0.0);
                assert_eq!(test.wants.get(&6).unwrap().reserved, 0.0);

                assert_eq!(test.property.get(&4).unwrap().owned, 5.0);
                assert_eq!(test.property.get(&4).unwrap().reserved, 10.0);
                assert_eq!(test.property.get(&4).unwrap().expended, 0.0);
                assert_eq!(test.property.get(&4).unwrap().used, 5.0);
                assert_eq!(test.property.get(&5).unwrap().owned, 10.0);
                assert_eq!(test.property.get(&5).unwrap().reserved, 10.0);
                assert_eq!(test.property.get(&5).unwrap().expended, 0.0);
                assert_eq!(test.property.get(&4).unwrap().used, 5.0);
                assert_eq!(test.property.get(&6).unwrap().owned, 10.0);
                assert_eq!(test.property.get(&6).unwrap().reserved, 10.0);
                assert_eq!(test.property.get(&6).unwrap().expended, 0.0);
                assert_eq!(test.property.get(&4).unwrap().used, 5.0);

                // second pass
                let result = test.consume_desire(&mut current_desire, &data);
                assert!(result);
                assert_eq!(current_desire.satisfaction, 30.0);
                assert_eq!(test.wants.get(&4).unwrap().owned, 0.0);
                assert_eq!(test.wants.get(&4).unwrap().expended, 30.0);
                assert_eq!(test.wants.get(&4).unwrap().reserved, 40.0);
                assert_eq!(test.wants.get(&5).unwrap().owned, 40.0);
                assert_eq!(test.wants.get(&5).unwrap().expended, 0.0);
                assert_eq!(test.wants.get(&5).unwrap().reserved, 0.0);
                assert_eq!(test.wants.get(&6).unwrap().owned, 10.0);
                assert_eq!(test.wants.get(&6).unwrap().expended, 0.0);
                assert_eq!(test.wants.get(&6).unwrap().reserved, 0.0);

                assert_eq!(test.property.get(&4).unwrap().owned, 0.0);
                assert_eq!(test.property.get(&4).unwrap().reserved, 10.0);
                assert_eq!(test.property.get(&4).unwrap().expended, 0.0);
                assert_eq!(test.property.get(&5).unwrap().used, 10.0);
                assert_eq!(test.property.get(&5).unwrap().owned, 0.0);
                assert_eq!(test.property.get(&5).unwrap().reserved, 10.0);
                assert_eq!(test.property.get(&5).unwrap().expended, 0.0);
                assert_eq!(test.property.get(&5).unwrap().used, 10.0);
                assert_eq!(test.property.get(&6).unwrap().owned, 10.0);
                assert_eq!(test.property.get(&6).unwrap().reserved, 10.0);
                assert_eq!(test.property.get(&6).unwrap().expended, 0.0);
                assert_eq!(test.property.get(&5).unwrap().used, 10.0);

                // third pass
                let result = test.consume_desire(&mut current_desire, &data);
                assert!(!result);
                assert_eq!(current_desire.satisfaction, 40.0);
                assert_eq!(test.wants.get(&4).unwrap().owned, 0.0);
                assert_eq!(test.wants.get(&4).unwrap().expended, 40.0);
                assert_eq!(test.wants.get(&4).unwrap().reserved, 40.0);
                assert_eq!(test.wants.get(&5).unwrap().owned, 60.0);
                assert_eq!(test.wants.get(&5).unwrap().expended, 0.0);
                assert_eq!(test.wants.get(&5).unwrap().reserved, 0.0);
                assert_eq!(test.wants.get(&6).unwrap().owned, 15.0);
                assert_eq!(test.wants.get(&6).unwrap().expended, 0.0);
                assert_eq!(test.wants.get(&6).unwrap().reserved, 0.0);

                assert_eq!(test.property.get(&4).unwrap().owned, 0.0);
                assert_eq!(test.property.get(&4).unwrap().reserved, 10.0);
                assert_eq!(test.property.get(&4).unwrap().expended, 0.0);
                assert_eq!(test.property.get(&5).unwrap().used, 10.0);
                assert_eq!(test.property.get(&5).unwrap().owned, 00.0);
                assert_eq!(test.property.get(&5).unwrap().reserved, 10.0);
                assert_eq!(test.property.get(&5).unwrap().expended, 0.0);
                assert_eq!(test.property.get(&5).unwrap().used, 10.0);
                assert_eq!(test.property.get(&6).unwrap().owned, 0.0);
                assert_eq!(test.property.get(&6).unwrap().reserved, 10.0);
                assert_eq!(test.property.get(&6).unwrap().expended, 10.0);
                assert_eq!(test.property.get(&5).unwrap().used, 10.0);
            }
        }
    
        mod consume_desires_should {
            use std::collections::HashMap;

            use crate::{constants::TIME_ID, data::Data, desire::{Desire, PriorityFn}, good::Good, item::Item, pop::{Pop, PropertyRecord, WantRecord}, want::Want};

            #[test]
            pub fn correctly_consume_desires() {
                let mut data = Data::new();
                data.add_time();
                data.wants.insert(4, Want::new(4, String::from("testWant1")));
                data.wants.insert(5, Want::new(5, String::from("testWant2")));
                data.wants.insert(6, Want::new(6, String::from("testWant3")));
                let mut wants = HashMap::new();
                wants.insert(4, 1.0);
                wants.insert(5, 2.0);
                wants.insert(6, 0.5);
                data.add_good(Good::new(4, String::from("testGood1"), String::new())
                    .with_ownership(wants.clone())
                    .in_class(4));
                data.add_good(Good::new(5, String::from("testGood2"), String::new())
                    .with_uses(2.0, wants.clone())
                    .in_class(4));
                data.add_good(Good::new(6, String::from("testGood3"), String::new())
                    .with_consumption(1.0, wants.clone()));

                let unit_slope = 4.0 / 3.0;

                let desire1 = Desire::new(Item::Good(4), 10.0, 2.0,
                    PriorityFn::linear(unit_slope))
                    .with_steps(0);
                let desire2 = Desire::new(Item::Class(4), 10.0, 2.0,
                    PriorityFn::linear(unit_slope))
                    .with_steps(0);
                let desire3 = Desire::new(Item::Want(4), 10.0, 2.0,
                        PriorityFn::linear(unit_slope))
                    .with_steps(0);
                let desire4 = Desire::new(Item::Good(5), 10.0, 2.0,
                        PriorityFn::linear(unit_slope))
                    .with_steps(0);
                let desire5 = Desire::new(Item::Want(5), 10.0, 2.0,
                        PriorityFn::linear(unit_slope))
                    .with_steps(0);

                let mut test = Pop::new(0, 0, 0);
                test.desires.push_back(desire1);
                test.desires.push_back(desire2);
                test.desires.push_back(desire3);
                test.desires.push_back(desire4);
                test.desires.push_back(desire5);

                test.property.insert(TIME_ID, PropertyRecord::new(100.0));
                test.property.insert(4, PropertyRecord::new(20.0));
                test.property.insert(5, PropertyRecord::new(20.0));
                test.property.insert(6, PropertyRecord::new(20.0)); 
                test.wants.insert(4, WantRecord { owned: 10.0, reserved: 0.0, expected: 0.0, expended: 0.0 });

                let result = test.consume_desires(&data);
                println!("Steps: {}", result.0);
                println!("value: {}", result.1);

                assert_eq!(result.0, 11.0);
                assert_eq!(result.1, 140.0);

                assert_eq!(test.property.get(&0).unwrap().owned, 80.0);
                assert_eq!(test.property.get(&0).unwrap().expended, 20.0);
                assert_eq!(test.property.get(&0).unwrap().reserved, 0.0);
                assert_eq!(test.property.get(&0).unwrap().used, 0.0);
                assert_eq!(test.property.get(&4).unwrap().owned, 0.0);
                assert_eq!(test.property.get(&4).unwrap().expended, 20.0);
                assert_eq!(test.property.get(&4).unwrap().reserved, 0.0);
                assert_eq!(test.property.get(&4).unwrap().used, 0.0);
                assert_eq!(test.property.get(&5).unwrap().owned, 0.0);
                assert_eq!(test.property.get(&5).unwrap().expended, 15.0);
                assert_eq!(test.property.get(&5).unwrap().reserved, 0.0);
                assert_eq!(test.property.get(&5).unwrap().used, 5.0);
                assert_eq!(test.property.get(&6).unwrap().owned, 0.0);
                assert_eq!(test.property.get(&6).unwrap().expended, 20.0);
                assert_eq!(test.property.get(&6).unwrap().reserved, 0.0);
                assert_eq!(test.property.get(&6).unwrap().used, 0.0);

                assert_eq!(test.wants.get(&4).unwrap().expected, 0.0);
                assert_eq!(test.wants.get(&4).unwrap().expended, 35.0);
                assert_eq!(test.wants.get(&4).unwrap().owned, 0.0);
                assert_eq!(test.wants.get(&4).unwrap().reserved, 0.0);
                assert_eq!(test.wants.get(&5).unwrap().expected, 0.0);
                assert_eq!(test.wants.get(&5).unwrap().expended, 50.0);
                assert_eq!(test.wants.get(&5).unwrap().owned, 0.0);
                assert_eq!(test.wants.get(&5).unwrap().reserved, 0.0);
            }
        }
    }
}