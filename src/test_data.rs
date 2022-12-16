use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub(crate) struct Entry {
    x: f64,
    y: f64,
}

pub(crate) const ENTRY_RAW_YML: &str = "x: 1.0\ny: 2.0\n";
pub(crate) const ENTRY_RAW_TOML: &str = "x = 1.0\ny = 2.0\n";
pub(crate) const ENTRY_RAW_JSON: &str = r#"{"x": 1.0, "y": 2.0}"#;
