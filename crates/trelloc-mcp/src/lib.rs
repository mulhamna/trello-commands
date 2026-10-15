#![doc = include_str!("../README.md")]

mod server;

pub use server::{run_stdio, run_streamable_http, TrelloMcpServer};
