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

            use crate::{data::Data, good::Good, process::{InputTag, Process}};
        }
    }
}