//! Incremental Sync System
//!
//! Tracks file changes and determines which artifacts need regeneration.
//!
//! Components:
//! - `FileTracker`: blake3-based file change detection
//! - `ChangeSet`: Categorized file changes (added/modified/deleted)
//! - `DependencyGraph`: File → Module → Artifact mappings
//! - `SyncResult`: Sync operation results

mod change_set;
mod dependencies;
mod result;
mod tracker;

pub use change_set::ChangeSet;
pub use dependencies::{ArtifactRef, DependencyGraph};
pub use result::{RegeneratedArtifact, SkippedArtifact, SyncError, SyncResult};
pub use tracker::FileTracker;
