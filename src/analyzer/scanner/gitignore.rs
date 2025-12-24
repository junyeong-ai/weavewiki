use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::Path;

pub struct GitIgnoreFilter {
    gitignore: Option<Gitignore>,
}

impl GitIgnoreFilter {
    pub fn new<P: AsRef<Path>>(root: P) -> Self {
        let gitignore_path = root.as_ref().join(".gitignore");

        let gitignore = if gitignore_path.exists() {
            let mut builder = GitignoreBuilder::new(root.as_ref());
            builder.add(&gitignore_path);
            builder.build().ok()
        } else {
            None
        };

        Self { gitignore }
    }

    pub fn is_ignored<P: AsRef<Path>>(&self, path: P) -> bool {
        match &self.gitignore {
            Some(gi) => gi
                .matched(path.as_ref(), path.as_ref().is_dir())
                .is_ignore(),
            None => false,
        }
    }
}
