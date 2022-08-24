use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use gw_config::P2PNetworkConfig;
use gw_utils::exponential_backoff::ExponentialBackoff;
use socket2::SockRef;
use tentacle::{
    async_trait,
    builder::ServiceBuilder,
    context::{ServiceContext, SessionContext},
    multiaddr::{MultiAddr, Protocol},
    secio::{PeerId, SecioKeyPair},
    service::{
        ProtocolMeta, Service, ServiceAsyncControl, ServiceError, ServiceEvent, TargetProtocol,
    },
    traits::{ProtocolSpawn, ServiceHandle},
    utils::extract_peer_id,
    ProtocolId, SubstreamReadPart,
};

const RECONNECT_BASE_DURATION: Duration = Duration::from_secs(2);

/// Wrapper for tentacle Service. Automatically reconnect dial addresses.
pub struct P2PNetwork {
    service: Service<SHandle>,
}

impl P2PNetwork {
    pub async fn init<PS>(config: &P2PNetworkConfig, protocols: PS) -> Result<Self>
    where
        PS: IntoIterator,
        PS::Item: Into<ProtocolMeta>,
    {
        #[allow(clippy::mutable_key_type)]
        let mut dial_backoff = HashMap::with_capacity(config.dial.len());
        for d in &config.dial {
            let address: MultiAddr = d.parse().context("parse dial address")?;
            dial_backoff.insert(address, ExponentialBackoff::new(RECONNECT_BASE_DURATION));
        }
        let dial_vec: Vec<MultiAddr> = dial_backoff.keys().cloned().collect();
        let key_pair = if let Some(ref secret_key_path) = config.secret_key_path {
            let key = std::fs::read(secret_key_path).with_context(|| {
                format!(
                    "read secret key from file {}",
                    secret_key_path.to_string_lossy()
                )
            })?;
            SecioKeyPair::secp256k1_raw_key(key).context("read secret key")?
        } else {
            SecioKeyPair::secp256k1_generated()
        };
        let mut builder = ServiceBuilder::new()
            .forever(true)
            .tcp_config(|socket| {
                let sock_ref = SockRef::from(&socket);
                sock_ref.set_nodelay(true)?;
                Ok(socket)
            })
            .key_pair(key_pair);
        for p in protocols {
            builder = builder.insert_protocol(p.into());
        }
        let allowed_peer_ids = if let Some(ref allowed) = config.allowed_peer_ids {
            let mut allowed_peer_ids = HashSet::new();
            for a in allowed {
                allowed_peer_ids.insert(
                    a.parse()
                        .with_context(|| format!("parse allowed peer id {}", a))?,
                );
            }
            for d in dial_backoff.keys() {
                if let Some(a) = extract_peer_id(d) {
                    allowed_peer_ids.insert(a);
                }
            }
            Some(allowed_peer_ids)
        } else {
            None
        };
        let mut service = builder.build(SHandle {
            dial_backoff,
            allowed_peer_ids,
        });
        let control = service.control().clone();
        // Send dial in another task to avoid deadlock.
        if !dial_vec.is_empty() {
            tokio::spawn(async move {
                for address in dial_vec {
                    // This sends the dial request through an async channel, err means the channel is closed.
                    log::info!("dial {}", address);
                    if control.dial(address, TargetProtocol::All).await.is_err() {
                        break;
                    }
                }
            });
        }
        // Listen must succeed.
        if let Some(listen) = config.listen.as_deref() {
            service
                .listen(listen.parse().context("parse listen address")?)
                .await
                .context("listen")?;
        }
        Ok(Self { service })
    }

    pub fn control(&self) -> &ServiceAsyncControl {
        self.service.control()
    }

    pub async fn run(&mut self) {
        self.service.run().await;
    }
}

// Implement ServiceHandle to handle tentacle events.
struct SHandle {
    allowed_peer_ids: Option<HashSet<PeerId>>,
    dial_backoff: HashMap<MultiAddr, ExponentialBackoff>,
}

impl SHandle {
    fn re_dial(&mut self, context: &ServiceContext, address: MultiAddr) {
        let address_without_peer_id: MultiAddr = address
            .iter()
            .take_while(|x| !matches!(x, Protocol::P2P(_)))
            .collect();
        let entry = match self.dial_backoff.entry(address) {
            Entry::Vacant(_) => self.dial_backoff.entry(address_without_peer_id),
            e => e,
        };
        if let Entry::Occupied(mut o) = entry {
            let dial = o.key().clone();
            let backoff = o.get_mut();
            let sleep = backoff.next_sleep();
            // Reconnect in a newly spawned task so that we don't block the whole tentacle service.
            let control = context.control().clone();
            tokio::spawn(async move {
                tokio::time::sleep(sleep).await;
                log::info!("dial {}", dial);
                let _ = control.dial(dial, TargetProtocol::All).await;
            });
        }
    }

    fn reset(&mut self, address: MultiAddr) {
        let address_without_peer_id: MultiAddr = address
            .iter()
            .take_while(|x| !matches!(x, Protocol::P2P(_)))
            .collect();
        let entry = match self.dial_backoff.entry(address) {
            Entry::Vacant(_) => self.dial_backoff.entry(address_without_peer_id),
            e => e,
        };
        if let Entry::Occupied(mut o) = entry {
            let backoff = o.get_mut();
            backoff.reset();
        }
    }
}

#[async_trait]
impl ServiceHandle for SHandle {
    // A lot of internal error events will be output here, but not all errors need to close the service,
    // some just tell users that they need to pay attention
    async fn handle_error(&mut self, context: &mut ServiceContext, error: ServiceError) {
        log::info!("service error: {:?}", error);
        if let ServiceError::DialerError { address, error: _ } = error {
            self.re_dial(context, address);
        }
    }

    async fn handle_event(&mut self, context: &mut ServiceContext, event: ServiceEvent) {
        log::info!("service event: {:?}", event);
        match event {
            ServiceEvent::SessionClose { session_context } => {
                self.re_dial(context, session_context.address.clone());
            }
            ServiceEvent::SessionOpen { session_context } => {
                // Check allow list.
                let mut allow = true;
                if let Some(ref allowed) = self.allowed_peer_ids {
                    if let Some(peer_id) = extract_peer_id(&session_context.address) {
                        if !allowed.contains(&peer_id) {
                            allow = false;
                        }
                    } else {
                        allow = false;
                    }
                };
                if !allow {
                    let _ = context.control().disconnect(session_context.id).await;
                } else {
                    self.reset(session_context.address.clone());
                }
            }
            _ => (),
        }
    }
}

/// ProtocolSpawn helper.
pub struct FnSpawn<F: Fn(Arc<SessionContext>, &ServiceAsyncControl, SubstreamReadPart)>(pub F);

impl<F: Fn(Arc<SessionContext>, &ServiceAsyncControl, SubstreamReadPart)> ProtocolSpawn
    for FnSpawn<F>
{
    fn spawn(
        &self,
        context: Arc<SessionContext>,
        control: &ServiceAsyncControl,
        read_part: SubstreamReadPart,
    ) {
        self.0(context, control, read_part);
    }
}

// Protocol registry: all p2p protocols should be declared here.

pub const P2P_MEM_BLOCK_SYNC_PROTOCOL: ProtocolId = ProtocolId::new(1);
pub const P2P_MEM_BLOCK_SYNC_PROTOCOL_NAME: &str = "/p2p/mem_block_sync";

pub const P2P_BLOCK_SYNC_PROTOCOL: ProtocolId = ProtocolId::new(2);
pub const P2P_BLOCK_SYNC_PROTOCOL_NAME: &str = "/p2p/block_sync";
