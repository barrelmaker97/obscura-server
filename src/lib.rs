#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]
#![warn(clippy::unwrap_used)]
#![warn(clippy::todo)]
#![warn(clippy::panic)]
#![warn(clippy::dbg_macro)]
#![warn(unreachable_pub)]
#![warn(missing_debug_implementations)]
#![deny(unused_must_use)]

pub mod api;
pub mod config;
pub mod domain;
pub mod error;
pub mod proto;
pub mod services;
pub mod storage;
pub mod telemetry;
