//! Ranksy API client.

pub mod generated {
    #![allow(clippy::all, dead_code)]
    include!(concat!(env!("OUT_DIR"), "/codegen.rs"));
}

mod client;
pub use client::{ApiError, ClientConfig, RanksyClient};
