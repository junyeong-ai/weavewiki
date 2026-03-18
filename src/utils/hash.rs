/// Compute stable, cryptographic hash of content using SHA-256.
///
/// SHA-256 provides:
/// - Deterministic across Rust versions and platforms
/// - Cryptographically secure
/// - Stable hash output (critical for caching)
pub fn content_hash(data: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let hash1 = content_hash("hello world");
        let hash2 = content_hash("hello world");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn different_inputs_differ() {
        let hash1 = content_hash("hello");
        let hash2 = content_hash("world");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn empty_input() {
        let hash = content_hash("");
        assert!(!hash.is_empty());
    }

    #[test]
    fn hex_format() {
        let hash = content_hash("test");
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
