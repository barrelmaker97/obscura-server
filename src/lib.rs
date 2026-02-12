#![forbid(unsafe_code)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::unwrap_used)]
#![warn(clippy::todo)]
#![warn(clippy::panic)]
#![warn(clippy::dbg_macro)]
#![warn(clippy::print_stdout)]
#![warn(clippy::print_stderr)]
#![warn(clippy::clone_on_ref_ptr)]
#![warn(unreachable_pub)]
#![warn(missing_debug_implementations)]
#![warn(unused_qualifications)]
#![deny(unused_must_use)]

pub mod api;
pub mod config;
pub mod domain;
pub mod error;
pub mod proto;
pub mod services;
pub mod storage;
pub mod telemetry;
