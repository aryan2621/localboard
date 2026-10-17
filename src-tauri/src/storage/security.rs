//! Small helpers for comparing secrets and encoding bytes.

pub fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Constant-time comparison for tokens and password hashes.
pub fn tokens_match(expected: &str, provided: &str) -> bool {
    if expected.len() != provided.len() {
        return false;
    }
    expected.as_bytes().iter().zip(provided.as_bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}
