//! Serialization Utilities
//!
//! Handles JSON serialization with optional compression for durable storage.

use serde::Serialize;
use std::io::Write;

use crate::Result;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StorageFormat {
    #[default]
    Json,
    JsonPretty,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Compression {
    #[default]
    None,
}

impl StorageFormat {
    pub fn serialize<T: Serialize>(&self, value: &T) -> Result<Vec<u8>> {
        let bytes = match self {
            Self::Json => serde_json::to_vec(value)?,
            Self::JsonPretty => serde_json::to_vec_pretty(value)?,
        };
        Ok(bytes)
    }
}

impl Compression {
    pub fn compress(&self, data: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::None => Ok(data.to_vec()),
        }
    }
}

pub fn write_atomic<W: Write>(mut writer: W, data: &[u8]) -> std::io::Result<()> {
    writer.write_all(data)?;
    writer.flush()
}
