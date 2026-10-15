#![doc = include_str!("../README.md")]

pub mod board_ref;
pub mod client;
pub mod config;
pub mod error;
pub mod model;
pub mod rate_limit;
pub mod time;

pub use board_ref::BoardRef;
pub use client::{Query, TrelloClient};
pub use config::TrelloConfig;
pub use error::{Result, TrelloError};
