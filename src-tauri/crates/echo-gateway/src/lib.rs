//! The model gateway shared by EchoBird and Wings.
//!
//! Moved here piece by piece from EchoBird's `services::local_llm::proxy`, `services::account_proxy` and
//! `services::smart_router`, without changing behaviour; EchoBird re-exports each piece at its old path.
//! Nothing in this crate may depend on Tauri or on either app's state.

pub mod convert;
pub mod error;
pub mod failure;
pub mod protocol;
pub mod responses;
pub mod signatures;
pub mod tools;

// Built for Wings' automatic routing: the plan as data (made by the scorer and the helper, checked before
// use), where the plan in use lives, how each channel is doing, and the hot-path choice.
pub mod engine;
pub mod health;
pub mod plan;
pub mod scorer;
pub mod select;
pub mod store;
pub mod telemetry;
pub mod upstream;
