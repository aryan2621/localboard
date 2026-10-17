//! Local vectors: f32 blobs in workspace SQLite, compared with brute-force cosine similarity.
//! Fast enough for the tens of thousands of rules, examples and notes one workspace holds.

mod documents;

pub use documents::read_knowledge;

use sha2::{Digest, Sha256};

/// nomic-embed-text v1.5 is trained with these prefixes. Hashing includes the prefix version
/// so a future model change re-embeds instead of mixing vector spaces.
pub const DOC_PREFIX: &str = "search_document: ";
pub const QUERY_PREFIX: &str = "search_query: ";
const HASH_VERSION: &str = "nomic-v1.5";

/// Below this, an embedding match is too weak to outrank a keyword hit.
pub const MIN_COSINE: f32 = 0.32;

/// Memories match case-insensitively, so "Exclude tests" and "exclude tests" are one memory.
pub fn memory_key(body: &str) -> String {
    content_hash(&body.trim().to_lowercase())
}

pub fn content_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(HASH_VERSION.as_bytes());
    hasher.update([0]);
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn pack(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

pub fn unpack(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    Some(bytes.chunks_exact(4).map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])).collect())
}

pub fn cosine(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let (dot, left_norm, right_norm) = left.iter().zip(right).fold((0.0f32, 0.0f32, 0.0f32), |acc, (a, b)| {
        (acc.0 + a * b, acc.1 + a * a, acc.2 + b * b)
    });
    let denom = left_norm.sqrt() * right_norm.sqrt();
    if denom == 0.0 { 0.0 } else { dot / denom }
}

/// Keeps the first `max_chars` characters. Embedding models reject inputs past their context.
pub fn clip(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_owned()
    } else {
        text.chars().take(max_chars).collect()
    }
}
