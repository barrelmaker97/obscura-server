#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![deny(unused_must_use)]

pub mod api;
pub mod config;
pub mod domain;
pub mod error;
pub mod proto;
pub mod services;
pub mod storage;
pub mod telemetry;
