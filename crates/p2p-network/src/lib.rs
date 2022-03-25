use std::{collections::HashSet, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use gw_config::P2PNetworkConfig;
use socket2::{SockRef, TcpKeepalive};
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
    SubstreamReadPart,
};

const RECONNECT_DURATION: Duration = Duration::from_secs(5);

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
        let mut dial = HashSet::with_capacity(config.dial.len());
        for d in &config.dial {
            let address: MultiAddr = d.parse().context("parse dial address")?;
            dial.insert(address);
        }
        let dial_vec: Vec<MultiAddr> = dial.iter().cloned().collect();
        let mut builder = ServiceBuilder::new()
            .forever(true)
            .tcp_config(|socket| {
                let sock_ref = SockRef::from(&socket);
                sock_ref.set_reuse_address(true)?;
                sock_ref.set_nodelay(true)?;
                sock_ref.set_tcp_keepalive(
                    &TcpKeepalive::new()
                        .with_interval(Duration::from_secs(15))
                        .with_time(Duration::from_secs(5))
                        .with_retries(3),
                )?;
                Ok(socket)
            })
            // TODO: allow config keypair.
            .key_pair(SecioKeyPair::secp256k1_generated());
        for p in protocols {
            builder = builder.insert_protocol(p.into());
        }
        let mut service = builder.build(SHandle { dial });
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
    dial: HashSet<MultiAddr>,
}

#[async_trait]
impl ServiceHandle for SHandle {
    // A lot of internal error events will be output here, but not all errors need to close the service,
    // some just tell users that they need to pay attention
    async fn handle_error(&mut self, context: &mut ServiceContext, error: ServiceError) {
        log::info!("service error: {:?}", error);
        // Reconnect.
        if let ServiceError::DialerError { address, error: _ } = error {
            tokio::time::sleep(RECONNECT_DURATION).await;
            log::info!("dial {}", address);
            let _ = context.dial(address, TargetProtocol::All).await;
        }
    }

    async fn handle_event(&mut self, context: &mut ServiceContext, event: ServiceEvent) {
        log::info!("service event: {:?}", event);
        if let ServiceEvent::SessionClose { session_context } = event {
            // session_context.address is like /ip4/127.0.0.1/tcp/32874/p2p/QmaFyRtib8rAULAq8tZEnFj2XcoLjtNPpymJmUZXxP3Z1k, we want to keep only stuff before /p2p.
            let address = session_context
                .address
                .iter()
                .take_while(|x| !matches!(x, Protocol::P2P(_)))
                .collect();
            if self.dial.contains(&address) {
                // Reconnect.
                tokio::time::sleep(RECONNECT_DURATION).await;
                log::info!("dial {}", address);
                let _ = context.dial(address, TargetProtocol::All).await;
            }
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
