//! Bundles resources
//!
//! This crate bundles the builtin scripts binaries of Godwoken
//!
//! The bundled files can be read via `Resource::Bundled`, for example:
//!
//! ```no_run
//! // Read bundled godwoken binaries
//! use gw_builtin_binaries::Resource;
//!
//! let binary = Resource::bundled("builtin/godwoken-polyjuice-v1.2.0/generator".to_string()).get().unwrap();
//! ```
//!
mod bundled {
    #![allow(missing_docs, clippy::unreadable_literal)]
    include!(concat!(env!("OUT_DIR"), "/bundled.rs"));
}
use std::{
    borrow::Cow,
    fmt, fs,
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
};

/// Bundled resources
pub use bundled::BUNDLED;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Represents a resource, which is either bundled in the GW binary or resident in the local file
/// system.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Resource {
    /// A resource that bundled in the GW binary.
    Bundled {
        /// The identifier of the bundled resource.
        bundled: String,
    },
    /// A resource that resides in the local file system.
    FileSystem {
        /// The file path to the resource.
        file: PathBuf,
    },
}

impl fmt::Display for Resource {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Resource::Bundled { bundled } => write!(f, "Bundled({})", bundled),
            Resource::FileSystem { file } => write!(f, "FileSystem({})", file.display()),
        }
    }
}

impl Resource {
    /// Creates a reference to the bundled resource.
    pub fn bundled(bundled: String) -> Resource {
        Resource::Bundled { bundled }
    }

    /// Creates a reference to the resource recident in the file system.
    pub fn file_system(file: PathBuf) -> Resource {
        Resource::FileSystem { file }
    }

    /// Returns `true` if this is a bundled resource.
    pub fn is_bundled(&self) -> bool {
        matches!(self, Resource::Bundled { .. })
    }

    /// Returns `true` if the resource exists.
    ///
    /// The bundled resource exists only when the identifier is included in the bundle.
    ///
    /// The file system resource exists only when the file exists.
    pub fn exists(&self) -> bool {
        match self {
            Resource::Bundled { bundled } => BUNDLED.is_available(bundled),
            Resource::FileSystem { file } => file.exists(),
        }
    }

    /// Gets resource content.
    pub fn get(&self) -> Result<Cow<'static, [u8]>> {
        match self {
            Resource::Bundled { bundled } => BUNDLED.get(bundled).map_err(Into::into),
            Resource::FileSystem { file } => Ok(Cow::Owned(fs::read(file)?)),
        }
    }

    /// Gets resource content via an input stream.
    pub fn read(&self) -> Result<Box<dyn Read>> {
        match self {
            Resource::Bundled { bundled } => BUNDLED.read(bundled).map_err(Into::into),
            Resource::FileSystem { file } => Ok(Box::new(BufReader::new(fs::File::open(file)?))),
        }
    }

    /// Exports a bundled resource.
    ///
    /// This function returns `Ok` immediatly when invoked on a file system resource.
    ///
    /// The file is exported to the path by combining `root_dir` and the resource indentifier.
    ///
    /// These bundled files can be customized for different chains using spec branches.
    /// See [Template](struct.Template.html).
    pub fn export<P: AsRef<Path>>(&self, root_dir: P) -> Result<()> {
        let key = match self {
            Resource::Bundled { bundled } => bundled,
            _ => return Ok(()),
        };
        let target = join_bundled_key(root_dir.as_ref().to_path_buf(), key);
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut f = fs::File::create(&target)?;
        f.write_all(self.get()?.as_ref())?;
        Ok(())
    }
}

fn join_bundled_key(mut root_dir: PathBuf, key: &str) -> PathBuf {
    key.split('/')
        .for_each(|component| root_dir.push(component));
    root_dir
}

pub fn content_checksum(content: &[u8]) -> [u8; 32] {
    Sha256::digest(content).into()
}

pub fn file_checksum<P: AsRef<Path>>(path: P) -> Result<[u8; 32]> {
    let content = std::fs::read(path)?;
    Ok(content_checksum(&content))
}
