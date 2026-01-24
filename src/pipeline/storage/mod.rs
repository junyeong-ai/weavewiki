//! Durable Storage Module
//!
//! File-based persistent state management for crash recovery and resumable execution.
//! All analysis results, execution state, and generated artifacts are stored durably.

mod durable_store;
mod serialization;

pub use durable_store::{DurableStore, StoreConfig};
pub use serialization::{Compression, StorageFormat};
