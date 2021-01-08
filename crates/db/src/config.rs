use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// TODO(doc): @doitian
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub path: PathBuf,
    #[serde(default)]
    pub options: HashMap<String, String>,
    pub options_file: Option<PathBuf>,
}
