use std::path::PathBuf;

use anyhow::{Context, Result};
use getrandom::getrandom;
use structopt::StructOpt;

pub const COMMAND_PEER_ID: &str = "peer-id";

/// P2P authentication secret key and peer id related commands.
#[derive(StructOpt)]
#[structopt(name = COMMAND_PEER_ID)]
pub enum PeerIdCommand {
    /// Generate secret key.
    Gen {
        /// Output secret key to file path.
        #[structopt(long)]
        secret_path: PathBuf,
    },
    /// Compute peer id from secret key.
    FromSecret {
        /// Secret key file path.
        #[structopt(long)]
        secret_path: PathBuf,
    },
}

impl PeerIdCommand {
    pub fn run(self) -> Result<()> {
        match self {
            PeerIdCommand::Gen { secret_path } => {
                let mut secret_key = [0u8; 32];
                getrandom(&mut secret_key).context("getrandom")?;
                tentacle_secio::SecioKeyPair::secp256k1_raw_key(secret_key)
                    .context("generate secret key")?;
                std::fs::write(&secret_path, secret_key).with_context(|| {
                    format!("write secret key to {}", secret_path.to_string_lossy())
                })?;
            }
            PeerIdCommand::FromSecret { secret_path } => {
                let secret_key = std::fs::read(secret_path).context("read secret key from file")?;
                let key_pair = tentacle_secio::SecioKeyPair::secp256k1_raw_key(secret_key)
                    .context("read secret key")?;
                let peer_id = key_pair.public_key().peer_id();
                println!("{}", peer_id.to_base58());
            }
        }
        Ok(())
    }
}
