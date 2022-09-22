//! To be run manually to test auto-reconnect.

use bytes::Bytes;
use futures_util::StreamExt;
use gw_config::P2PNetworkConfig;
use gw_p2p_network::*;
use tentacle::{builder::MetaBuilder, multiaddr::Protocol, service::ProtocolMeta, ProtocolId};

const PROTOCOL_ZERO: ProtocolId = ProtocolId::new(0);

fn protocol() -> ProtocolMeta {
    MetaBuilder::new()
        .id(PROTOCOL_ZERO)
        .protocol_spawn(FnSpawn(|ctx, control, mut read| {
            let control = control.clone();
            tokio::spawn(async move {
                let _ = control
                    .send_message_to(ctx.id, PROTOCOL_ZERO, Bytes::from_static(b"hello!"))
                    .await;
                while let Some(Ok(msg)) = read.next().await {
                    log::info!(
                        "{:?}",
                        ctx.address
                            .iter()
                            .find(|p| matches!(p, Protocol::Ip4(_) | Protocol::Ip6(_)))
                    );
                    log::info!("received from {}: {:?}", ctx.id, msg);
                }
            });
        }))
        .build()
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let is_server = std::env::args().nth(1).as_deref() == Some("server");
    let config = if is_server {
        P2PNetworkConfig {
            listen: Some("/ip6/::1/tcp/32874".into()),
            dial: Vec::new(),
            secret_key_path: Some("examples/server-key".into()),
            allowed_peer_ids: Some(vec!["Qme22rAhVjej4UCYxzW52L8PtYVv3XHeY2JqRKuwJn5ZFQ".into()]),
        }
    } else {
        P2PNetworkConfig {
            listen: None,
            dial: vec![
                "/ip6/::1/tcp/32874/p2p/QmPM86hUFFsc5c5Twuux7yaW2PdziwRrmbThGZec13veQ1".into(),
            ],
            secret_key_path: Some("examples/client-key".into()),
            allowed_peer_ids: None,
        }
    };
    let mut network = P2PNetwork::init(&config, [protocol()]).await?;
    network.run().await;

    Ok(())
}
