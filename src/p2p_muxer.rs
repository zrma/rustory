use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

pub(crate) fn config() -> libp2p_yamux::Config {
    libp2p_yamux::Config::default()
}

pub(crate) fn log_negotiated(relayed: bool) {
    eprintln!("p2p transport: muxer=yamux relayed={relayed}");
}

// transport 협상 성공 누계다. Swarm 수락 여부나 활성 연결 수와는 구분한다.
#[derive(Clone, Default)]
pub(crate) struct NegotiationStats(Arc<AtomicU64>);

impl NegotiationStats {
    pub(crate) fn record_yamux(&self) {
        let _ = self
            .0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                Some(value.saturating_add(1))
            });
    }

    pub(crate) fn yamux_total(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
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
        upgrade::{
            InboundConnectionUpgrade, OutboundConnectionUpgrade, SelectUpgrade, UpgradeInfo,
            Version,
        },
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

    // legacy의 협상 이름만 제공한다. 교집합이 없으므로 payload upgrade는 호출되면 안 된다.
    #[derive(Clone)]
    struct LegacyProtocol;

    impl UpgradeInfo for LegacyProtocol {
        type Info = &'static str;
        type InfoIter = std::iter::Once<Self::Info>;
        fn protocol_info(&self) -> Self::InfoIter {
            std::iter::once("/mplex/6.7.0")
        }
    }

    impl<C: futures::AsyncRead + futures::AsyncWrite + Send + Unpin + 'static>
        InboundConnectionUpgrade<C> for LegacyProtocol
    {
        type Output = libp2p_yamux::Muxer<C>;
        type Error = std::io::Error;
        type Future = futures::future::Ready<Result<Self::Output, Self::Error>>;
        fn upgrade_inbound(self, _: C, _: Self::Info) -> Self::Future {
            panic!("unsupported legacy protocol was negotiated")
        }
    }

    impl<C: futures::AsyncRead + futures::AsyncWrite + Send + Unpin + 'static>
        OutboundConnectionUpgrade<C> for LegacyProtocol
    {
        type Output = libp2p_yamux::Muxer<C>;
        type Error = std::io::Error;
        type Future = futures::future::Ready<Result<Self::Output, Self::Error>>;
        fn upgrade_outbound(self, _: C, _: Self::Info) -> Self::Future {
            panic!("unsupported legacy protocol was negotiated")
        }
    }

    fn test_transport(
        mode: Mode,
        observed: Observed,
        stats: NegotiationStats,
    ) -> Boxed<(PeerId, StreamMuxerBox)> {
        let identity = libp2p::identity::Keypair::generate_ed25519();
        let pnet = libp2p::pnet::PnetConfig::new(libp2p::pnet::PreSharedKey::new([1; 32]));
        let transport = libp2p::tcp::tokio::Transport::default()
            .and_then(move |socket, _| pnet.handshake(socket))
            .upgrade(Version::V1)
            .authenticate(libp2p::noise::Config::new(&identity).unwrap());
        let transport = match mode {
            Mode::Transition => transport
                .multiplex(SelectUpgrade::new(config(), LegacyProtocol))
                .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
                .boxed(),
            Mode::Legacy => transport
                .multiplex(LegacyProtocol)
                .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
                .boxed(),
            Mode::YamuxOnly => transport
                .multiplex(config())
                .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
                .boxed(),
        };
        Transport::map(transport, move |result, _| {
            stats.record_yamux();
            observed.lock().unwrap().push("yamux");
            result
        })
        .boxed()
    }

    async fn negotiate(dialer_mode: Mode, listener_mode: Mode, succeeds: bool) {
        let observed = Observed::default();
        let stats = NegotiationStats::default();
        let mut listener = test_transport(listener_mode, observed.clone(), stats.clone());
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
        let mut dialer = test_transport(dialer_mode, observed.clone(), stats.clone());
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
                    return upgrade.await;
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            let (outbound, inbound) = futures::join!(dialing, accepting);
            if succeeds {
                assert_ne!(outbound.unwrap().0, inbound.unwrap().0);
                assert_eq!(*observed.lock().unwrap(), vec!["yamux", "yamux"]);
                assert_eq!(stats.yamux_total(), 2);
            } else {
                assert!(outbound.is_err());
                assert!(inbound.is_err());
                assert!(observed.lock().unwrap().is_empty());
                assert_eq!(stats.yamux_total(), 0);
            }
        })
        .await
        .expect("multiplexer negotiation timed out");
    }

    #[test]
    fn muxer_only_advertises_yamux() {
        assert_eq!(
            config().protocol_info().collect::<Vec<_>>(),
            ["/yamux/1.0.0"]
        );
    }

    #[tokio::test]
    async fn muxer_yamux_peers_negotiate() {
        negotiate(Mode::YamuxOnly, Mode::YamuxOnly, true).await;
    }

    #[tokio::test]
    async fn muxer_transition_peers_connect_in_both_directions() {
        negotiate(Mode::Transition, Mode::YamuxOnly, true).await;
        negotiate(Mode::YamuxOnly, Mode::Transition, true).await;
    }

    #[tokio::test]
    async fn muxer_legacy_peers_are_rejected_in_both_directions() {
        negotiate(Mode::Legacy, Mode::YamuxOnly, false).await;
        negotiate(Mode::YamuxOnly, Mode::Legacy, false).await;
    }
}
