use crate::libp2p;
use crate::libp2p::core::transport::choice::OrTransport;
use crate::libp2p::core::upgrade::Version;
use crate::libp2p::swarm::{SwarmEvent, dial_opts::DialOpts};
use crate::libp2p::{Multiaddr, PeerId, StreamProtocol, Swarm, Transport};
use crate::storage::{LocalStore, PeerBookPeer, PullBatch};
use anyhow::{Context, Result};
use futures::StreamExt;
use libp2p_request_response::ProtocolSupport;
use multiaddr::Protocol;
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, ToSocketAddrs};
use std::time::Duration;
use time::OffsetDateTime;

const SYNC_PULL_PROTOCOL_PLAIN: &str = "/rustory/sync-pull/1.0.0";
const SYNC_PULL_PROTOCOL_ZSTD: &str = "/rustory/sync-pull/1.0.1";
const ENTRIES_PUSH_PROTOCOL_PLAIN: &str = "/rustory/entries-push/1.0.0";
const ENTRIES_PUSH_PROTOCOL_ZSTD: &str = "/rustory/entries-push/1.0.1";

// request-response는 stream EOF까지 읽기 때문에, 크기 상한을 너무 작게 잡으면 “잘린 JSON 파싱 실패”로 보이기 쉽다.
// PoC/MVP 범위에서는 "상한을 넉넉히" + "명확한 too-large 에러"를 우선한다.
const PULL_REQ_MAX_BYTES: u64 = 64 * 1024;
const PULL_RESP_MAX_BYTES: u64 = 32 * 1024 * 1024;
const PUSH_REQ_MAX_BYTES: u64 = 16 * 1024 * 1024;
const PUSH_RESP_MAX_BYTES: u64 = 64 * 1024;

// zstd 프로토콜에서는 "wire 상한"과 별개로 decode(압축 해제 후 JSON bytes) 상한을 둔다.
const DECODED_MAX_MULTIPLIER: u64 = 4;
const PULL_REQ_DECODED_MAX_BYTES: u64 = PULL_REQ_MAX_BYTES * DECODED_MAX_MULTIPLIER;
const PULL_RESP_DECODED_MAX_BYTES: u64 = PULL_RESP_MAX_BYTES * DECODED_MAX_MULTIPLIER;
const PUSH_REQ_DECODED_MAX_BYTES: u64 = PUSH_REQ_MAX_BYTES * DECODED_MAX_MULTIPLIER;
const PUSH_RESP_DECODED_MAX_BYTES: u64 = PUSH_RESP_MAX_BYTES * DECODED_MAX_MULTIPLIER;

pub const DEFAULT_RELAY_MAX_RESERVATIONS: usize = 512;
pub const DEFAULT_RELAY_MAX_RESERVATIONS_PER_PEER: usize = 64;
pub const DEFAULT_RELAY_MAX_CIRCUITS: usize = 256;
pub const DEFAULT_RELAY_MAX_CIRCUITS_PER_PEER: usize = 64;
pub const DEFAULT_RELAY_MAX_CIRCUIT_DURATION_SEC: u64 = 15 * 60;
pub const DEFAULT_RELAY_MAX_CIRCUIT_BYTES: u64 = 64 * 1024 * 1024;

// request-response behaviour 내부 timeout은 request 상태 추적/정리를 위한 용도다.
// pull/push는 attempt별 timeout을 별도로 구현하므로, 여기서 너무 작은 값을 두면
// 사용자가 `--req-timeout-cap-sec` 등을 크게 잡았을 때 내부 Timeout이 먼저 터질 수 있다.
// 따라서 "충분히 큰 값"으로 두고, 실제 attempt timeout은 클라이언트 로직에서 결정한다.
const REQUEST_RESPONSE_INTERNAL_TIMEOUT: Duration = Duration::from_secs(60 * 60);

#[derive(Clone)]
pub struct ServeConfig {
    pub identity: libp2p::identity::Keypair,
    pub psk: libp2p::pnet::PreSharedKey,
    pub relay_addr: Option<Multiaddr>,
    pub trackers: Vec<String>,
    pub tracker_token: Option<String>,
    pub meta: crate::tracker::PeerMeta,
}

#[derive(Clone)]
pub struct SyncConfig {
    pub identity: libp2p::identity::Keypair,
    pub psk: libp2p::pnet::PreSharedKey,
    pub relay_addr: Option<Multiaddr>,
    pub trackers: Vec<String>,
    pub tracker_token: Option<String>,
    pub user_id: Option<String>,
    pub device_id: Option<String>,
    pub request_retry_policy: RequestRetryPolicy,
    pub max_peers_per_tick: usize,
}

#[derive(Debug, Clone)]
pub struct RequestRetryPolicy {
    pub attempts: usize,
    pub timeout_base: Duration,
    pub timeout_cap: Duration,
    pub backoff_base: Duration,
}

impl Default for RequestRetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 3,
            timeout_base: Duration::from_secs(5),
            timeout_cap: Duration::from_secs(30),
            backoff_base: Duration::from_millis(200),
        }
    }
}

#[derive(Clone)]
pub struct RelayServeConfig {
    pub identity: libp2p::identity::Keypair,
    pub psk: libp2p::pnet::PreSharedKey,
    pub limits: RelayLimits,
}

#[derive(Debug, Clone, Copy)]
pub struct RelayLimits {
    pub max_reservations: usize,
    pub max_reservations_per_peer: usize,
    pub max_circuits: usize,
    pub max_circuits_per_peer: usize,
    pub max_circuit_duration: Duration,
    pub max_circuit_bytes: u64,
    pub rate_limits: bool,
}

impl Default for RelayLimits {
    fn default() -> Self {
        Self {
            max_reservations: DEFAULT_RELAY_MAX_RESERVATIONS,
            max_reservations_per_peer: DEFAULT_RELAY_MAX_RESERVATIONS_PER_PEER,
            max_circuits: DEFAULT_RELAY_MAX_CIRCUITS,
            max_circuits_per_peer: DEFAULT_RELAY_MAX_CIRCUITS_PER_PEER,
            max_circuit_duration: Duration::from_secs(DEFAULT_RELAY_MAX_CIRCUIT_DURATION_SEC),
            max_circuit_bytes: DEFAULT_RELAY_MAX_CIRCUIT_BYTES,
            rate_limits: false,
        }
    }
}

impl RelayLimits {
    fn to_libp2p_config(self) -> Result<libp2p::relay::Config> {
        if self.max_reservations == 0 {
            anyhow::bail!("max_reservations must be greater than 0");
        }
        if self.max_reservations_per_peer == 0 {
            anyhow::bail!("max_reservations_per_peer must be greater than 0");
        }
        if self.max_circuits == 0 {
            anyhow::bail!("max_circuits must be greater than 0");
        }
        if self.max_circuits_per_peer == 0 {
            anyhow::bail!("max_circuits_per_peer must be greater than 0");
        }

        let mut cfg = libp2p::relay::Config {
            max_reservations: self.max_reservations,
            max_reservations_per_peer: self.max_reservations_per_peer,
            max_circuits: self.max_circuits,
            max_circuits_per_peer: self.max_circuits_per_peer,
            max_circuit_duration: self.max_circuit_duration,
            max_circuit_bytes: self.max_circuit_bytes,
            ..libp2p::relay::Config::default()
        };

        if !self.rate_limits {
            cfg.reservation_rate_limiters.clear();
            cfg.circuit_src_rate_limiters.clear();
        }

        Ok(cfg)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SyncPull {
    cursor: i64,
    limit: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SyncBatch {
    entries: Vec<crate::core::Entry>,
    next_cursor: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct EntriesPush {
    entries: Vec<crate::core::Entry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PushAck {
    ok: bool,
    inserted: Option<usize>,
    ignored: Option<usize>,
}

#[derive(Debug, Clone)]
struct AuthorizedPeer {
    peer_id: String,
    user_id: Option<String>,
    device_id: Option<String>,
}

#[derive(libp2p_swarm::NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct RustoryBehaviour {
    relay: libp2p::relay::client::Behaviour,
    identify: libp2p::identify::Behaviour,
    dcutr: libp2p::dcutr::Behaviour,
    ping: libp2p::ping::Behaviour,
    sync: libp2p_request_response::Behaviour<crate::p2p_codec::JsonCodec<SyncPull, SyncBatch>>,
    push: libp2p_request_response::Behaviour<crate::p2p_codec::JsonCodec<EntriesPush, PushAck>>,
}

#[derive(libp2p_swarm::NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct RelayServerBehaviour {
    relay: libp2p::relay::Behaviour,
    identify: libp2p::identify::Behaviour,
    ping: libp2p::ping::Behaviour,
}

#[cfg(test)]
fn build_rustory_swarm(psk: libp2p::pnet::PreSharedKey) -> Result<Swarm<RustoryBehaviour>> {
    let identity = libp2p::identity::Keypair::generate_ed25519();
    build_rustory_swarm_with_identity(identity, psk)
}

fn build_rustory_swarm_with_identity(
    identity: libp2p::identity::Keypair,
    psk: libp2p::pnet::PreSharedKey,
) -> Result<Swarm<RustoryBehaviour>> {
    let local_public_key = identity.public();
    let local_peer_id = local_public_key.to_peer_id();

    // 양쪽이 지원하면 zstd(1.0.1)를 우선 선택한다. 구버전은 plain(1.0.0)으로 폴백.
    let protocols = [
        (
            StreamProtocol::new(SYNC_PULL_PROTOCOL_ZSTD),
            ProtocolSupport::Full,
        ),
        (
            StreamProtocol::new(SYNC_PULL_PROTOCOL_PLAIN),
            ProtocolSupport::Full,
        ),
    ];

    let rr_cfg = libp2p_request_response::Config::default()
        .with_request_timeout(REQUEST_RESPONSE_INTERNAL_TIMEOUT);
    let rr_codec = crate::p2p_codec::JsonCodec::<SyncPull, SyncBatch>::new(
        PULL_REQ_MAX_BYTES,
        PULL_RESP_MAX_BYTES,
    )
    .with_decoded_maximum(PULL_REQ_DECODED_MAX_BYTES, PULL_RESP_DECODED_MAX_BYTES);
    let rr = libp2p_request_response::Behaviour::with_codec(rr_codec, protocols, rr_cfg);

    // 양쪽이 지원하면 zstd(1.0.1)를 우선 선택한다. 구버전은 plain(1.0.0)으로 폴백.
    let push_protocols = [
        (
            StreamProtocol::new(ENTRIES_PUSH_PROTOCOL_ZSTD),
            ProtocolSupport::Full,
        ),
        (
            StreamProtocol::new(ENTRIES_PUSH_PROTOCOL_PLAIN),
            ProtocolSupport::Full,
        ),
    ];
    let push_cfg = libp2p_request_response::Config::default()
        .with_request_timeout(REQUEST_RESPONSE_INTERNAL_TIMEOUT);
    let push_codec = crate::p2p_codec::JsonCodec::<EntriesPush, PushAck>::new(
        PUSH_REQ_MAX_BYTES,
        PUSH_RESP_MAX_BYTES,
    )
    .with_decoded_maximum(PUSH_REQ_DECODED_MAX_BYTES, PUSH_RESP_DECODED_MAX_BYTES);
    let push_rr =
        libp2p_request_response::Behaviour::with_codec(push_codec, push_protocols, push_cfg);

    let (relay_transport, relay_behaviour) = libp2p::relay::client::new(local_peer_id);
    let tcp_transport = libp2p::tcp::tokio::Transport::default();
    let transport = OrTransport::new(relay_transport, tcp_transport);

    let pnet = libp2p::pnet::PnetConfig::new(psk);
    let transport = transport.and_then(move |socket, _| pnet.handshake(socket));

    let noise_cfg = libp2p::noise::Config::new(&identity).context("noise config")?;
    let transport = transport
        .upgrade(Version::V1)
        .authenticate(noise_cfg)
        .multiplex(libp2p_mplex::Config::default())
        .boxed();

    let identify_cfg = libp2p::identify::Config::new(
        format!("rustory/{}", crate::build_info::VERSION),
        local_public_key,
    )
    .with_agent_version(format!("rustory/{}", crate::build_info::VERSION_DISPLAY))
    // Rustory 실사용 동기화는 tracker + relay를 우선한다. 임시 local listen 주소를
    // peer에게 광고하면 DCUtR가 NAT/router 밖에서 의미 없는 loopback/private 후보
    // 다이얼을 반복할 수 있다.
    .with_hide_listen_addrs(true);

    let behaviour = RustoryBehaviour {
        relay: relay_behaviour,
        identify: libp2p::identify::Behaviour::new(identify_cfg),
        dcutr: libp2p::dcutr::Behaviour::new(local_peer_id),
        ping: libp2p::ping::Behaviour::new(libp2p::ping::Config::new()),
        sync: rr,
        push: push_rr,
    };

    Ok(Swarm::new(
        transport,
        behaviour,
        local_peer_id,
        libp2p::swarm::Config::with_tokio_executor(),
    ))
}

fn build_relay_swarm_with_identity(
    identity: libp2p::identity::Keypair,
    psk: libp2p::pnet::PreSharedKey,
    limits: RelayLimits,
) -> Result<Swarm<RelayServerBehaviour>> {
    let local_public_key = identity.public();
    let local_peer_id = identity.public().to_peer_id();

    let transport = libp2p::tcp::tokio::Transport::default();

    let pnet = libp2p::pnet::PnetConfig::new(psk);
    let transport = transport.and_then(move |socket, _| pnet.handshake(socket));

    let noise_cfg = libp2p::noise::Config::new(&identity).context("noise config")?;
    let transport = transport
        .upgrade(Version::V1)
        .authenticate(noise_cfg)
        .multiplex(libp2p_mplex::Config::default())
        .boxed();

    let identify_cfg = libp2p::identify::Config::new(
        format!("rustory-relay/{}", crate::build_info::VERSION),
        local_public_key,
    )
    .with_agent_version(format!("rustory/{}", crate::build_info::VERSION_DISPLAY));

    let behaviour = RelayServerBehaviour {
        relay: libp2p::relay::Behaviour::new(local_peer_id, limits.to_libp2p_config()?),
        identify: libp2p::identify::Behaviour::new(identify_cfg),
        ping: libp2p::ping::Behaviour::new(libp2p::ping::Config::new()),
    };

    Ok(Swarm::new(
        transport,
        behaviour,
        local_peer_id,
        libp2p::swarm::Config::with_tokio_executor(),
    ))
}

pub fn relay_serve(listen: &str, cfg: RelayServeConfig) -> Result<()> {
    let listen: Multiaddr = listen.parse().context("parse listen multiaddr")?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;

    rt.block_on(async move { relay_serve_async(listen, cfg).await })
}

async fn relay_serve_async(listen: Multiaddr, cfg: RelayServeConfig) -> Result<()> {
    let RelayServeConfig {
        identity,
        psk,
        limits,
    } = cfg;
    eprintln!(
        "relay config: max_reservations={} max_reservations_per_peer={} max_circuits={} max_circuits_per_peer={} max_circuit_duration_sec={} max_circuit_bytes={} rate_limits={}",
        limits.max_reservations,
        limits.max_reservations_per_peer,
        limits.max_circuits,
        limits.max_circuits_per_peer,
        limits.max_circuit_duration.as_secs(),
        limits.max_circuit_bytes,
        limits.rate_limits
    );
    let mut swarm = build_relay_swarm_with_identity(identity, psk, limits)?;
    swarm.listen_on(listen).context("listen_on")?;
    let local_peer_id = *swarm.local_peer_id();

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                // relay reservation 응답에는 "dial 가능한 relay 주소"가 최소 1개 필요하다.
                // Swarm의 external address set이 비어 있으면 클라이언트가 reservation을 유효하게
                // 처리하지 못해(relay spec상 최소 1개 필요) circuit이 항상 실패할 수 있다.
                //
                // PoC에서는 listen addr를 external addr로도 같이 등록해 단순하게 처리한다.
                swarm.add_external_address(address.clone());
                println!("relay listen: {}/p2p/{}", address, local_peer_id);
            }
            SwarmEvent::Behaviour(event) => match event {
                RelayServerBehaviourEvent::Relay(
                    libp2p::relay::Event::ReservationReqAccepted { src_peer_id, .. },
                ) => {
                    eprintln!("relay: reservation accepted: {src_peer_id}");
                }
                RelayServerBehaviourEvent::Relay(libp2p::relay::Event::CircuitReqAccepted {
                    src_peer_id,
                    dst_peer_id,
                }) => {
                    eprintln!("relay: circuit accepted: {src_peer_id} -> {dst_peer_id}");
                }
                _ => {}
            },
            _ => {}
        }
    }
}

pub fn serve(listen: &str, db_path: &str, cfg: ServeConfig) -> Result<()> {
    let listen: Multiaddr = listen.parse().context("parse listen multiaddr")?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;

    rt.block_on(async move { serve_async(listen, db_path, cfg).await })
}

async fn serve_async(listen: Multiaddr, db_path: &str, cfg: ServeConfig) -> Result<()> {
    let ServeConfig {
        identity,
        psk,
        relay_addr,
        trackers,
        tracker_token,
        meta,
    } = cfg;

    let store = LocalStore::open(db_path)?;
    let mut swarm = build_rustory_swarm_with_identity(identity, psk)?;

    swarm.listen_on(listen).context("listen_on")?;

    let mut relay_listener_id = None;
    if let Some(relay_addr) = relay_addr.clone() {
        let relay_listen = relay_circuit_listen_addr(&relay_addr)?;
        eprintln!("p2p relay listen requested: {relay_listen}");
        relay_listener_id = Some(swarm.listen_on(relay_listen).context("listen_on relay")?);
    }

    let local_peer_id = *swarm.local_peer_id();

    let trackers = trackers
        .into_iter()
        .map(|base_url| crate::tracker::TrackerClient::new(base_url, tracker_token.clone()))
        .collect::<Vec<_>>();

    let mut known_addrs: HashSet<String> = HashSet::new();
    let mut pending_pull_responses = HashMap::new();
    let mut next_register = tokio::time::interval(Duration::from_secs(30));

    loop {
        tokio::select! {
            _ = next_register.tick() => {
                if !trackers.is_empty() && !known_addrs.is_empty() {
                    spawn_register_all(trackers.clone(), local_peer_id, known_addrs.iter().cloned().collect(), meta.clone());
                }
            }
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        let Some(full) =
                            tracker_announce_addr_from_listen_addr(address, local_peer_id)
                        else {
                            continue;
                        };
                        println!("p2p listen: {}", full);
                        known_addrs.insert(full);

                        // 주소를 1개 이상 확보한 시점에 tracker에 즉시 등록한다.
                        if !trackers.is_empty() {
                            spawn_register_all(trackers.clone(), local_peer_id, known_addrs.iter().cloned().collect(), meta.clone());
                        }
                    }
                    SwarmEvent::NewExternalAddrCandidate { address } => {
                        let Some(full) =
                            dialable_tracker_addr_from_external_candidate(address, local_peer_id)
                        else {
                            continue;
                        };
                        if !known_addrs.insert(full.clone()) {
                            continue;
                        }

                        eprintln!("p2p external addr candidate: {full}");
                        if !trackers.is_empty() {
                            spawn_register_all(
                                trackers.clone(),
                                local_peer_id,
                                known_addrs.iter().cloned().collect(),
                                meta.clone(),
                            );
                        }
                    }
                    SwarmEvent::ExternalAddrConfirmed { address } => {
                        let Some(full) =
                            dialable_tracker_addr_from_external_candidate(address, local_peer_id)
                        else {
                            continue;
                        };
                        if !known_addrs.insert(full.clone()) {
                            continue;
                        }

                        eprintln!("p2p external addr confirmed: {full}");
                        if !trackers.is_empty() {
                            spawn_register_all(
                                trackers.clone(),
                                local_peer_id,
                                known_addrs.iter().cloned().collect(),
                                meta.clone(),
                            );
                        }
                    }
                    SwarmEvent::Behaviour(RustoryBehaviourEvent::Sync(event)) => match event {
                        libp2p_request_response::Event::Message { peer, message, .. } => match message {
                            libp2p_request_response::Message::Request {
                                request,
                                channel,
                                request_id,
                            } => {
                                if let Err(err) =
                                    authorize_inbound_peer(&store, peer, &meta, &trackers)
                                {
                                    eprintln!("warn: p2p pull rejected: peer={peer} error={err:#}");
                                    continue;
                                }

                                let batch = store.pull_since_cursor(
                                    request.cursor,
                                    clamp_remote_pull_limit(request.limit),
                                )?;
                                let resp = SyncBatch {
                                    entries: batch.entries,
                                    next_cursor: batch.next_cursor,
                                };
                                if let Some(next_cursor) = resp.next_cursor {
                                    pending_pull_responses.insert((peer, request_id), next_cursor);
                                }
                                if swarm
                                    .behaviour_mut()
                                    .sync
                                    .send_response(channel, resp)
                                    .is_err()
                                {
                                    pending_pull_responses.remove(&(peer, request_id));
                                }
                            }
                            libp2p_request_response::Message::Response { .. } => {}
                        },
                        libp2p_request_response::Event::OutboundFailure { .. } => {}
                        libp2p_request_response::Event::InboundFailure {
                            peer,
                            request_id,
                            ..
                        } => {
                            pending_pull_responses.remove(&(peer, request_id));
                        }
                        libp2p_request_response::Event::ResponseSent {
                            peer,
                            request_id,
                            ..
                        } => {
                            if let Some(next_cursor) =
                                pending_pull_responses.remove(&(peer, request_id))
                            {
                                let peer_key = peer.to_string();
                                if let Err(err) =
                                    store.advance_last_pushed_seq(&peer_key, next_cursor)
                                {
                                    eprintln!(
                                        "warn: p2p pull response cursor update failed: peer={peer} cursor={next_cursor} error={err:#}"
                                    );
                                }
                            }
                        }
                    },
                    SwarmEvent::Behaviour(RustoryBehaviourEvent::Push(event)) => match event {
                        libp2p_request_response::Event::Message { peer, message, .. } => match message {
                            libp2p_request_response::Message::Request { request, channel, .. } => {
                                let resp = match authorize_inbound_peer(
                                    &store,
                                    peer,
                                    &meta,
                                    &trackers,
                                )
                                .and_then(|authorized| {
                                    validate_push_provenance(&request.entries, &authorized)?;
                                    store.insert_entries_with_stats(&request.entries)
                                }) {
                                    Ok(stats) => PushAck {
                                        ok: true,
                                        inserted: Some(stats.inserted),
                                        ignored: Some(stats.ignored),
                                    },
                                    Err(err) => {
                                        eprintln!("warn: p2p push rejected: peer={peer} error={err:#}");
                                        PushAck {
                                            ok: false,
                                            inserted: None,
                                            ignored: None,
                                        }
                                    }
                                };

                                let _ = swarm.behaviour_mut().push.send_response(channel, resp);
                            }
                            libp2p_request_response::Message::Response { .. } => {}
                        },
                        libp2p_request_response::Event::OutboundFailure { .. } => {}
                        libp2p_request_response::Event::InboundFailure { .. } => {}
                        libp2p_request_response::Event::ResponseSent { .. } => {}
                    },
                    SwarmEvent::Behaviour(RustoryBehaviourEvent::Dcutr(event)) => {
                        log_dcutr_event(&event);
                    }
                    SwarmEvent::Behaviour(RustoryBehaviourEvent::Relay(event)) => match event {
                        libp2p::relay::client::Event::ReservationReqAccepted {
                            relay_peer_id,
                            renewal,
                            ..
                        } => {
                            eprintln!(
                                "p2p relay reservation accepted: relay={relay_peer_id} renewal={renewal}"
                            );
                        }
                        libp2p::relay::client::Event::OutboundCircuitEstablished {
                            relay_peer_id,
                            ..
                        } => {
                            eprintln!("p2p relay outbound circuit established: relay={relay_peer_id}");
                        }
                        libp2p::relay::client::Event::InboundCircuitEstablished {
                            src_peer_id,
                            ..
                        } => {
                            eprintln!("p2p relay inbound circuit established: src={src_peer_id}");
                        }
                    },
                    SwarmEvent::Dialing { peer_id, connection_id } => {
                        eprintln!("p2p dialing: peer={peer_id:?} connection_id={connection_id:?}");
                    }
                    SwarmEvent::ConnectionEstablished {
                        peer_id,
                        connection_id,
                        endpoint,
                        ..
                    } => {
                        eprintln!(
                            "p2p connection established: peer={peer_id} connection_id={connection_id:?} endpoint={endpoint:?}"
                        );
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                        let error = error.to_string();
                        if !is_loopback_direct_dial_noise(&error) {
                            eprintln!(
                                "warn: p2p outgoing connection error: peer={peer_id:?} error={error}"
                            );
                        }
                    }
                    SwarmEvent::ListenerClosed {
                        listener_id,
                        addresses,
                        reason,
                        ..
                    } => {
                        eprintln!(
                            "warn: p2p listener closed: addresses={addresses:?} reason={reason:?}"
                        );
                        let is_tracked_relay_listener =
                            relay_listener_id.as_ref() == Some(&listener_id);
                        if is_tracked_relay_listener {
                            relay_listener_id = None;
                        }
                        let had_relay_listener =
                            is_tracked_relay_listener || addresses.iter().any(is_relay_circuit_addr);
                        if remove_known_listen_addrs(&mut known_addrs, &addresses, local_peer_id)
                            && !trackers.is_empty()
                        {
                            spawn_register_all(
                                trackers.clone(),
                                local_peer_id,
                                known_addrs.iter().cloned().collect(),
                                meta.clone(),
                            );
                        }
                        if had_relay_listener
                            && let Some(relay_addr) = relay_addr.clone()
                        {
                            match relay_circuit_listen_addr(&relay_addr) {
                                Ok(relay_listen) => {
                                    eprintln!(
                                        "p2p relay listener closed; re-listen requested: {relay_listen}"
                                    );
                                    match swarm.listen_on(relay_listen) {
                                        Ok(new_listener_id) => {
                                            relay_listener_id = Some(new_listener_id);
                                        }
                                        Err(err) => {
                                            eprintln!("warn: p2p relay re-listen failed: {err:#}");
                                        }
                                    }
                                }
                                Err(err) => {
                                    eprintln!(
                                        "warn: p2p relay re-listen addr resolve failed: {err:#}"
                                    );
                                }
                            }
                        }
                    }
                    SwarmEvent::ListenerError { error, .. } => {
                        eprintln!("warn: p2p listener error: {error}");
                    }
                    SwarmEvent::ExpiredListenAddr { address, .. } => {
                        let full = ensure_p2p_suffix(address, local_peer_id);
                        if known_addrs.remove(&full.to_string()) && !trackers.is_empty() {
                            spawn_register_all(
                                trackers.clone(),
                                local_peer_id,
                                known_addrs.iter().cloned().collect(),
                                meta.clone(),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn spawn_register_all(
    trackers: Vec<crate::tracker::TrackerClient>,
    local_peer_id: PeerId,
    addrs: Vec<String>,
    meta: crate::tracker::PeerMeta,
) {
    // tracker 등록은 블로킹 I/O(ureq)이므로 런타임을 멈추지 않게 분리한다.
    let peer_id = local_peer_id.to_string();
    let req = crate::tracker::RegisterRequest {
        peer_id,
        addrs,
        meta: Some(meta),
    };

    drop(tokio::task::spawn_blocking(move || {
        for t in trackers {
            let _ = t.register(&req);
        }
    }));
}

fn authorize_inbound_peer(
    store: &LocalStore,
    peer: PeerId,
    local_meta: &crate::tracker::PeerMeta,
    trackers: &[crate::tracker::TrackerClient],
) -> Result<AuthorizedPeer> {
    let peer_id = peer.to_string();
    if let Some(peer) = store.get_peer_book_peer(&peer_id)? {
        return validate_authorized_peer_record(peer, local_meta);
    }

    if let Some(peer) = refresh_peer_book_peer_from_trackers(
        store,
        trackers,
        &peer_id,
        local_meta.user_id.as_deref(),
    )? {
        return validate_authorized_peer_record(peer, local_meta);
    }

    anyhow::bail!("peer is not present in peer_book or tracker: {peer_id}");
}

fn refresh_peer_book_peer_from_trackers(
    store: &LocalStore,
    trackers: &[crate::tracker::TrackerClient],
    peer_id: &str,
    user_id: Option<&str>,
) -> Result<Option<PeerBookPeer>> {
    for tracker in trackers {
        let list = match tracker.list(user_id) {
            Ok(list) => list,
            Err(err) => {
                eprintln!("warn: tracker peer authorization lookup failed: {err:#}");
                continue;
            }
        };

        for peer in list.peers {
            if peer.peer_id != peer_id {
                continue;
            }

            let peer_book = PeerBookPeer {
                peer_id: peer.peer_id,
                addrs: peer.addrs,
                user_id: peer.meta.as_ref().and_then(|m| m.user_id.clone()),
                device_id: peer.meta.as_ref().and_then(|m| m.device_id.clone()),
                last_seen_unix: peer.last_seen_unix,
            };
            store.upsert_peer_book(&peer_book)?;
            return Ok(Some(peer_book));
        }
    }

    Ok(None)
}

fn validate_authorized_peer_record(
    peer: PeerBookPeer,
    local_meta: &crate::tracker::PeerMeta,
) -> Result<AuthorizedPeer> {
    if let Some(local_user_id) = local_meta.user_id.as_deref()
        && peer.user_id.as_deref() != Some(local_user_id)
    {
        anyhow::bail!(
            "peer user_id mismatch: peer={} got={:?} want={local_user_id}",
            peer.peer_id,
            peer.user_id
        );
    }

    Ok(AuthorizedPeer {
        peer_id: peer.peer_id,
        user_id: peer.user_id,
        device_id: peer.device_id,
    })
}

fn validate_push_provenance(entries: &[crate::core::Entry], peer: &AuthorizedPeer) -> Result<()> {
    let user_id = peer
        .user_id
        .as_deref()
        .context("authorized peer is missing user_id")?;
    let device_id = peer
        .device_id
        .as_deref()
        .context("authorized peer is missing device_id")?;

    for entry in entries {
        if entry.user_id != user_id {
            anyhow::bail!(
                "entry user_id mismatch for peer {}: got={} want={user_id}",
                peer.peer_id,
                entry.user_id
            );
        }
        if !device_ids_match(&entry.device_id, device_id) {
            anyhow::bail!(
                "entry device_id mismatch for peer {}: got={} want={device_id}",
                peer.peer_id,
                entry.device_id
            );
        }
    }

    Ok(())
}

fn clamp_remote_pull_limit(limit: usize) -> usize {
    limit.clamp(1, crate::sync::SERVER_SYNC_PULL_LIMIT_MAX)
}

fn ensure_p2p_suffix(mut addr: Multiaddr, peer_id: PeerId) -> Multiaddr {
    match addr.iter().last() {
        Some(Protocol::P2p(got)) if got == peer_id => {}
        Some(Protocol::P2p(_)) => {
            let _ = addr.pop();
            addr.push(Protocol::P2p(peer_id));
        }
        _ => addr.push(Protocol::P2p(peer_id)),
    }
    addr
}

fn is_relay_circuit_addr(addr: &Multiaddr) -> bool {
    addr.iter().any(|p| matches!(p, Protocol::P2pCircuit))
}

fn remove_known_listen_addrs(
    known_addrs: &mut HashSet<String>,
    addrs: &[Multiaddr],
    peer_id: PeerId,
) -> bool {
    let mut removed = false;
    for addr in addrs {
        removed |= known_addrs.remove(&ensure_p2p_suffix(addr.clone(), peer_id).to_string());
    }
    removed
}

fn dialable_tracker_addr_from_external_candidate(
    addr: Multiaddr,
    peer_id: PeerId,
) -> Option<String> {
    // tracker에 광고하는 direct 주소는 다른 NAT/WiFi 뒤 peer가 blind dial하는 표면이다.
    // loopback/private/link-local/unspecified 및 relay circuit 주소는 direct 후보로 의미가 없다.
    if is_disallowed_tracker_direct_addr(&addr)
        || addr.iter().any(|p| matches!(p, Protocol::P2pCircuit))
    {
        return None;
    }

    Some(ensure_p2p_suffix(addr, peer_id).to_string())
}

pub fn sync(
    peers: &[String],
    limit: usize,
    db_path: &str,
    cfg: SyncConfig,
    push: bool,
) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;

    rt.block_on(async move { sync_async(peers, limit, db_path, cfg, push).await })
}

#[derive(Debug, Clone)]
struct SyncTarget {
    peer_id: PeerId,
    peer_key: String,
    dial_addrs: Vec<Multiaddr>,
    relay_addr: Option<Multiaddr>,
}

async fn sync_async(
    peers: &[String],
    limit: usize,
    db_path: &str,
    cfg: SyncConfig,
    push: bool,
) -> Result<()> {
    if limit == 0 {
        return Ok(());
    }

    let store = LocalStore::open(db_path)?;

    let targets = if !peers.is_empty() {
        build_manual_targets(&store, peers, cfg.relay_addr.clone())?
    } else {
        let discovered = discover_targets(&store, &cfg)?;
        limit_targets_per_tick(discovered, cfg.max_peers_per_tick)
    };

    if targets.is_empty() {
        anyhow::bail!("no peers found");
    }

    let push_device_id = if push {
        Some(
            cfg.device_id
                .as_deref()
                .context("device_id required for push")?,
        )
    } else {
        None
    };

    let mut progress = crate::sync::SyncRunProgress::new(push);
    let mut last_err: Option<anyhow::Error> = None;
    for t in targets {
        let mut client = match P2pClient::new(
            t.peer_id,
            t.dial_addrs,
            t.relay_addr,
            cfg.identity.clone(),
            cfg.psk,
            cfg.request_retry_policy.clone(),
        ) {
            Ok(v) => v,
            Err(err) => {
                eprintln!("warn: p2p client init failed: {}: {err:#}", t.peer_key);
                last_err = Some(err);
                continue;
            }
        };

        let pull_res =
            crate::sync::sync_pull_from_peer_async(&store, &t.peer_key, limit, &mut client)
                .await
                .with_context(|| format!("p2p pull peer: {}", t.peer_key));

        match pull_res {
            Ok(stats) => {
                progress.mark_pull_ok();
                if stats.received > 0 || stats.inserted > 0 {
                    eprintln!(
                        "p2p pull summary: {}: received={} inserted={} ignored={}",
                        t.peer_key, stats.received, stats.inserted, stats.ignored
                    );
                }
            }
            Err(err) => {
                eprintln!("warn: p2p pull failed: {}: {err:#}", t.peer_key);
                last_err = Some(err);
            }
        }

        if push {
            let pending_push = match store.count_pending_push_entries(&t.peer_key, push_device_id) {
                Ok(count) => count,
                Err(err) => {
                    eprintln!("warn: p2p push preflight failed: {}: {err:#}", t.peer_key);
                    last_err = Some(err);
                    continue;
                }
            };
            let push_needed = pending_push > 0;
            progress.note_push_needed(push_needed);

            client.reset_push_ack_stats();

            let push_res = crate::sync::sync_push_to_peer_async(
                &store,
                &t.peer_key,
                limit,
                push_device_id,
                &mut client,
            )
            .await
            .with_context(|| format!("p2p push peer: {}", t.peer_key));

            match push_res {
                Ok(pushed) => {
                    progress.mark_push_ok(push_needed);
                    if let Some((inserted, ignored)) = client.take_push_ack_stats() {
                        eprintln!(
                            "p2p push summary: {}: sent={pushed} inserted={inserted} ignored={ignored}",
                            t.peer_key
                        );
                    }
                }
                Err(err) => {
                    if let Some((inserted, ignored)) = client.take_push_ack_stats() {
                        eprintln!(
                            "warn: p2p push partial: {}: inserted={inserted} ignored={ignored}",
                            t.peer_key
                        );
                    }
                    eprintln!("warn: p2p push failed: {}: {err:#}", t.peer_key);
                    last_err = Some(err);
                }
            }
        }
    }

    if progress.is_success() {
        Ok(())
    } else {
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("p2p sync failed")))
    }
}

fn build_manual_targets(
    store: &LocalStore,
    peers: &[String],
    relay_addr: Option<Multiaddr>,
) -> Result<Vec<SyncTarget>> {
    let mut out = Vec::new();
    for peer_addr in peers {
        let peer_key_old = peer_addr.trim().to_string();
        if peer_key_old.is_empty() {
            continue;
        }

        let (peer_id, base_addr) = split_peer_multiaddr(&peer_key_old)?;
        let peer_key = peer_id.to_string();

        // Stage1 -> Stage2 마이그레이션: multiaddr 키에 커서가 있으면 peer_id 키로 복사.
        if store.get_last_cursor_opt(&peer_key)?.is_none()
            && let Some(old_cursor) = store.get_last_cursor_opt(&peer_key_old)?
        {
            store.set_last_cursor(&peer_key, old_cursor)?;
        }

        if store.get_last_pushed_seq_opt(&peer_key)?.is_none()
            && let Some(old_seq) = store.get_last_pushed_seq_opt(&peer_key_old)?
        {
            store.set_last_pushed_seq(&peer_key, old_seq)?;
        }

        // 수동 입력도 peerbook 캐시에 기록해 tracker 다운 시 fallback 후보로 활용한다.
        store.upsert_peer_book(&PeerBookPeer {
            peer_id: peer_key.clone(),
            addrs: vec![peer_key_old.clone()],
            user_id: None,
            device_id: None,
            last_seen_unix: OffsetDateTime::now_utc().unix_timestamp(),
        })?;

        out.push(SyncTarget {
            peer_id,
            peer_key,
            dial_addrs: vec![base_addr],
            relay_addr: relay_addr.clone(),
        });
    }
    Ok(out)
}

fn limit_targets_per_tick(
    mut targets: Vec<SyncTarget>,
    max_peers_per_tick: usize,
) -> Vec<SyncTarget> {
    if max_peers_per_tick == 0 || targets.len() <= max_peers_per_tick {
        return targets;
    }

    targets.sort_by(|a, b| a.peer_key.cmp(&b.peer_key));
    let tick = OffsetDateTime::now_utc().unix_timestamp().max(0) as usize / 60;
    let offset = tick % targets.len();
    let mut selected = Vec::with_capacity(max_peers_per_tick);
    for idx in 0..max_peers_per_tick {
        selected.push(targets[(offset + idx) % targets.len()].clone());
    }
    eprintln!(
        "p2p-sync tick: selected {}/{} peers (max_peers_per_tick={})",
        selected.len(),
        targets.len(),
        max_peers_per_tick
    );
    selected
}

fn discover_targets(store: &LocalStore, cfg: &SyncConfig) -> Result<Vec<SyncTarget>> {
    const PEER_BOOK_MAX_AGE_SECS: i64 = 60 * 60 * 24 * 7;
    const PEER_BOOK_LIMIT: usize = 1000;

    if cfg.trackers.is_empty() {
        anyhow::bail!("no peers provided and no trackers configured");
    }

    let relay_addr = cfg
        .relay_addr
        .clone()
        .context("relay_addr required for tracker-based sync")?;
    let local_peer_id = cfg.identity.public().to_peer_id().to_string();

    let mut by_peer: HashMap<String, crate::tracker::PeerInfo> = HashMap::new();
    for base_url in &cfg.trackers {
        let client =
            crate::tracker::TrackerClient::new(base_url.clone(), cfg.tracker_token.clone());
        match client.list(cfg.user_id.as_deref()) {
            Ok(list) => {
                for p in list.peers {
                    // self는 제외한다.
                    if sync_target_is_self(
                        &p.peer_id,
                        p.meta.as_ref().and_then(|m| m.device_id.as_deref()),
                        &local_peer_id,
                        cfg.device_id.as_deref(),
                    ) {
                        continue;
                    }

                    // 성공한 tracker 결과는 peerbook 캐시로 저장한다.
                    store.upsert_peer_book(&PeerBookPeer {
                        peer_id: p.peer_id.clone(),
                        addrs: p.addrs.clone(),
                        user_id: p.meta.as_ref().and_then(|m| m.user_id.clone()),
                        device_id: p.meta.as_ref().and_then(|m| m.device_id.clone()),
                        last_seen_unix: p.last_seen_unix,
                    })?;

                    by_peer.entry(p.peer_id.clone()).or_insert(p);
                }
            }
            Err(err) => {
                eprintln!("warn: tracker list failed: {base_url}: {err:#}");
            }
        }
    }

    if by_peer.is_empty() {
        let now_ts = OffsetDateTime::now_utc().unix_timestamp();
        let min_last_seen = now_ts - PEER_BOOK_MAX_AGE_SECS;
        let cached =
            store.list_peer_book(cfg.user_id.as_deref(), min_last_seen, PEER_BOOK_LIMIT)?;
        for peer in cached {
            // self는 제외한다.
            if sync_target_is_self(
                &peer.peer_id,
                peer.device_id.as_deref(),
                &local_peer_id,
                cfg.device_id.as_deref(),
            ) {
                continue;
            }

            by_peer.insert(
                peer.peer_id.clone(),
                crate::tracker::PeerInfo {
                    peer_id: peer.peer_id,
                    addrs: peer.addrs,
                    meta: Some(crate::tracker::PeerMeta {
                        device_id: peer.device_id,
                        hostname: None,
                        user_id: peer.user_id,
                        version: None,
                        build_revision: None,
                        build_dirty: None,
                    }),
                    last_seen_unix: peer.last_seen_unix,
                },
            );
        }

        if by_peer.is_empty() {
            anyhow::bail!("no peers found from trackers and peer_book cache is empty");
        }
    }

    let mut out = Vec::new();
    for (peer_id_str, peer) in by_peer {
        if sync_target_is_self(
            &peer_id_str,
            peer.meta.as_ref().and_then(|m| m.device_id.as_deref()),
            &local_peer_id,
            cfg.device_id.as_deref(),
        ) {
            continue;
        }

        let peer_id: PeerId = match peer_id_str.parse() {
            Ok(peer_id) => peer_id,
            Err(err) => {
                eprintln!("warn: skip peer with invalid peer_id: {peer_id_str}: {err}");
                continue;
            }
        };
        let (dial_addrs, target_relay_addr) =
            tracker_target_addrs(&peer.addrs, peer_id, &relay_addr);
        if dial_addrs.is_empty() && target_relay_addr.is_none() {
            eprintln!(
                "warn: skip peer without dialable direct addr or relay reservation: {peer_id_str}"
            );
            continue;
        }
        out.push(SyncTarget {
            peer_id,
            peer_key: peer_id_str,
            dial_addrs,
            relay_addr: target_relay_addr,
        });
    }

    if out.is_empty() {
        anyhow::bail!("no valid peers found after discovery");
    }

    Ok(out)
}

fn sync_target_is_self(
    peer_id: &str,
    peer_device_id: Option<&str>,
    local_peer_id: &str,
    local_device_id: Option<&str>,
) -> bool {
    if peer_id == local_peer_id {
        return true;
    }
    match (peer_device_id, local_device_id) {
        (Some(peer_device_id), Some(local_device_id)) => {
            device_ids_match(peer_device_id, local_device_id)
        }
        _ => false,
    }
}

fn device_ids_match(a: &str, b: &str) -> bool {
    let a = canonical_device_id(a);
    let b = canonical_device_id(b);
    !a.is_empty() && a == b
}

fn canonical_device_id(value: &str) -> String {
    let mut out = value.trim().to_string();
    for suffix in [
        "-x86_64", "-aarch64", "-arm64", "_x86_64", "_aarch64", "_arm64",
    ] {
        if let Some(stripped) = out.strip_suffix(suffix) {
            out = stripped.to_string();
            break;
        }
    }
    out
}

fn tracker_target_addrs(
    addrs: &[String],
    peer_id: PeerId,
    relay_addr: &Multiaddr,
) -> (Vec<Multiaddr>, Option<Multiaddr>) {
    let relay_addrs = relay_candidate_addrs_from_tracker(addrs, peer_id, relay_addr);
    if !relay_addrs.is_empty() {
        return (relay_addrs, None);
    }

    let direct_addrs = direct_candidate_addrs_from_tracker(addrs);
    if direct_addrs.is_empty() {
        return (Vec::new(), None);
    }

    (direct_addrs, None)
}

fn relay_candidate_addrs_from_tracker(
    addrs: &[String],
    peer_id: PeerId,
    configured_relay_addr: &Multiaddr,
) -> Vec<Multiaddr> {
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for raw in addrs {
        let Ok(addr) = raw.parse::<Multiaddr>() else {
            continue;
        };

        if !addr.iter().any(|p| matches!(p, Protocol::P2pCircuit)) {
            continue;
        }

        let Some(Protocol::P2p(got)) = addr.iter().last() else {
            continue;
        };
        if got != peer_id {
            continue;
        }

        let configured = configured_relay_addr.clone().with(Protocol::P2pCircuit);
        let key = configured.to_string();
        if seen.insert(key) {
            out.push(configured);
        }
    }

    out
}

fn direct_candidate_addrs_from_tracker(addrs: &[String]) -> Vec<Multiaddr> {
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for raw in addrs {
        let Ok(mut addr) = raw.parse::<Multiaddr>() else {
            continue;
        };

        // tracker/peerbook에서 온 direct 주소는 blind dial 표면이므로 공인 routable 후보만 허용한다.
        if is_disallowed_tracker_direct_addr(&addr) {
            continue;
        }

        // relay 주소는 direct 후보에서 제외한다.
        if addr.iter().any(|p| matches!(p, Protocol::P2pCircuit)) {
            continue;
        }

        // `/p2p/<peer_id>` suffix는 Swarm이 dial 시 자동으로 붙이므로 제거한다.
        if matches!(addr.iter().last(), Some(Protocol::P2p(_))) {
            let _ = addr.pop();
        }

        let key = addr.to_string();
        if seen.insert(key) {
            out.push(addr);
        }
    }

    out
}

fn tracker_announce_addr_from_listen_addr(addr: Multiaddr, peer_id: PeerId) -> Option<String> {
    if is_relay_circuit_addr(&addr) {
        return Some(ensure_p2p_suffix(addr, peer_id).to_string());
    }

    dialable_tracker_addr_from_external_candidate(addr, peer_id)
}

fn is_disallowed_tracker_direct_addr(addr: &Multiaddr) -> bool {
    addr.iter().any(|p| match p {
        Protocol::Ip4(ip) => {
            ip.is_unspecified()
                || ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_multicast()
                || ip.is_broadcast()
        }
        Protocol::Ip6(ip) => {
            ip.is_unspecified()
                || ip.is_loopback()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.is_multicast()
        }
        Protocol::Dns(_) | Protocol::Dns4(_) | Protocol::Dns6(_) | Protocol::Dnsaddr(_) => true,
        _ => false,
    })
}

fn addrs_include_relay_circuit(addrs: &[Multiaddr]) -> bool {
    addrs
        .iter()
        .any(|addr| addr.iter().any(|p| matches!(p, Protocol::P2pCircuit)))
}

fn relay_circuit_listen_addr(relay_addr: &Multiaddr) -> Result<Multiaddr> {
    Ok(resolve_dns_multiaddr_first(relay_addr)?.with(Protocol::P2pCircuit))
}

fn resolve_dns_multiaddrs(addrs: &[Multiaddr]) -> Result<Vec<Multiaddr>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for addr in addrs {
        for resolved in resolve_dns_multiaddr(addr)? {
            let key = resolved.to_string();
            if seen.insert(key) {
                out.push(resolved);
            }
        }
    }
    Ok(out)
}

fn resolve_dns_multiaddr_first(addr: &Multiaddr) -> Result<Multiaddr> {
    resolve_dns_multiaddr(addr)?
        .into_iter()
        .next()
        .with_context(|| format!("dns multiaddr resolved to no addresses: {addr}"))
}

fn resolve_dns_multiaddr(addr: &Multiaddr) -> Result<Vec<Multiaddr>> {
    let protocols: Vec<Protocol<'_>> = addr.iter().collect();
    let Some((dns_index, host, family)) =
        protocols
            .iter()
            .enumerate()
            .find_map(|(index, protocol)| match protocol {
                Protocol::Dns(host) => Some((index, host.to_string(), DnsAddressFamily::Any)),
                Protocol::Dns4(host) => Some((index, host.to_string(), DnsAddressFamily::V4)),
                Protocol::Dns6(host) => Some((index, host.to_string(), DnsAddressFamily::V6)),
                Protocol::Dnsaddr(host) => {
                    Some((index, host.to_string(), DnsAddressFamily::Dnsaddr))
                }
                _ => None,
            })
    else {
        return Ok(vec![addr.clone()]);
    };

    if family == DnsAddressFamily::Dnsaddr {
        anyhow::bail!("dnsaddr multiaddrs are not supported: {addr}");
    }

    let port = protocols
        .iter()
        .find_map(|protocol| match protocol {
            Protocol::Tcp(port) | Protocol::Udp(port) => Some(*port),
            _ => None,
        })
        .with_context(|| format!("dns multiaddr is missing tcp/udp port: {addr}"))?;

    let ips = resolve_dns_host(&host, port)?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for ip in ips {
        if !family.allows(ip) {
            continue;
        }

        let mut resolved = Multiaddr::empty();
        for (index, protocol) in protocols.iter().enumerate() {
            if index == dns_index {
                match ip {
                    IpAddr::V4(ip) => resolved.push(Protocol::Ip4(ip)),
                    IpAddr::V6(ip) => resolved.push(Protocol::Ip6(ip)),
                }
            } else {
                resolved.push(protocol.clone());
            }
        }

        let key = resolved.to_string();
        if seen.insert(key) {
            out.push(resolved);
        }
    }

    if out.is_empty() {
        anyhow::bail!("dns multiaddr resolved to no matching addresses: {addr}");
    }

    Ok(out)
}

fn resolve_dns_host(host: &str, port: u16) -> Result<Vec<IpAddr>> {
    if host.eq_ignore_ascii_case("localhost") {
        return Ok(vec![
            IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
        ]);
    }

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for socket_addr in (host, port)
        .to_socket_addrs()
        .with_context(|| format!("resolve dns host {host:?}"))?
    {
        let ip = socket_addr.ip();
        if seen.insert(ip) {
            out.push(ip);
        }
    }
    Ok(out)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DnsAddressFamily {
    Any,
    V4,
    V6,
    Dnsaddr,
}

impl DnsAddressFamily {
    fn allows(self, ip: IpAddr) -> bool {
        matches!(
            (self, ip),
            (Self::Any, _) | (Self::V4, IpAddr::V4(_)) | (Self::V6, IpAddr::V6(_))
        )
    }
}

fn split_peer_multiaddr(value: &str) -> Result<(PeerId, Multiaddr)> {
    let mut addr: Multiaddr = value.parse().context("parse peer multiaddr")?;
    let Some(last) = addr.pop() else {
        anyhow::bail!("peer multiaddr is empty");
    };
    let Protocol::P2p(peer_id) = last else {
        anyhow::bail!("peer multiaddr must end with /p2p/<peer_id>");
    };
    Ok((peer_id, addr))
}

struct P2pClient {
    peer_id: PeerId,
    dial_addrs: Vec<Multiaddr>,
    relay_addr: Option<Multiaddr>,
    request_retry_policy: RequestRetryPolicy,
    push_ack_stats_known: bool,
    push_ack_inserted_total: usize,
    push_ack_ignored_total: usize,
    swarm: Swarm<RustoryBehaviour>,
}

impl P2pClient {
    fn new(
        peer_id: PeerId,
        dial_addrs: Vec<Multiaddr>,
        relay_addr: Option<Multiaddr>,
        identity: libp2p::identity::Keypair,
        psk: libp2p::pnet::PreSharedKey,
        request_retry_policy: RequestRetryPolicy,
    ) -> Result<Self> {
        let dial_addrs = resolve_dns_multiaddrs(&dial_addrs)?;
        let relay_addr = relay_addr
            .map(|addr| resolve_dns_multiaddr_first(&addr))
            .transpose()?;
        let mut swarm = build_rustory_swarm_with_identity(identity, psk)?;
        let listen: Multiaddr = "/ip4/0.0.0.0/tcp/0"
            .parse()
            .context("parse ephemeral listen multiaddr")?;
        swarm.listen_on(listen).context("listen_on ephemeral")?;

        Ok(Self {
            peer_id,
            dial_addrs,
            relay_addr,
            request_retry_policy,
            push_ack_stats_known: false,
            push_ack_inserted_total: 0,
            push_ack_ignored_total: 0,
            swarm,
        })
    }

    fn reset_push_ack_stats(&mut self) {
        self.push_ack_stats_known = false;
        self.push_ack_inserted_total = 0;
        self.push_ack_ignored_total = 0;
    }

    fn take_push_ack_stats(&mut self) -> Option<(usize, usize)> {
        if !self.push_ack_stats_known {
            return None;
        }
        let out = (self.push_ack_inserted_total, self.push_ack_ignored_total);
        self.reset_push_ack_stats();
        Some(out)
    }

    async fn ensure_connected(&mut self) -> Result<()> {
        const DIRECT_BASE_TIMEOUT: Duration = Duration::from_secs(3);
        const RELAY_BASE_TIMEOUT: Duration = Duration::from_secs(10);
        const RELAY_TIMEOUT_CAP: Duration = Duration::from_secs(30);

        if self.swarm.is_connected(&self.peer_id) {
            return Ok(());
        }

        let mut relay_err: Option<anyhow::Error> = None;

        if let Some(relay_addr) = self.relay_addr.clone() {
            let addr = relay_addr.with(Protocol::P2pCircuit);
            match self
                .dial_with_retries(vec![addr], RELAY_BASE_TIMEOUT, Some(RELAY_TIMEOUT_CAP))
                .await
            {
                Ok(()) => return Ok(()),
                Err(err) => relay_err = Some(err),
            }
        }

        if !self.dial_addrs.is_empty() {
            let (timeout_base, timeout_cap) = if addrs_include_relay_circuit(&self.dial_addrs) {
                (RELAY_BASE_TIMEOUT, Some(RELAY_TIMEOUT_CAP))
            } else {
                (DIRECT_BASE_TIMEOUT, None)
            };
            match self
                .dial_with_retries(self.dial_addrs.clone(), timeout_base, timeout_cap)
                .await
            {
                Ok(()) => return Ok(()),
                Err(err) => {
                    if let Some(relay_err) = relay_err {
                        return Err(err).with_context(|| {
                            format!("dial addrs failed after relay dial failed: {relay_err:#}")
                        });
                    }
                    return Err(err);
                }
            }
        }

        if let Some(relay_err) = relay_err {
            return Err(relay_err);
        }

        anyhow::bail!("dial failed: no relay addr and no dial addrs");
    }

    async fn dial_with_retries(
        &mut self,
        addrs: Vec<Multiaddr>,
        base_timeout: Duration,
        timeout_cap: Option<Duration>,
    ) -> Result<()> {
        const ATTEMPTS: usize = 3;
        const BACKOFF_BASE: Duration = Duration::from_millis(200);

        let mut last_err: Option<anyhow::Error> = None;

        for attempt in 0..ATTEMPTS {
            let timeout = exp_duration(base_timeout, attempt as u32, timeout_cap);

            match self.dial_once(&addrs, timeout).await {
                Ok(()) => return Ok(()),
                Err(err) => last_err = Some(err),
            }

            if attempt + 1 < ATTEMPTS {
                let backoff = exp_duration(BACKOFF_BASE, attempt as u32, None);
                tokio::time::sleep(backoff).await;
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("dial failed")))
    }

    async fn dial_once(&mut self, addrs: &[Multiaddr], timeout: Duration) -> Result<()> {
        if self.swarm.is_connected(&self.peer_id) {
            return Ok(());
        }

        // 같은 peer에 대한 이전 시도 실패 후에도 다음 relay/direct 후보를 바로 시도할 수 있게,
        // dial 조건에서 NotDialing을 제거한다.
        let opts = DialOpts::peer_id(self.peer_id)
            .condition(libp2p::swarm::dial_opts::PeerCondition::Disconnected)
            .addresses(addrs.to_vec())
            .build();
        let connection_id = opts.connection_id();
        self.swarm.dial(opts).context("dial")?;

        let res = tokio::time::timeout(timeout, async {
            loop {
                match self.swarm.select_next_some().await {
                    SwarmEvent::ConnectionEstablished { peer_id, .. }
                        if peer_id == self.peer_id =>
                    {
                        return Ok(());
                    }
                    SwarmEvent::Behaviour(RustoryBehaviourEvent::Dcutr(event)) => {
                        log_dcutr_event(&event);
                    }
                    SwarmEvent::OutgoingConnectionError {
                        connection_id: got,
                        peer_id,
                        error,
                    } if got == connection_id && peer_id.is_none_or(|p| p == self.peer_id) => {
                        return Err(anyhow::anyhow!("dial failed: {error}"));
                    }
                    _ => {}
                }
            }
        })
        .await;

        match res {
            Ok(v) => v,
            Err(_) => {
                // pending dial attempt를 가능한 한 중단한다.
                let _ = self.swarm.disconnect_peer_id(self.peer_id);
                anyhow::bail!("dial timeout after {timeout:?}");
            }
        }
    }

    async fn pull_batch_with_retries(&mut self, cursor: i64, limit: usize) -> Result<PullBatch> {
        // mutable borrow(&mut self) 중에도 policy 값을 쓰기 위해 복사해 둔다.
        let attempts = self.request_retry_policy.attempts;
        let timeout_base = self.request_retry_policy.timeout_base;
        let timeout_cap = self.request_retry_policy.timeout_cap;
        let backoff_base = self.request_retry_policy.backoff_base;

        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..attempts {
            let timeout = exp_duration(timeout_base, attempt as u32, Some(timeout_cap));

            match self.pull_batch_once(cursor, limit, timeout).await {
                Ok(v) => return Ok(v),
                Err(err) => {
                    if !is_retryable_p2p_request_error(&err) || attempt + 1 >= attempts {
                        return Err(err);
                    }
                    last_err = Some(err);
                }
            }

            // pending 상태를 정리하기 위해 best-effort disconnect를 시도한다.
            let _ = self.swarm.disconnect_peer_id(self.peer_id);

            let backoff = exp_duration(backoff_base, attempt as u32, None);
            if backoff > Duration::from_millis(0) {
                tokio::time::sleep(backoff).await;
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("p2p pull failed")))
    }

    async fn pull_batch_once(
        &mut self,
        cursor: i64,
        limit: usize,
        timeout: Duration,
    ) -> Result<PullBatch> {
        self.ensure_connected().await?;

        let req = SyncPull { cursor, limit };
        let request_id = self
            .swarm
            .behaviour_mut()
            .sync
            .send_request(&self.peer_id, req);

        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);

        loop {
            tokio::select! {
                _ = &mut deadline => {
                    anyhow::bail!("p2p request timeout after {timeout:?}");
                }
                event = self.swarm.select_next_some() => {
                    match event {
                        SwarmEvent::Behaviour(RustoryBehaviourEvent::Sync(event)) => match event {
                            libp2p_request_response::Event::Message { message, .. } => match message {
                                libp2p_request_response::Message::Response {
                                    request_id: got_id,
                                    response,
                                } => {
                                    if got_id == request_id {
                                        return Ok(PullBatch {
                                            entries: response.entries,
                                            next_cursor: response.next_cursor,
                                        });
                                    }
                                }
                                libp2p_request_response::Message::Request { .. } => {}
                            },
                            libp2p_request_response::Event::OutboundFailure {
                                request_id: got_id,
                                error,
                                ..
                            } => {
                                if got_id == request_id {
                                    return Err(anyhow::Error::new(error))
                                        .context("p2p outbound request failed");
                                }
                            }
                            libp2p_request_response::Event::InboundFailure { .. } => {}
                            libp2p_request_response::Event::ResponseSent { .. } => {}
                        },
                        SwarmEvent::Behaviour(RustoryBehaviourEvent::Dcutr(event)) => {
                            log_dcutr_event(&event);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    async fn push_batch_with_retries(&mut self, entries: Vec<crate::core::Entry>) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        // mutable borrow(&mut self) 중에도 policy 값을 쓰기 위해 복사해 둔다.
        let attempts = self.request_retry_policy.attempts;
        let timeout_base = self.request_retry_policy.timeout_base;
        let timeout_cap = self.request_retry_policy.timeout_cap;
        let backoff_base = self.request_retry_policy.backoff_base;

        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..attempts {
            let timeout = exp_duration(timeout_base, attempt as u32, Some(timeout_cap));

            match self.push_batch_once(entries.clone(), timeout).await {
                Ok(()) => return Ok(()),
                Err(err) => {
                    if !is_retryable_p2p_request_error(&err) || attempt + 1 >= attempts {
                        return Err(err);
                    }
                    last_err = Some(err);
                }
            }

            let _ = self.swarm.disconnect_peer_id(self.peer_id);

            let backoff = exp_duration(backoff_base, attempt as u32, None);
            if backoff > Duration::from_millis(0) {
                tokio::time::sleep(backoff).await;
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("p2p push failed")))
    }

    async fn push_batch_once(
        &mut self,
        entries: Vec<crate::core::Entry>,
        timeout: Duration,
    ) -> Result<()> {
        self.ensure_connected().await?;

        let entries_len = entries.len();
        let req = EntriesPush { entries };
        let request_id = self
            .swarm
            .behaviour_mut()
            .push
            .send_request(&self.peer_id, req);

        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);

        loop {
            tokio::select! {
                _ = &mut deadline => {
                    anyhow::bail!("p2p request timeout after {timeout:?}");
                }
                event = self.swarm.select_next_some() => {
                    match event {
                        SwarmEvent::Behaviour(RustoryBehaviourEvent::Push(event)) => match event {
                            libp2p_request_response::Event::Message { message, .. } => match message {
                                libp2p_request_response::Message::Response {
                                    request_id: got_id,
                                    response,
                                } => {
                                    if got_id == request_id {
                                        if response.ok {
                                            if let (Some(inserted), Some(ignored)) =
                                                (response.inserted, response.ignored)
                                                && (ignored > 0 || inserted != entries_len)
                                            {
                                                self.push_ack_stats_known = true;
                                                self.push_ack_inserted_total += inserted;
                                                self.push_ack_ignored_total += ignored;
                                                eprintln!(
                                                    "p2p push ack: inserted={inserted} ignored={ignored}"
                                                );
                                            } else if let (Some(inserted), Some(ignored)) =
                                                (response.inserted, response.ignored)
                                            {
                                                self.push_ack_stats_known = true;
                                                self.push_ack_inserted_total += inserted;
                                                self.push_ack_ignored_total += ignored;
                                            }
                                            return Ok(());
                                        }
                                        anyhow::bail!("p2p push rejected");
                                    }
                                }
                                libp2p_request_response::Message::Request { .. } => {}
                            },
                            libp2p_request_response::Event::OutboundFailure {
                                request_id: got_id,
                                error,
                                ..
                            } => {
                                if got_id == request_id {
                                    return Err(anyhow::Error::new(error))
                                        .context("p2p outbound request failed");
                                }
                            }
                            libp2p_request_response::Event::InboundFailure { .. } => {}
                            libp2p_request_response::Event::ResponseSent { .. } => {}
                        },
                        SwarmEvent::Behaviour(RustoryBehaviourEvent::Dcutr(event)) => {
                            log_dcutr_event(&event);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

impl crate::sync::Puller for P2pClient {
    fn pull<'a>(
        &'a mut self,
        cursor: i64,
        limit: usize,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<PullBatch>> + 'a>> {
        Box::pin(self.pull_batch_with_retries(cursor, limit))
    }
}

impl crate::sync::Pusher for P2pClient {
    fn push<'a>(
        &'a mut self,
        entries: Vec<crate::core::Entry>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + 'a>> {
        Box::pin(self.push_batch_with_retries(entries))
    }
}

fn is_loopback_direct_dial_noise(message: &str) -> bool {
    let msg = message.to_ascii_lowercase();
    let has_loopback = msg.contains("/ip4/127.")
        || msg.contains("/ip6/::1")
        || msg.contains("/ip6/0:0:0:0:0:0:0:1");
    has_loopback && msg.contains("connection refused")
}

fn log_dcutr_event(event: &libp2p::dcutr::Event) {
    match &event.result {
        Ok(connection_id) => {
            eprintln!(
                "dcutr: upgraded to direct: peer={} connection_id={connection_id:?}",
                event.remote_peer_id
            );
        }
        Err(err) => {
            let error = err.to_string();
            if !is_dcutr_direct_upgrade_noise(&error) {
                eprintln!(
                    "warn: dcutr direct upgrade failed: peer={} error={error}",
                    event.remote_peer_id
                );
            }
        }
    }
}

fn is_dcutr_direct_upgrade_noise(message: &str) -> bool {
    let msg = message.to_ascii_lowercase();
    msg.contains("failed to hole-punch connection")
        && (msg.contains("outbound stream error")
            || msg.contains("giving up after")
            || msg.contains("io error")
            || msg.contains("protocol error"))
}

fn is_retryable_p2p_request_error(err: &anyhow::Error) -> bool {
    // payload-too-large는 상위 로직(배치 limit 축소)에 맡긴다.
    if crate::sync::is_payload_too_large_error(err) {
        return false;
    }

    // request-response 자체를 `tokio::select!`로 타임아웃 처리할 때는 anyhow string-only 에러가 된다.
    // 이 경우도 일시 오류로 보고 retryable로 취급한다.
    if err
        .chain()
        .any(|cause| cause.to_string().starts_with("p2p request timeout after"))
    {
        return true;
    }

    if err.chain().any(|cause| {
        let msg = cause.to_string().to_ascii_lowercase();
        msg.starts_with("dial failed:")
            && (msg.contains("resource limit exceeded")
                || msg.contains("connection reset by peer")
                || msg.contains("temporarily unavailable")
                || msg.contains("timed out")
                || msg.contains("oneshot canceled")
                || msg.contains("response from behaviour was canceled"))
    }) {
        return true;
    }

    for cause in err.chain() {
        if let Some(of) = cause.downcast_ref::<libp2p_request_response::OutboundFailure>() {
            return match of {
                libp2p_request_response::OutboundFailure::UnsupportedProtocols => false,
                libp2p_request_response::OutboundFailure::DialFailure => true,
                libp2p_request_response::OutboundFailure::Timeout => true,
                libp2p_request_response::OutboundFailure::ConnectionClosed => true,
                libp2p_request_response::OutboundFailure::Io(_) => true,
            };
        }
    }

    false
}

fn exp_duration(base: Duration, attempt: u32, cap: Option<Duration>) -> Duration {
    let factor = 1u32.checked_shl(attempt).unwrap_or(u32::MAX);
    let got = base
        .checked_mul(factor)
        .unwrap_or_else(|| cap.unwrap_or(Duration::MAX));
    match cap {
        Some(cap) if got > cap => cap,
        _ => got,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Entry;
    use libp2p_request_response::OutboundFailure;
    use tempfile::tempdir;
    use time::OffsetDateTime;

    fn entry(entry_id: &str, ts: i64, cmd: &str) -> Entry {
        Entry {
            entry_id: entry_id.to_string(),
            device_id: "dev1".to_string(),
            user_id: "user1".to_string(),
            ts: OffsetDateTime::from_unix_timestamp(ts).unwrap(),
            cmd: cmd.to_string(),
            cwd: "/tmp".to_string(),
            exit_code: 0,
            duration_ms: 12,
            shell: "zsh".to_string(),
            hostname: "host".to_string(),
            version: crate::build_info::VERSION.to_string(),
        }
    }

    #[test]
    fn relay_limits_default_raise_capacity_and_disable_rate_limiters() {
        let cfg = RelayLimits::default().to_libp2p_config().unwrap();
        assert_eq!(cfg.max_reservations, DEFAULT_RELAY_MAX_RESERVATIONS);
        assert_eq!(
            cfg.max_reservations_per_peer,
            DEFAULT_RELAY_MAX_RESERVATIONS_PER_PEER
        );
        assert_eq!(cfg.max_circuits, DEFAULT_RELAY_MAX_CIRCUITS);
        assert_eq!(
            cfg.max_circuits_per_peer,
            DEFAULT_RELAY_MAX_CIRCUITS_PER_PEER
        );
        assert_eq!(
            cfg.max_circuit_duration,
            Duration::from_secs(DEFAULT_RELAY_MAX_CIRCUIT_DURATION_SEC)
        );
        assert_eq!(cfg.max_circuit_bytes, DEFAULT_RELAY_MAX_CIRCUIT_BYTES);
        assert!(cfg.reservation_rate_limiters.is_empty());
        assert!(cfg.circuit_src_rate_limiters.is_empty());
    }

    #[test]
    fn split_peer_multiaddr_requires_p2p_suffix() {
        let err = split_peer_multiaddr("/ip4/127.0.0.1/tcp/1234").unwrap_err();
        assert!(err.to_string().contains("must end with /p2p/"));
    }

    #[test]
    fn direct_candidate_addrs_from_tracker_filters_relay_and_strips_p2p_suffix() {
        let peer_id = PeerId::random();
        let relay_id = PeerId::random();

        let direct = format!("/ip4/198.51.100.10/tcp/1234/p2p/{peer_id}");
        let relay = format!("/ip4/127.0.0.1/tcp/4001/p2p/{relay_id}/p2p-circuit/p2p/{peer_id}");
        let invalid = "not a multiaddr".to_string();

        let got = direct_candidate_addrs_from_tracker(&[direct, relay, invalid]);
        assert_eq!(got, vec!["/ip4/198.51.100.10/tcp/1234".parse().unwrap()]);
    }

    #[test]
    fn direct_candidate_addrs_from_tracker_rejects_private_loopback_and_link_local() {
        let peer_id = PeerId::random();
        let got = direct_candidate_addrs_from_tracker(&[
            format!("/ip4/127.0.0.1/tcp/1234/p2p/{peer_id}"),
            format!("/ip4/10.0.0.5/tcp/1234/p2p/{peer_id}"),
            format!("/ip4/172.19.0.5/tcp/1234/p2p/{peer_id}"),
            format!("/ip4/192.168.1.5/tcp/1234/p2p/{peer_id}"),
            format!("/ip4/169.254.1.5/tcp/1234/p2p/{peer_id}"),
            format!("/ip6/::1/tcp/1234/p2p/{peer_id}"),
            format!("/ip6/fd00::1/tcp/1234/p2p/{peer_id}"),
            format!("/ip6/fe80::1/tcp/1234/p2p/{peer_id}"),
            format!("/dns4/peer.example/tcp/1234/p2p/{peer_id}"),
            format!("/dns6/peer.example/tcp/1234/p2p/{peer_id}"),
        ]);
        assert!(got.is_empty());
    }

    #[test]
    fn relay_candidate_addrs_from_tracker_prefers_configured_relay() {
        let peer_id = PeerId::random();
        let relay_id = PeerId::random();
        let configured_relay: Multiaddr = format!("/ip4/198.51.100.10/tcp/4001/p2p/{relay_id}")
            .parse()
            .unwrap();
        let advertised =
            format!("/ip4/172.19.0.3/tcp/4001/p2p/{relay_id}/p2p-circuit/p2p/{peer_id}");
        let direct = format!("/ip4/172.19.0.5/tcp/1234/p2p/{peer_id}");

        let got =
            relay_candidate_addrs_from_tracker(&[direct, advertised], peer_id, &configured_relay);

        assert_eq!(
            got,
            vec![
                format!("/ip4/198.51.100.10/tcp/4001/p2p/{relay_id}/p2p-circuit")
                    .parse()
                    .unwrap()
            ]
        );
    }

    #[test]
    fn relay_candidate_addrs_from_tracker_ignores_wrong_target_peer() {
        let peer_id = PeerId::random();
        let other_peer = PeerId::random();
        let relay_id = PeerId::random();
        let configured_relay: Multiaddr = format!("/ip4/198.51.100.10/tcp/4001/p2p/{relay_id}")
            .parse()
            .unwrap();
        let advertised =
            format!("/ip4/172.19.0.3/tcp/4001/p2p/{relay_id}/p2p-circuit/p2p/{other_peer}");

        let got = relay_candidate_addrs_from_tracker(&[advertised], peer_id, &configured_relay);

        assert!(got.is_empty());
    }

    #[test]
    fn tracker_target_addrs_uses_configured_relay_when_tracker_advertises_circuit() {
        let peer_id = PeerId::random();
        let relay_id = PeerId::random();
        let configured_relay: Multiaddr = format!("/ip4/198.51.100.10/tcp/4001/p2p/{relay_id}")
            .parse()
            .unwrap();
        let advertised_relay =
            format!("/ip4/172.19.0.3/tcp/4001/p2p/{relay_id}/p2p-circuit/p2p/{peer_id}");
        let advertised_direct = format!("/ip4/172.19.0.5/tcp/1234/p2p/{peer_id}");

        let (dial_addrs, relay_addr) = tracker_target_addrs(
            &[advertised_direct, advertised_relay],
            peer_id,
            &configured_relay,
        );

        assert_eq!(
            dial_addrs,
            vec![
                format!("/ip4/198.51.100.10/tcp/4001/p2p/{relay_id}/p2p-circuit")
                    .parse()
                    .unwrap()
            ]
        );
        assert!(relay_addr.is_none());
    }

    #[test]
    fn tracker_target_addrs_skips_private_direct_records_without_relay_reservation() {
        let peer_id = PeerId::random();
        let relay_id = PeerId::random();
        let configured_relay: Multiaddr = format!("/ip4/198.51.100.10/tcp/4001/p2p/{relay_id}")
            .parse()
            .unwrap();
        let advertised_direct = format!("/ip4/172.19.0.5/tcp/1234/p2p/{peer_id}");

        let (dial_addrs, relay_addr) =
            tracker_target_addrs(&[advertised_direct], peer_id, &configured_relay);

        assert!(dial_addrs.is_empty());
        assert!(relay_addr.is_none());
    }

    #[test]
    fn addrs_include_relay_circuit_detects_manual_and_tracker_relay_targets() {
        let relay_id = PeerId::random();
        let target_peer = PeerId::random();
        let direct: Multiaddr = "/ip4/127.0.0.1/tcp/1234".parse().unwrap();
        let relay: Multiaddr =
            format!("/ip4/127.0.0.1/tcp/4001/p2p/{relay_id}/p2p-circuit/p2p/{target_peer}")
                .parse()
                .unwrap();

        assert!(!addrs_include_relay_circuit(&[direct]));
        assert!(addrs_include_relay_circuit(&[relay]));
    }

    #[test]
    fn resolve_dns_multiaddr_preserves_plain_ip_addr() {
        let peer_id = PeerId::random();
        let addr: Multiaddr = format!("/ip4/127.0.0.1/tcp/4001/p2p/{peer_id}")
            .parse()
            .unwrap();

        let got = resolve_dns_multiaddr(&addr).unwrap();

        assert_eq!(got, vec![addr]);
    }

    #[test]
    fn resolve_dns_multiaddr_expands_dns4_addr() {
        let peer_id = PeerId::random();
        let addr: Multiaddr = format!("/dns4/localhost/tcp/4001/p2p/{peer_id}")
            .parse()
            .unwrap();

        let got = resolve_dns_multiaddr(&addr).unwrap();

        assert_eq!(got.len(), 1);
        assert!(got[0].to_string().starts_with("/ip4/127.0.0.1/tcp/4001/"));
        assert!(got[0].to_string().ends_with(&format!("/p2p/{peer_id}")));
    }

    #[test]
    fn relay_circuit_listen_addr_preserves_plain_ip_relay_addr() {
        let relay_id = PeerId::random();
        let relay_addr: Multiaddr = format!("/ip4/127.0.0.1/tcp/4001/p2p/{relay_id}")
            .parse()
            .unwrap();

        let got = relay_circuit_listen_addr(&relay_addr).unwrap();

        assert_eq!(got, relay_addr.with(Protocol::P2pCircuit));
    }

    #[test]
    fn relay_circuit_listen_addr_resolves_dns4_before_p2p_circuit() {
        let relay_id = PeerId::random();
        let relay_addr: Multiaddr = format!("/dns4/localhost/tcp/4001/p2p/{relay_id}")
            .parse()
            .unwrap();

        let got = relay_circuit_listen_addr(&relay_addr).unwrap();

        assert!(got.to_string().starts_with("/ip4/127.0.0.1/tcp/4001/"));
        assert!(
            got.to_string()
                .ends_with(&format!("/p2p/{relay_id}/p2p-circuit"))
        );
    }

    #[test]
    fn resolve_dns_multiaddr_rejects_dnsaddr_addr() {
        let addr: Multiaddr = "/dnsaddr/relay.example/tcp/4001".parse().unwrap();

        let err = resolve_dns_multiaddr(&addr).unwrap_err().to_string();

        assert!(err.contains("dnsaddr multiaddrs are not supported"));
    }

    #[test]
    fn remove_known_listen_addrs_drops_relay_circuit_after_listener_close() {
        let relay_id = PeerId::random();
        let peer_id = PeerId::random();
        let direct: Multiaddr = "/ip4/127.0.0.1/tcp/1234".parse().unwrap();
        let relay: Multiaddr = format!("/ip4/127.0.0.1/tcp/4001/p2p/{relay_id}/p2p-circuit")
            .parse()
            .unwrap();
        let relay_full = relay.clone().with(Protocol::P2p(peer_id));

        let direct_full = ensure_p2p_suffix(direct.clone(), peer_id).to_string();
        let relay_full_string = relay_full.to_string();
        let mut known = HashSet::from([direct_full.clone(), relay_full_string.clone()]);

        assert!(is_relay_circuit_addr(&relay_full));
        assert!(remove_known_listen_addrs(&mut known, &[relay], peer_id));
        assert!(known.contains(&direct_full));
        assert!(!known.contains(&relay_full_string));
    }

    #[test]
    fn dialable_tracker_addr_from_external_candidate_filters_unspecified_and_relay() {
        let peer_id = PeerId::random();

        let ok: Multiaddr = "/ip4/192.0.2.10/tcp/4001".parse().unwrap();
        let out = dialable_tracker_addr_from_external_candidate(ok, peer_id).unwrap();
        assert!(out.ends_with(&format!("/p2p/{}", peer_id)));

        let unspecified: Multiaddr = "/ip4/0.0.0.0/tcp/4001".parse().unwrap();
        assert!(dialable_tracker_addr_from_external_candidate(unspecified, peer_id).is_none());

        let relay: Multiaddr = "/ip4/192.0.2.10/tcp/4001/p2p-circuit".parse().unwrap();
        assert!(dialable_tracker_addr_from_external_candidate(relay, peer_id).is_none());
    }

    #[test]
    fn tracker_announce_addr_from_listen_addr_filters_local_direct_but_keeps_relay() {
        let peer_id = PeerId::random();
        let relay_id = PeerId::random();

        let local: Multiaddr = "/ip4/127.0.0.1/tcp/1234".parse().unwrap();
        assert!(tracker_announce_addr_from_listen_addr(local, peer_id).is_none());

        let relay: Multiaddr =
            format!("/dns4/rustory-relay.example/tcp/4001/p2p/{relay_id}/p2p-circuit")
                .parse()
                .unwrap();
        let out = tracker_announce_addr_from_listen_addr(relay, peer_id).unwrap();
        assert!(out.ends_with(&format!("/p2p/{peer_id}")));
        assert!(out.contains("/p2p-circuit/"));
    }

    #[test]
    fn dialable_tracker_addr_from_external_candidate_overwrites_wrong_p2p_suffix() {
        let peer_id = PeerId::random();
        let other = PeerId::random();

        let addr: Multiaddr = format!("/ip4/192.0.2.10/tcp/4001/p2p/{other}")
            .parse()
            .unwrap();
        let out = dialable_tracker_addr_from_external_candidate(addr, peer_id).unwrap();
        assert!(out.ends_with(&format!("/p2p/{}", peer_id)));
    }

    #[test]
    fn discover_targets_falls_back_to_peer_book_when_trackers_fail() {
        let store = LocalStore::open(":memory:").unwrap();

        let peer_id = PeerId::random().to_string();
        let relay_id = PeerId::random();
        let relay_addr = format!("/ip4/127.0.0.1/tcp/4001/p2p/{relay_id}");
        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: peer_id.clone(),
                addrs: vec![format!("{relay_addr}/p2p-circuit/p2p/{peer_id}")],
                user_id: Some("u1".to_string()),
                device_id: Some("dev-remote".to_string()),
                last_seen_unix: OffsetDateTime::now_utc().unix_timestamp(),
            })
            .unwrap();

        let cfg = SyncConfig {
            identity: libp2p::identity::Keypair::generate_ed25519(),
            psk: libp2p::pnet::PreSharedKey::new([0; 32]),
            relay_addr: Some(relay_addr.parse().unwrap()),
            // connection refused should fail fast on loopback.
            trackers: vec!["http://127.0.0.1:1".to_string()],
            tracker_token: None,
            user_id: Some("u1".to_string()),
            device_id: Some("dev-local".to_string()),
            request_retry_policy: RequestRetryPolicy::default(),
            max_peers_per_tick: 0,
        };

        let got = discover_targets(&store, &cfg).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].peer_key, peer_id);
        assert_eq!(got[0].dial_addrs.len(), 1);
        assert!(got[0].relay_addr.is_none());
    }

    #[test]
    fn tracker_target_addrs_requires_dialable_direct_or_advertised_relay_reservation() {
        let peer_id = PeerId::random();
        let relay_id = PeerId::random();
        let relay_addr: Multiaddr = format!("/ip4/203.0.113.10/tcp/4001/p2p/{relay_id}")
            .parse()
            .unwrap();

        let (dial_addrs, relay_fallback) = tracker_target_addrs(
            &[format!("/ip4/127.0.0.1/tcp/1234/p2p/{peer_id}")],
            peer_id,
            &relay_addr,
        );
        assert!(dial_addrs.is_empty());
        assert!(relay_fallback.is_none());

        let (dial_addrs, relay_fallback) = tracker_target_addrs(
            &[format!("{relay_addr}/p2p-circuit/p2p/{peer_id}")],
            peer_id,
            &relay_addr,
        );
        assert_eq!(dial_addrs.len(), 1);
        assert!(relay_fallback.is_none());
    }

    #[test]
    fn discover_targets_skips_self_peer_book_entries_by_peer_id_and_device_id() {
        let store = LocalStore::open(":memory:").unwrap();
        let identity = libp2p::identity::Keypair::generate_ed25519();
        let local_peer_id = identity.public().to_peer_id().to_string();
        let stale_self_peer_id = PeerId::random().to_string();
        let remote_peer_id = PeerId::random().to_string();
        let relay_id = PeerId::random();
        let relay_addr = format!("/ip4/127.0.0.1/tcp/4001/p2p/{relay_id}");

        for (peer_id, device_id) in [
            (local_peer_id.clone(), "remote-looking-device"),
            (stale_self_peer_id, " dev-local "),
            (remote_peer_id.clone(), "dev-remote"),
        ] {
            let addrs = if peer_id == remote_peer_id {
                vec![format!("{relay_addr}/p2p-circuit/p2p/{peer_id}")]
            } else {
                vec![format!("/ip4/127.0.0.1/tcp/1234/p2p/{peer_id}")]
            };
            store
                .upsert_peer_book(&PeerBookPeer {
                    peer_id: peer_id.clone(),
                    addrs,
                    user_id: Some("u1".to_string()),
                    device_id: Some(device_id.to_string()),
                    last_seen_unix: OffsetDateTime::now_utc().unix_timestamp(),
                })
                .unwrap();
        }

        let cfg = SyncConfig {
            identity,
            psk: libp2p::pnet::PreSharedKey::new([0; 32]),
            relay_addr: Some(relay_addr.parse().unwrap()),
            trackers: vec!["http://127.0.0.1:1".to_string()],
            tracker_token: None,
            user_id: Some("u1".to_string()),
            device_id: Some("dev-local".to_string()),
            request_retry_policy: RequestRetryPolicy::default(),
            max_peers_per_tick: 0,
        };

        let got = discover_targets(&store, &cfg).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].peer_key, remote_peer_id);
    }

    #[test]
    fn build_manual_targets_migrates_legacy_pull_and_push_cursor_keys() {
        let store = LocalStore::open(":memory:").unwrap();
        let peer_id = PeerId::random();
        let peer_addr = format!("/ip4/127.0.0.1/tcp/1234/p2p/{peer_id}");
        let peer_key = peer_id.to_string();

        store.set_last_cursor(&peer_addr, 11).unwrap();
        store.set_last_pushed_seq(&peer_addr, 7).unwrap();

        let targets = build_manual_targets(&store, &[peer_addr], None).unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].peer_key, peer_key);
        assert_eq!(store.get_last_cursor(&targets[0].peer_key).unwrap(), 11);
        assert_eq!(store.get_last_pushed_seq(&targets[0].peer_key).unwrap(), 7);
    }

    #[test]
    fn limit_targets_per_tick_caps_tracker_fanout() {
        let targets = (0..3)
            .map(|idx| {
                let peer_id = PeerId::random();
                SyncTarget {
                    peer_id,
                    peer_key: format!("peer-{idx}"),
                    dial_addrs: Vec::new(),
                    relay_addr: None,
                }
            })
            .collect::<Vec<_>>();

        assert_eq!(limit_targets_per_tick(targets.clone(), 0).len(), 3);
        let limited = limit_targets_per_tick(targets, 1);
        assert_eq!(limited.len(), 1);
    }

    #[test]
    fn inbound_peer_authorization_rejects_unknown_peer() {
        let store = LocalStore::open(":memory:").unwrap();
        let peer = PeerId::random();
        let meta = crate::tracker::PeerMeta {
            user_id: Some("u1".to_string()),
            device_id: Some("dev-local".to_string()),
            hostname: None,
            version: None,
            build_revision: None,
            build_dirty: None,
        };

        let err = authorize_inbound_peer(&store, peer, &meta, &[]).unwrap_err();
        assert!(format!("{err:#}").contains("not present in peer_book or tracker"));
    }

    #[test]
    fn inbound_peer_authorization_requires_same_user_scope() {
        let store = LocalStore::open(":memory:").unwrap();
        let peer = PeerId::random();
        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: peer.to_string(),
                addrs: vec![],
                user_id: Some("other-user".to_string()),
                device_id: Some("dev-remote".to_string()),
                last_seen_unix: OffsetDateTime::now_utc().unix_timestamp(),
            })
            .unwrap();
        let meta = crate::tracker::PeerMeta {
            user_id: Some("u1".to_string()),
            device_id: Some("dev-local".to_string()),
            hostname: None,
            version: None,
            build_revision: None,
            build_dirty: None,
        };

        let err = authorize_inbound_peer(&store, peer, &meta, &[]).unwrap_err();
        assert!(format!("{err:#}").contains("peer user_id mismatch"));
    }

    #[test]
    fn inbound_push_provenance_requires_peer_user_and_device() {
        let authorized = AuthorizedPeer {
            peer_id: "peer-1".to_string(),
            user_id: Some("u1".to_string()),
            device_id: Some("dev-remote".to_string()),
        };
        let mut good = entry("id-1", 1, "echo ok");
        good.user_id = "u1".to_string();
        good.device_id = "dev-remote".to_string();
        validate_push_provenance(&[good.clone()], &authorized).unwrap();

        let mut bad = good;
        bad.device_id = "forged-device".to_string();
        let err = validate_push_provenance(&[bad], &authorized).unwrap_err();
        assert!(format!("{err:#}").contains("entry device_id mismatch"));
    }

    #[test]
    fn inbound_push_provenance_accepts_arch_suffix_device_renames() {
        let authorized = AuthorizedPeer {
            peer_id: "peer-1".to_string(),
            user_id: Some("u1".to_string()),
            device_id: Some("node1".to_string()),
        };
        let mut renamed = entry("id-1", 1, "echo ok");
        renamed.user_id = "u1".to_string();
        renamed.device_id = "node1-x86_64".to_string();

        validate_push_provenance(&[renamed], &authorized).unwrap();

        assert!(sync_target_is_self(
            "peer-remote",
            Some("node1-x86_64"),
            "peer-local",
            Some("node1")
        ));
    }

    #[test]
    fn remote_pull_limit_is_clamped_before_storage_read() {
        assert_eq!(clamp_remote_pull_limit(0), 1);
        assert_eq!(clamp_remote_pull_limit(10), 10);
        assert_eq!(
            clamp_remote_pull_limit(usize::MAX),
            crate::sync::SERVER_SYNC_PULL_LIMIT_MAX
        );
    }

    #[test]
    fn discover_targets_skips_invalid_cached_peer_ids() {
        let store = LocalStore::open(":memory:").unwrap();

        let valid_peer_id = PeerId::random().to_string();
        let relay_id = PeerId::random();
        let relay_addr = format!("/ip4/127.0.0.1/tcp/4001/p2p/{relay_id}");
        for (peer_id, user_id) in [
            ("not-a-peer-id".to_string(), "u1".to_string()),
            (valid_peer_id.clone(), "u1".to_string()),
        ] {
            let addrs = if peer_id == valid_peer_id {
                vec![format!("{relay_addr}/p2p-circuit/p2p/{peer_id}")]
            } else {
                vec![format!("/ip4/127.0.0.1/tcp/1234/p2p/{peer_id}")]
            };
            store
                .upsert_peer_book(&PeerBookPeer {
                    peer_id: peer_id.clone(),
                    addrs,
                    user_id: Some(user_id),
                    device_id: Some("dev-remote".to_string()),
                    last_seen_unix: OffsetDateTime::now_utc().unix_timestamp(),
                })
                .unwrap();
        }

        let cfg = SyncConfig {
            identity: libp2p::identity::Keypair::generate_ed25519(),
            psk: libp2p::pnet::PreSharedKey::new([0; 32]),
            relay_addr: Some(relay_addr.parse().unwrap()),
            trackers: vec!["http://127.0.0.1:1".to_string()],
            tracker_token: None,
            user_id: Some("u1".to_string()),
            device_id: Some("dev-local".to_string()),
            request_retry_policy: RequestRetryPolicy::default(),
            max_peers_per_tick: 0,
        };

        let got = discover_targets(&store, &cfg).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].peer_key, valid_peer_id);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn p2p_request_response_roundtrip_on_loopback() {
        let psk = libp2p::pnet::PreSharedKey::new([0; 32]);

        let dir = tempdir().unwrap();
        let remote_db = dir.path().join("remote.db");
        let remote = LocalStore::open(remote_db.to_str().unwrap()).unwrap();
        remote
            .insert_entries(&[entry("id-1", 1, "echo 1"), entry("id-2", 2, "echo 2")])
            .unwrap();

        let mut server = build_rustory_swarm(psk).unwrap();
        server
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();
        let server_peer = *server.local_peer_id();

        // 서버가 listen 주소를 얻을 때까지 진행.
        let listen_addr = loop {
            let event = server.select_next_some().await;
            if let SwarmEvent::NewListenAddr { address, .. } = event {
                break address;
            }
        };

        let mut client = build_rustory_swarm(psk).unwrap();
        client
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();
        client.add_peer_address(server_peer, listen_addr.clone());

        let req_id = client.behaviour_mut().sync.send_request(
            &server_peer,
            SyncPull {
                cursor: 0,
                limit: 10,
            },
        );

        let result = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                tokio::select! {
                    e = server.select_next_some() => {
                        if let SwarmEvent::Behaviour(RustoryBehaviourEvent::Sync(event)) = e
                            && let libp2p_request_response::Event::Message { message, .. } = event
                            && let libp2p_request_response::Message::Request { request, channel, .. } = message
                        {
                            let batch = remote
                                .pull_since_cursor(request.cursor, request.limit)
                                .unwrap();
                            let resp = SyncBatch {
                                entries: batch.entries,
                                next_cursor: batch.next_cursor,
                            };
                            let _ = server.behaviour_mut().sync.send_response(channel, resp);
                        }
                    }
                    e = client.select_next_some() => {
                        if let SwarmEvent::Behaviour(RustoryBehaviourEvent::Sync(event)) = e
                            && let libp2p_request_response::Event::Message { message, .. } = event
                            && let libp2p_request_response::Message::Response { request_id, response } = message
                            && request_id == req_id
                        {
                            break response;
                        }
                    }
                }
            }
        })
        .await
        .expect("timeout");

        assert_eq!(result.entries.len(), 2);
        assert_eq!(result.next_cursor, Some(2));
        assert_eq!(result.entries[0].entry_id, "id-1");
        assert_eq!(result.entries[1].entry_id, "id-2");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn p2p_entries_push_roundtrip_on_loopback() {
        let psk = libp2p::pnet::PreSharedKey::new([0; 32]);

        let dir = tempdir().unwrap();
        let remote_db = dir.path().join("remote.db");
        let remote = LocalStore::open(remote_db.to_str().unwrap()).unwrap();

        let mut server = build_rustory_swarm(psk).unwrap();
        server
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();
        let server_peer = *server.local_peer_id();

        // 서버가 listen 주소를 얻을 때까지 진행.
        let listen_addr = loop {
            let event = server.select_next_some().await;
            if let SwarmEvent::NewListenAddr { address, .. } = event {
                break address;
            }
        };

        let mut client = build_rustory_swarm(psk).unwrap();
        client
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();
        client.add_peer_address(server_peer, listen_addr.clone());

        let entry = entry("id-1", 1, "echo 1");
        let req_id = client.behaviour_mut().push.send_request(
            &server_peer,
            EntriesPush {
                entries: vec![entry.clone()],
            },
        );

        let result = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                tokio::select! {
                    e = server.select_next_some() => {
                        if let SwarmEvent::Behaviour(RustoryBehaviourEvent::Push(event)) = e
                            && let libp2p_request_response::Event::Message { message, .. } = event
                            && let libp2p_request_response::Message::Request { request, channel, .. } = message
                        {
                            remote.insert_entries(&request.entries).unwrap();
                            let _ = server.behaviour_mut().push.send_response(channel, PushAck { ok: true, inserted: None, ignored: None });
                        }
                    }
                    e = client.select_next_some() => {
                        if let SwarmEvent::Behaviour(RustoryBehaviourEvent::Push(event)) = e
                            && let libp2p_request_response::Event::Message { message, .. } = event
                            && let libp2p_request_response::Message::Response { request_id, response } = message
                            && request_id == req_id
                        {
                            break response;
                        }
                    }
                }
            }
        })
        .await
        .expect("timeout");

        assert!(result.ok);
        let got = remote.list_recent(10).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].entry_id, entry.entry_id);
        assert_eq!(got[0].cmd, entry.cmd);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn p2p_relay_reservation_reports_relays_listen_addr() {
        let psk = libp2p::pnet::PreSharedKey::new([1; 32]);

        let mut relay = build_relay_swarm_with_identity(
            libp2p::identity::Keypair::generate_ed25519(),
            psk,
            RelayLimits::default(),
        )
        .unwrap();
        relay
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();
        let relay_peer_id = *relay.local_peer_id();

        let relay_addr = loop {
            let event = relay.select_next_some().await;
            if let SwarmEvent::NewListenAddr { address, .. } = event {
                relay.add_external_address(address.clone());
                break address;
            }
        };

        let mut client = build_rustory_swarm(psk).unwrap();
        let client_peer_id = *client.local_peer_id();
        let client_addr = relay_addr
            .with(Protocol::P2p(relay_peer_id))
            .with(Protocol::P2pCircuit);
        client.listen_on(client_addr.clone()).unwrap();

        let expected_listen_addr = client_addr.with(Protocol::P2p(client_peer_id));
        let mut reservation_accepted = false;
        let mut listen_addr_reported = false;

        tokio::time::timeout(Duration::from_secs(5), async {
            while !reservation_accepted || !listen_addr_reported {
                tokio::select! {
                    event = relay.select_next_some() => {
                        match event {
                            SwarmEvent::NewListenAddr { address, .. } => {
                                relay.add_external_address(address);
                            }
                            SwarmEvent::Behaviour(RelayServerBehaviourEvent::Relay(
                                libp2p::relay::Event::ReservationReqAccepted { .. },
                            )) => {}
                            _ => {}
                        }
                    }
                    event = client.select_next_some() => {
                        match event {
                            SwarmEvent::Behaviour(RustoryBehaviourEvent::Relay(
                                libp2p::relay::client::Event::ReservationReqAccepted {
                                    relay_peer_id: got,
                                    renewal,
                                    ..
                                },
                            )) => {
                                assert_eq!(got, relay_peer_id);
                                assert!(!renewal);
                                reservation_accepted = true;
                            }
                            SwarmEvent::NewListenAddr { address, .. }
                                if address == expected_listen_addr =>
                            {
                                listen_addr_reported = true;
                            }
                            _ => {}
                        }
                    }
                }
            }
        })
        .await
        .expect("relay reservation timeout");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn p2p_relay_reservation_accepts_dns_relay_listen_addr() {
        let psk = libp2p::pnet::PreSharedKey::new([1; 32]);

        let mut relay = build_relay_swarm_with_identity(
            libp2p::identity::Keypair::generate_ed25519(),
            psk,
            RelayLimits::default(),
        )
        .unwrap();
        relay
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();
        let relay_peer_id = *relay.local_peer_id();

        let relay_port = loop {
            let event = relay.select_next_some().await;
            if let SwarmEvent::NewListenAddr { address, .. } = event {
                relay.add_external_address(address.clone());
                let Some(Protocol::Tcp(port)) =
                    address.iter().find(|p| matches!(p, Protocol::Tcp(_)))
                else {
                    panic!("relay listen addr missing tcp port: {address}");
                };
                break port;
            }
        };

        let mut client = build_rustory_swarm(psk).unwrap();
        let client_peer_id = *client.local_peer_id();
        let dns_relay_addr: Multiaddr =
            format!("/dns4/localhost/tcp/{relay_port}/p2p/{relay_peer_id}")
                .parse()
                .unwrap();
        let client_addr = relay_circuit_listen_addr(&dns_relay_addr).unwrap();

        assert!(client_addr.to_string().starts_with("/ip4/127.0.0.1/"));
        assert!(
            client_addr
                .to_string()
                .ends_with(&format!("/p2p/{relay_peer_id}/p2p-circuit"))
        );

        client.listen_on(client_addr.clone()).unwrap();

        let expected_listen_addr = client_addr.with(Protocol::P2p(client_peer_id));
        let mut reservation_accepted = false;
        let mut listen_addr_reported = false;

        tokio::time::timeout(Duration::from_secs(5), async {
            while !reservation_accepted || !listen_addr_reported {
                tokio::select! {
                    event = relay.select_next_some() => {
                        match event {
                            SwarmEvent::NewListenAddr { address, .. } => {
                                relay.add_external_address(address);
                            }
                            SwarmEvent::Behaviour(RelayServerBehaviourEvent::Relay(
                                libp2p::relay::Event::ReservationReqAccepted { .. },
                            )) => {}
                            _ => {}
                        }
                    }
                    event = client.select_next_some() => {
                        match event {
                            SwarmEvent::Behaviour(RustoryBehaviourEvent::Relay(
                                libp2p::relay::client::Event::ReservationReqAccepted {
                                    relay_peer_id: got,
                                    renewal,
                                    ..
                                },
                            )) => {
                                assert_eq!(got, relay_peer_id);
                                assert!(!renewal);
                                reservation_accepted = true;
                            }
                            SwarmEvent::NewListenAddr { address, .. }
                                if address == expected_listen_addr =>
                            {
                                listen_addr_reported = true;
                            }
                            _ => {}
                        }
                    }
                }
            }
        })
        .await
        .expect("dns relay reservation timeout");
    }

    #[test]
    fn is_retryable_p2p_request_error_marks_only_transient_failures_as_retryable() {
        let err = anyhow::Error::new(OutboundFailure::UnsupportedProtocols);
        assert!(!is_retryable_p2p_request_error(&err));

        let ioe = std::io::Error::new(std::io::ErrorKind::InvalidInput, "request too large");
        let err = anyhow::Error::new(OutboundFailure::Io(ioe));
        assert!(!is_retryable_p2p_request_error(&err));

        let err = anyhow::Error::new(OutboundFailure::Timeout);
        assert!(is_retryable_p2p_request_error(&err));

        let err = anyhow::anyhow!("p2p request timeout after 5s");
        assert!(is_retryable_p2p_request_error(&err));

        let err = anyhow::anyhow!(
            "dial failed: Failed to connect to destination.: Remote reported resource limit exceeded."
        );
        assert!(is_retryable_p2p_request_error(&err));

        let err = anyhow::anyhow!(
            "dial failed: Failed to negotiate transport protocol(s): Handshake error: Connection reset by peer (os error 104)"
        );
        assert!(is_retryable_p2p_request_error(&err));

        let err = anyhow::anyhow!(
            "dial failed: Failed to negotiate transport protocol(s): [(/ip4/192.0.2.1/tcp/4001/p2p/12D3KooW/p2p-circuit/p2p/12D3KooT: : Response from behaviour was canceled: oneshot canceled)]"
        );
        assert!(is_retryable_p2p_request_error(&err));

        let err = anyhow::anyhow!("dial failed: no relay addr and no dial addrs");
        assert!(!is_retryable_p2p_request_error(&err));
    }

    #[test]
    fn loopback_direct_dial_failure_is_log_noise() {
        let err = "Failed to negotiate transport protocol(s): [(/ip4/127.0.0.6/tcp/36485/p2p/12D3KooW: : Multiple dial errors occurred:
	 - Connection refused (os error 111): Connection refused (os error 111))]";

        assert!(is_loopback_direct_dial_noise(err));
        assert!(!is_loopback_direct_dial_noise(
            "Failed to negotiate transport protocol(s): [(/dns4/rustory-relay.example/tcp/4001/p2p/12D3KooRelay/p2p-circuit/p2p/12D3KooW: relay rejected)]"
        ));
    }

    #[test]
    fn dcutr_direct_upgrade_failure_is_log_noise() {
        assert!(is_dcutr_direct_upgrade_noise(
            "Failed to hole-punch connection: Outbound stream error: IO error"
        ));
        assert!(is_dcutr_direct_upgrade_noise(
            "Failed to hole-punch connection: Giving up after 3 dial attempts"
        ));
        assert!(!is_dcutr_direct_upgrade_noise(
            "Failed to connect to destination.: Relay has no reservation for destination."
        ));
    }

    #[test]
    fn exp_duration_saturates_to_cap_on_multiplication_overflow() {
        let base = Duration::from_secs(u64::MAX / 8);
        let cap = Duration::from_secs(u64::MAX / 4);

        assert_eq!(exp_duration(base, 4, Some(cap)), cap);
    }

    #[test]
    fn exp_duration_saturates_to_duration_max_without_cap_on_overflow() {
        let base = Duration::from_secs(u64::MAX / 8);

        assert_eq!(exp_duration(base, 4, None), Duration::MAX);
    }
}
