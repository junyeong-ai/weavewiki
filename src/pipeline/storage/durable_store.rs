//! Durable Store Implementation
//!
//! File-based persistent storage for AST analysis results.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use super::serialization::{Compression, StorageFormat};
use crate::Result;

#[derive(Debug, Clone)]
pub struct StoreConfig {
    pub base_dir: PathBuf,
    pub format: StorageFormat,
    pub compression: Compression,
}

impl StoreConfig {
    pub fn new(project_root: &Path) -> Self {
        Self {
            base_dir: project_root.join(".claudegen"),
            format: StorageFormat::JsonPretty,
            compression: Compression::None,
        }
    }

    fn ast_dir(&self) -> PathBuf {
        self.base_dir.join("ast")
    }
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            base_dir: PathBuf::from(".claudegen"),
            format: StorageFormat::JsonPretty,
            compression: Compression::None,
        }
    }
}

pub struct DurableStore {
    config: StoreConfig,
}

impl DurableStore {
    pub fn new(config: StoreConfig) -> Result<Self> {
        let store = Self { config };
        store.ensure_directories()?;
        Ok(store)
    }

    pub fn from_project_root(project_root: &Path) -> Result<Self> {
        Self::new(StoreConfig::new(project_root))
    }

    fn ensure_directories(&self) -> Result<()> {
        let dirs = [self.config.ast_dir(), self.config.ast_dir().join("files")];

        for dir in dirs {
            fs::create_dir_all(&dir)?;
        }
        Ok(())
    }

    fn write_json<T: Serialize>(&self, path: &Path, value: &T) -> Result<()> {
        let temp_path = path.with_extension("tmp");
        let data = self.config.format.serialize(value)?;
        let compressed = self.config.compression.compress(&data)?;

        let file = File::create(&temp_path)?;
        let mut writer = BufWriter::new(file);
        super::serialization::write_atomic(&mut writer, &compressed)?;
        drop(writer);

        fs::rename(&temp_path, path)?;
        Ok(())
    }

    fn file_hash(path: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(path.as_bytes());
        let result = hasher.finalize();
        hex::encode(&result[..8])
    }

    pub async fn save_ast_result<T: Serialize>(&self, result: &T) -> Result<()> {
        let path = self.config.ast_dir().join("project_structure.json");
        self.write_json(&path, result)
    }

    pub async fn save_file_ast<T: Serialize>(&self, file_path: &str, result: &T) -> Result<()> {
        let hash = Self::file_hash(file_path);
        let path = self
            .config
            .ast_dir()
            .join("files")
            .join(format!("{hash}.json"));
        self.write_json(&path, result)
    }
}

mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
