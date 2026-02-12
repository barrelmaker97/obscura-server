#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::needless_raw_string_hashes)]
#![deny(unused_must_use)]

pub mod api;
pub mod config;
pub mod domain;
pub mod error;
pub mod proto;
pub mod services;
pub mod storage;
pub mod telemetry;
