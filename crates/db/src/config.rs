use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// TODO(doc): @doitian
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// TODO(doc): @doitian
    #[serde(default)]
    pub path: PathBuf,
    /// TODO(doc): @doitian
    #[serde(default)]
    pub options: HashMap<String, String>,
    /// TODO(doc): @doitian
    pub options_file: Option<PathBuf>,
}
