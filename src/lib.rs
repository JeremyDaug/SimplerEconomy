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
                availables.insert(1, 10.0);
                availables.insert(2, 10.0);
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
                assert_eq!(result.iterations, 20.0);
                assert_eq!(result.consumed.len(), 2);
                assert_eq!(*result.consumed.get(&0).unwrap(), 20.0);
                assert_eq!(*result.consumed.get(&1).unwrap(), 10.0);
                assert_eq!(result.used.len(), 2);
                assert_eq!(*result.used.get(&0).unwrap(), 20.0);
                assert_eq!(*result.used.get(&2).unwrap(), 0.0);
                assert_eq!(result.created.len(), 3);
                assert_eq!(*result.created.get(&Item::Good(11)).unwrap(), 20.0);
                assert_eq!(*result.created.get(&Item::Good(3)).unwrap(), 40.0);
                assert_eq!(*result.created.get(&Item::Want(5)).unwrap(), 60.0);
            }

            #[test]
            pub fn run_partial_process_with_optionals_and_unable_to_run() {
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
                assert_eq!(result.iterations, 18.0);
                assert_eq!(result.consumed.len(), 2);
                assert_eq!(*result.consumed.get(&0).unwrap(), 18.0);
                assert_eq!(*result.consumed.get(&1).unwrap(), 5.0);
                assert_eq!(result.used.len(), 2);
                assert_eq!(*result.used.get(&0).unwrap(), 18.0);
                assert_eq!(*result.used.get(&2).unwrap(), 0.0);
                assert_eq!(result.created.len(), 3);
                assert_eq!(*result.created.get(&Item::Good(11)).unwrap(), 10.0);
                assert_eq!(*result.created.get(&Item::Good(3)).unwrap(), 36.0);
                assert_eq!(*result.created.get(&Item::Want(5)).unwrap(), 54.0);
            }
        }
    }
}