#![allow(dead_code)]
#![feature(closure_to_fn_coercion)]

mod cell;
mod graph;
mod safe;
mod test;

pub use self::cell::{DepCell, Task};
pub use self::graph::{DepGraph, DepNodeName, DepNodeIndex};
pub use self::safe::DepGraphSafe;
