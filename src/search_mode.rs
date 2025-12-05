use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchMode {
    Apps,
    Files,
    Run,
}

impl SearchMode {
    pub fn as_str(&self) -> &str {
        match self {
            SearchMode::Apps => "Apps",
            SearchMode::Files => "Files",
            SearchMode::Run => "Run",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub name: String,
    pub path: String,
    pub result_type: SearchMode,
}

impl SearchResult {
    pub fn new(name: String, path: String, result_type: SearchMode) -> Self {
        Self {
            name,
            path,
            result_type,
        }
    }
}
