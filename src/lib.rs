#![feature(plugin)]
#![plugin(interpolate_idents)]

// #![feature(specialization)]
#![allow(missing_docs)]
#![allow(unknown_lints)]

mod types;
mod typesetting;
pub mod mathmlparser;
pub use typesetting::*;
pub use types::*;
