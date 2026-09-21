use futures::future::Either;
use libp2p_core::upgrade::SelectUpgrade;

// 전환 기간에는 Yamux를 먼저 제안하고 기존 mplex 전용 peer와의 연결을 유지한다.
pub(crate) fn config() -> SelectUpgrade<libp2p_yamux::Config, libp2p_mplex::Config> {
    SelectUpgrade::new(
        libp2p_yamux::Config::default(),
        libp2p_mplex::Config::default(),
    )
}

pub(crate) fn log_negotiated<A, B>(muxer: &Either<A, B>, relayed: bool) {
    let protocol = match muxer {
        Either::Left(_) => "yamux",
        Either::Right(_) => "mplex",
    };
    eprintln!("p2p transport: muxer={protocol} relayed={relayed}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::libp2p;
    use futures::StreamExt;
    use libp2p_core::{
        Endpoint, PeerId, Transport,
        muxing::StreamMuxerBox,
        transport::{Boxed, DialOpts, ListenerId, PortUse, TransportEvent},
        upgrade::{UpgradeInfo, Version},
    };
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[derive(Clone, Copy)]
    enum Mode {
        Transition,
        Legacy,
        YamuxOnly,
    }

    type Observed = Arc<Mutex<Vec<&'static str>>>;

    fn test_transport(mode: Mode, observed: Observed) -> Boxed<(PeerId, StreamMuxerBox)> {
        let identity = libp2p::identity::Keypair::generate_ed25519();
        let pnet = libp2p::pnet::PnetConfig::new(libp2p::pnet::PreSharedKey::new([1; 32]));
        let transport = libp2p::tcp::tokio::Transport::default()
            .and_then(move |socket, _| pnet.handshake(socket))
            .upgrade(Version::V1)
            .authenticate(libp2p::noise::Config::new(&identity).unwrap());
        match mode {
            Mode::Transition => transport
                .multiplex(config())
                .map(move |(peer, muxer), _| {
                    observed.lock().unwrap().push(match &muxer {
                        Either::Left(_) => "yamux",
                        Either::Right(_) => "mplex",
                    });
                    (peer, StreamMuxerBox::new(muxer))
                })
                .boxed(),
            Mode::Legacy => transport
                .multiplex(libp2p_mplex::Config::default())
                .map(move |(peer, muxer), _| {
                    observed.lock().unwrap().push("mplex");
                    (peer, StreamMuxerBox::new(muxer))
                })
                .boxed(),
            Mode::YamuxOnly => transport
                .multiplex(libp2p_yamux::Config::default())
                .map(move |(peer, muxer), _| {
                    observed.lock().unwrap().push("yamux");
                    (peer, StreamMuxerBox::new(muxer))
                })
                .boxed(),
        }
    }

    async fn negotiate(dialer_mode: Mode, listener_mode: Mode, expected: &'static str) {
        let observed = Observed::default();
        let mut listener = test_transport(listener_mode, observed.clone());
        listener
            .listen_on(ListenerId::next(), "/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();
        let address = loop {
            if let TransportEvent::NewAddress { listen_addr, .. } =
                listener.select_next_some().await
            {
                break listen_addr;
            }
        };
        let mut dialer = test_transport(dialer_mode, observed.clone());
        let dialing = dialer
            .dial(
                address,
                DialOpts {
                    role: Endpoint::Dialer,
                    port_use: PortUse::Reuse,
                },
            )
            .unwrap();
        let accepting = async {
            loop {
                if let TransportEvent::Incoming { upgrade, .. } = listener.select_next_some().await
                {
                    return upgrade.await.unwrap();
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            let (outbound, inbound) = futures::join!(dialing, accepting);
            let outbound = outbound.unwrap();
            assert_ne!(outbound.0, inbound.0);
            assert_eq!(*observed.lock().unwrap(), vec![expected, expected]);
        })
        .await
        .expect("multiplexer negotiation timed out");
    }

    #[test]
    fn muxer_protocols_prefer_yamux() {
        let protocols: Vec<_> = config()
            .protocol_info()
            .map(|p| AsRef::<str>::as_ref(&p).to_owned())
            .collect();
        assert_eq!(protocols, ["/yamux/1.0.0", "/mplex/6.7.0"]);
    }

    #[tokio::test]
    async fn muxer_transition_peers_negotiate_yamux() {
        negotiate(Mode::Transition, Mode::Transition, "yamux").await;
    }

    #[tokio::test]
    async fn muxer_legacy_peers_connect_in_both_directions() {
        negotiate(Mode::Legacy, Mode::Transition, "mplex").await;
        negotiate(Mode::Transition, Mode::Legacy, "mplex").await;
    }

    #[tokio::test]
    async fn muxer_yamux_only_peers_connect_in_both_directions() {
        negotiate(Mode::YamuxOnly, Mode::Transition, "yamux").await;
        negotiate(Mode::Transition, Mode::YamuxOnly, "yamux").await;
    }
}
