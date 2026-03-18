//! Change Set Types
//!
//! Categorizes file changes detected by FileTracker.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeSet {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
}

impl ChangeSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }

    pub fn total_changes(&self) -> usize {
        self.added.len() + self.modified.len() + self.deleted.len()
    }

    pub fn all_changed_files(&self) -> impl Iterator<Item = &String> {
        self.added
            .iter()
            .chain(self.modified.iter())
            .chain(self.deleted.iter())
    }

    pub fn affected_files(&self) -> impl Iterator<Item = &String> {
        self.added.iter().chain(self.modified.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_change_set() {
        let cs = ChangeSet::new();
        assert!(cs.is_empty());
        assert_eq!(cs.total_changes(), 0);
    }

    #[test]
    fn test_non_empty_change_set() {
        let cs = ChangeSet {
            added: vec!["new.rs".into()],
            modified: vec!["changed.rs".into()],
            deleted: vec!["removed.rs".into()],
        };
        assert!(!cs.is_empty());
        assert_eq!(cs.total_changes(), 3);
    }

    #[test]
    fn test_all_changed_files() {
        let cs = ChangeSet {
            added: vec!["a.rs".into()],
            modified: vec!["b.rs".into()],
            deleted: vec!["c.rs".into()],
        };
        let files: Vec<_> = cs.all_changed_files().collect();
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn test_affected_files() {
        let cs = ChangeSet {
            added: vec!["a.rs".into()],
            modified: vec!["b.rs".into()],
            deleted: vec!["c.rs".into()],
        };
        let files: Vec<_> = cs.affected_files().collect();
        assert_eq!(files.len(), 2);
        assert!(!files.contains(&&"c.rs".to_string()));
    }
}
