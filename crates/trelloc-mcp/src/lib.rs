#![doc = include_str!("../README.md")]

pub mod app;
pub mod error;
pub mod models;
mod server;

pub use server::{run_stdio, run_streamable_http, TrelloMcpServer};
