use serde::{Deserialize, Serialize};

pub const COMMAND: &str = "--add-candidate-from-stdin";
pub const EXISTS: i32 = 2;
pub const FAILED: i32 = 3;
pub const MAX_REQUEST_BYTES: u64 = 8192;
pub const DEFAULT_KEY: u8 = 2;

#[derive(Serialize, Deserialize)]
pub struct Candidate {
    pub head: String,
    pub reading: String,
    pub russian: String,
}

pub fn parse_key(value: &str) -> Option<u8> {
    value.parse::<u8>().ok().filter(|key| (1..=12).contains(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_supported_function_keys() {
        assert_eq!(parse_key("1"), Some(1));
        assert_eq!(parse_key("12"), Some(12));
        for value in ["", "0", "13", "A", "F2"] {
            assert_eq!(parse_key(value), None);
        }
    }
}
