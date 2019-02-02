use sha2::{Digest, Sha256};

pub fn sha256hash(input: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.input(input);
    hasher.result()[..].to_vec()
}
