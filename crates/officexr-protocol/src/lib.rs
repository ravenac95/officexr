//! Wire types for the realtime server and a client that joins one of its rooms.

mod client;
#[allow(clippy::all, clippy::pedantic, unused)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/protocol.rs"));
}

pub use client::{Config, Error, Incoming, WsClient};
pub use generated::*;
