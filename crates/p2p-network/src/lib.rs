use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use gw_config::P2PNetworkConfig;
use gw_utils::exponential_backoff::ExponentialBackoff;
use socket2::SockRef;
use tentacle::{
    async_trait,
    builder::ServiceBuilder,
    context::{ServiceContext, SessionContext},
    multiaddr::{MultiAddr, Protocol},
    secio::SecioKeyPair,
    service::{
        ProtocolMeta, Service, ServiceAsyncControl, ServiceError, ServiceEvent, TargetProtocol,
    },
    traits::{ProtocolSpawn, ServiceHandle},
    ProtocolId, SubstreamReadPart,
};

const RECONNECT_BASE_DURATION: Duration = Duration::from_secs(2);

/// Wrapper for tentacle Service. Automatcially reconnect dial addresses.
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
        let mut builder = ServiceBuilder::new()
            .forever(true)
            .tcp_config(|socket| {
                let sock_ref = SockRef::from(&socket);
                sock_ref.set_nodelay(true)?;
                Ok(socket)
            })
            // TODO: allow config keypair.
            .key_pair(SecioKeyPair::secp256k1_generated());
        for p in protocols {
            builder = builder.insert_protocol(p.into());
        }
        let mut service = builder.build(SHandle { dial_backoff });
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
    dial_backoff: HashMap<MultiAddr, ExponentialBackoff>,
}

#[async_trait]
impl ServiceHandle for SHandle {
    // A lot of internal error events will be output here, but not all errors need to close the service,
    // some just tell users that they need to pay attention
    async fn handle_error(&mut self, context: &mut ServiceContext, error: ServiceError) {
        log::info!("service error: {:?}", error);
        if let ServiceError::DialerError { address, error: _ } = error {
            if let Some(backoff) = self.dial_backoff.get_mut(&address) {
                let sleep = backoff.next_sleep();
                // Reconnect in a newly spawned task so that we don't block the whole tentacle service.
                let control = context.control().clone();
                tokio::spawn(async move {
                    tokio::time::sleep(sleep).await;
                    log::info!("dial {}", address);
                    let _ = control.dial(address, TargetProtocol::All).await;
                });
            }
        }
    }

    async fn handle_event(&mut self, context: &mut ServiceContext, event: ServiceEvent) {
        log::info!("service event: {:?}", event);
        match event {
            ServiceEvent::SessionClose { session_context } => {
                // session_context.address is like /ip4/127.0.0.1/tcp/32874/p2p/QmaFyRtib8rAULAq8tZEnFj2XcoLjtNPpymJmUZXxP3Z1k, we want to keep only stuff before /p2p.
                let address = session_context
                    .address
                    .iter()
                    .take_while(|x| !matches!(x, Protocol::P2P(_)))
                    .collect();
                if let Some(backoff) = self.dial_backoff.get_mut(&address) {
                    let sleep = backoff.next_sleep();
                    let control = context.control().clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(sleep).await;
                        log::info!("dial {}", address);
                        let _ = control.dial(address, TargetProtocol::All).await;
                    });
                }
            }
            ServiceEvent::SessionOpen { session_context } => {
                let address = session_context
                    .address
                    .iter()
                    .take_while(|x| !matches!(x, Protocol::P2P(_)))
                    .collect();
                if let Some(backoff) = self.dial_backoff.get_mut(&address) {
                    backoff.reset();
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
