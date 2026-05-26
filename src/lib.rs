//! `goctl` library surface — separated from the binary so integration tests can drive it.

pub mod auth;
pub mod cli;
pub mod config;
pub mod endpoints;
pub mod error;
pub mod exec;
pub mod fixtures;
pub mod output;
pub mod render;
