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
                let mut test = Process::new(0, "Test".to_string(), String::new())
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
            }
        }

        mod do_process_should {
            use std::collections::HashMap;

            use crate::{data::Data, good::Good, process::{InputTag, Process}};
        }
    }
}