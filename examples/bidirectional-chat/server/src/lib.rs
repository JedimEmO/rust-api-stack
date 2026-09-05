//! Bidirectional chat server library

pub mod config;
pub mod persistence;

mod app;
mod auth;
mod chat;
pub use app::{ApplicationDependencies, ChatApplication, build_application};
