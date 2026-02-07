use crate::storage::{LocalStore, PeerBookPeer, PullBatch};
use anyhow::{Context, Result};
use futures::StreamExt;
use libp2p::core::transport::choice::OrTransport;
use libp2p::core::upgrade::Version;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::{SwarmEvent, dial_opts::DialOpts};
use libp2p::{Multiaddr, PeerId, StreamProtocol, Swarm, Transport};
use libp2p_request_response::ProtocolSupport;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use time::OffsetDateTime;

const SYNC_PULL_PROTOCOL: &str = "/rustory/sync-pull/1.0.0";
const ENTRIES_PUSH_PROTOCOL: &str = "/rustory/entries-push/1.0.0";

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
    pub psk: libp2p::pnet::PreSharedKey,
    pub relay_addr: Option<Multiaddr>,
    pub trackers: Vec<String>,
    pub tracker_token: Option<String>,
    pub user_id: Option<String>,
    pub device_id: Option<String>,
}

#[derive(Clone)]
pub struct RelayServeConfig {
    pub identity: libp2p::identity::Keypair,
    pub psk: libp2p::pnet::PreSharedKey,
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
}

#[derive(libp2p::swarm::NetworkBehaviour)]
#[behaviour(prelude = "libp2p::swarm::derive_prelude")]
struct RustoryBehaviour {
    relay: libp2p::relay::client::Behaviour,
    identify: libp2p::identify::Behaviour,
    dcutr: libp2p::dcutr::Behaviour,
    ping: libp2p::ping::Behaviour,
    sync: libp2p_request_response::json::Behaviour<SyncPull, SyncBatch>,
    push: libp2p_request_response::json::Behaviour<EntriesPush, PushAck>,
}

#[derive(libp2p::swarm::NetworkBehaviour)]
#[behaviour(prelude = "libp2p::swarm::derive_prelude")]
struct RelayServerBehaviour {
    relay: libp2p::relay::Behaviour,
    identify: libp2p::identify::Behaviour,
    ping: libp2p::ping::Behaviour,
}

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

    let protocols = [(
        StreamProtocol::new(SYNC_PULL_PROTOCOL),
        ProtocolSupport::Full,
    )];

    let rr_cfg =
        libp2p_request_response::Config::default().with_request_timeout(Duration::from_secs(30));
    let rr =
        libp2p_request_response::json::Behaviour::<SyncPull, SyncBatch>::new(protocols, rr_cfg);

    let push_protocols = [(
        StreamProtocol::new(ENTRIES_PUSH_PROTOCOL),
        ProtocolSupport::Full,
    )];
    let push_cfg =
        libp2p_request_response::Config::default().with_request_timeout(Duration::from_secs(30));
    let push_rr = libp2p_request_response::json::Behaviour::<EntriesPush, PushAck>::new(
        push_protocols,
        push_cfg,
    );

    let (relay_transport, relay_behaviour) = libp2p::relay::client::new(local_peer_id);
    let tcp_transport = libp2p::tcp::tokio::Transport::default();
    let transport = OrTransport::new(relay_transport, tcp_transport);
    let transport = libp2p::dns::tokio::Transport::system(transport).context("dns transport")?;

    let pnet = libp2p::pnet::PnetConfig::new(psk);
    let transport = transport.and_then(move |socket, _| pnet.handshake(socket));

    let noise_cfg = libp2p::noise::Config::new(&identity).context("noise config")?;
    let transport = transport
        .upgrade(Version::V1)
        .authenticate(noise_cfg)
        .multiplex(libp2p::yamux::Config::default())
        .boxed();

    let identify_cfg = libp2p::identify::Config::new("rustory/0.1.0".to_string(), local_public_key)
        .with_agent_version(format!("rustory/{}", env!("CARGO_PKG_VERSION")));

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
) -> Result<Swarm<RelayServerBehaviour>> {
    let local_public_key = identity.public();
    let local_peer_id = identity.public().to_peer_id();

    let tcp_transport = libp2p::tcp::tokio::Transport::default();
    let transport =
        libp2p::dns::tokio::Transport::system(tcp_transport).context("dns transport")?;

    let pnet = libp2p::pnet::PnetConfig::new(psk);
    let transport = transport.and_then(move |socket, _| pnet.handshake(socket));

    let noise_cfg = libp2p::noise::Config::new(&identity).context("noise config")?;
    let transport = transport
        .upgrade(Version::V1)
        .authenticate(noise_cfg)
        .multiplex(libp2p::yamux::Config::default())
        .boxed();

    let identify_cfg =
        libp2p::identify::Config::new("rustory-relay/0.1.0".to_string(), local_public_key)
            .with_agent_version(format!("rustory/{}", env!("CARGO_PKG_VERSION")));

    let behaviour = RelayServerBehaviour {
        relay: libp2p::relay::Behaviour::new(local_peer_id, libp2p::relay::Config::default()),
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
    let RelayServeConfig { identity, psk } = cfg;
    let mut swarm = build_relay_swarm_with_identity(identity, psk)?;
    swarm.listen_on(listen).context("listen_on")?;
    let local_peer_id = *swarm.local_peer_id();

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
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

    if let Some(relay_addr) = relay_addr.clone() {
        let relay_listen = relay_addr.with(Protocol::P2pCircuit);
        swarm.listen_on(relay_listen).context("listen_on relay")?;
    }

    let local_peer_id = *swarm.local_peer_id();

    let trackers = trackers
        .into_iter()
        .map(|base_url| crate::tracker::TrackerClient::new(base_url, tracker_token.clone()))
        .collect::<Vec<_>>();

    let mut known_addrs: HashSet<String> = HashSet::new();
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
                        let full = ensure_p2p_suffix(address, local_peer_id);
                        println!("p2p listen: {}", full);
                        known_addrs.insert(full.to_string());

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
                        libp2p_request_response::Event::Message { message, .. } => match message {
                            libp2p_request_response::Message::Request { request, channel, .. } => {
                                let batch = store.pull_since_cursor(request.cursor, request.limit)?;
                                let resp = SyncBatch {
                                    entries: batch.entries,
                                    next_cursor: batch.next_cursor,
                                };
                                let _ = swarm.behaviour_mut().sync.send_response(channel, resp);
                            }
                            libp2p_request_response::Message::Response { .. } => {}
                        },
                        libp2p_request_response::Event::OutboundFailure { .. } => {}
                        libp2p_request_response::Event::InboundFailure { .. } => {}
                        libp2p_request_response::Event::ResponseSent { .. } => {}
                    },
                    SwarmEvent::Behaviour(RustoryBehaviourEvent::Push(event)) => match event {
                        libp2p_request_response::Event::Message { message, .. } => match message {
                            libp2p_request_response::Message::Request { request, channel, .. } => {
                                let ok = store.insert_entries(&request.entries).is_ok();
                                if !ok {
                                    eprintln!("warn: p2p push insert failed");
                                }
                                let _ = swarm
                                    .behaviour_mut()
                                    .push
                                    .send_response(channel, PushAck { ok });
                            }
                            libp2p_request_response::Message::Response { .. } => {}
                        },
                        libp2p_request_response::Event::OutboundFailure { .. } => {}
                        libp2p_request_response::Event::InboundFailure { .. } => {}
                        libp2p_request_response::Event::ResponseSent { .. } => {}
                    },
                    SwarmEvent::Behaviour(RustoryBehaviourEvent::Dcutr(event)) => {
                        match &event.result {
                            Ok(connection_id) => {
                                eprintln!(
                                    "dcutr: upgraded to direct: peer={} connection_id={connection_id:?}",
                                    event.remote_peer_id
                                );
                            }
                            Err(err) => {
                                eprintln!(
                                    "dcutr: upgrade failed: peer={} error={err}",
                                    event.remote_peer_id
                                );
                            }
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

fn dialable_tracker_addr_from_external_candidate(
    addr: Multiaddr,
    peer_id: PeerId,
) -> Option<String> {
    // `0.0.0.0/::` 및 relay circuit 주소는 direct dial 후보로 의미가 없다.
    if addr.iter().any(|p| {
        matches!(p, Protocol::Ip4(ip) if ip.is_unspecified())
            || matches!(p, Protocol::Ip6(ip) if ip.is_unspecified())
            || matches!(p, Protocol::P2pCircuit)
    }) {
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
    direct_addrs: Vec<Multiaddr>,
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
        discover_targets(&store, &cfg)?
    };

    if targets.is_empty() {
        anyhow::bail!("no peers found");
    }

    let mut any_ok = false;
    let mut last_err: Option<anyhow::Error> = None;
    for t in targets {
        let mut client = match P2pClient::new(t.peer_id, t.direct_addrs, t.relay_addr, cfg.psk) {
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
            Ok(_) => any_ok = true,
            Err(err) => {
                eprintln!("warn: p2p pull failed: {}: {err:#}", t.peer_key);
                last_err = Some(err);
            }
        }

        if push {
            let push_res =
                crate::sync::sync_push_to_peer_async(&store, &t.peer_key, limit, &mut client)
                    .await
                    .with_context(|| format!("p2p push peer: {}", t.peer_key));

            match push_res {
                Ok(pushed) => {
                    if pushed > 0 {
                        any_ok = true;
                    }
                }
                Err(err) => {
                    eprintln!("warn: p2p push failed: {}: {err:#}", t.peer_key);
                    last_err = Some(err);
                }
            }
        }
    }

    if any_ok {
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
            direct_addrs: vec![base_addr],
            relay_addr: relay_addr.clone(),
        });
    }
    Ok(out)
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

    let mut by_peer: HashMap<String, crate::tracker::PeerInfo> = HashMap::new();
    for base_url in &cfg.trackers {
        let client =
            crate::tracker::TrackerClient::new(base_url.clone(), cfg.tracker_token.clone());
        match client.list(cfg.user_id.as_deref()) {
            Ok(list) => {
                for p in list.peers {
                    // self는 제외한다.
                    if let Some(my_device) = cfg.device_id.as_deref()
                        && p.meta.as_ref().and_then(|m| m.device_id.as_deref()) == Some(my_device)
                    {
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
            if let Some(my_device) = cfg.device_id.as_deref()
                && peer.device_id.as_deref() == Some(my_device)
            {
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
        if let Some(my_device) = cfg.device_id.as_deref()
            && peer.meta.as_ref().and_then(|m| m.device_id.as_deref()) == Some(my_device)
        {
            continue;
        }

        let peer_id: PeerId = peer_id_str.parse().context("parse peer_id")?;
        let direct_addrs = direct_candidate_addrs_from_tracker(&peer.addrs);
        out.push(SyncTarget {
            peer_id,
            peer_key: peer_id_str,
            direct_addrs,
            relay_addr: Some(relay_addr.clone()),
        });
    }

    Ok(out)
}

fn direct_candidate_addrs_from_tracker(addrs: &[String]) -> Vec<Multiaddr> {
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for raw in addrs {
        let Ok(mut addr) = raw.parse::<Multiaddr>() else {
            continue;
        };

        // 0.0.0.0/:: 리슨 주소는 상대가 dial 가능한 direct 후보가 아니다.
        if addr.iter().any(|p| {
            matches!(p, Protocol::Ip4(ip) if ip.is_unspecified())
                || matches!(p, Protocol::Ip6(ip) if ip.is_unspecified())
        }) {
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
    direct_addrs: Vec<Multiaddr>,
    relay_addr: Option<Multiaddr>,
    swarm: Swarm<RustoryBehaviour>,
}

impl P2pClient {
    fn new(
        peer_id: PeerId,
        direct_addrs: Vec<Multiaddr>,
        relay_addr: Option<Multiaddr>,
        psk: libp2p::pnet::PreSharedKey,
    ) -> Result<Self> {
        let mut swarm = build_rustory_swarm(psk)?;
        let listen: Multiaddr = "/ip4/0.0.0.0/tcp/0"
            .parse()
            .context("parse ephemeral listen multiaddr")?;
        swarm.listen_on(listen).context("listen_on ephemeral")?;

        Ok(Self {
            peer_id,
            direct_addrs,
            relay_addr,
            swarm,
        })
    }

    async fn ensure_connected(&mut self) -> Result<()> {
        const DIRECT_BASE_TIMEOUT: Duration = Duration::from_secs(3);
        const RELAY_BASE_TIMEOUT: Duration = Duration::from_secs(10);
        const RELAY_TIMEOUT_CAP: Duration = Duration::from_secs(30);

        if self.swarm.is_connected(&self.peer_id) {
            return Ok(());
        }

        if !self.direct_addrs.is_empty()
            && self
                .dial_with_retries(self.direct_addrs.clone(), DIRECT_BASE_TIMEOUT, None)
                .await
                .is_ok()
        {
            return Ok(());
        }

        if let Some(relay_addr) = self.relay_addr.clone() {
            let addr = relay_addr.with(Protocol::P2pCircuit);
            self.dial_with_retries(vec![addr], RELAY_BASE_TIMEOUT, Some(RELAY_TIMEOUT_CAP))
                .await?;
            return Ok(());
        }

        anyhow::bail!("dial failed: no relay addr and direct dial failed");
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

        let opts = DialOpts::peer_id(self.peer_id)
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
                        match &event.result {
                            Ok(connection_id) => {
                                eprintln!(
                                    "dcutr: upgraded to direct: peer={} connection_id={connection_id:?}",
                                    event.remote_peer_id
                                );
                            }
                            Err(err) => {
                                eprintln!(
                                    "dcutr: upgrade failed: peer={} error={err}",
                                    event.remote_peer_id
                                );
                            }
                        }
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

    async fn pull_batch(&mut self, cursor: i64, limit: usize) -> Result<PullBatch> {
        self.ensure_connected().await?;

        let req = SyncPull { cursor, limit };
        let request_id = self
            .swarm
            .behaviour_mut()
            .sync
            .send_request(&self.peer_id, req);

        loop {
            let event = self.swarm.select_next_some().await;
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
                            anyhow::bail!("p2p outbound request failed: {error}");
                        }
                    }
                    libp2p_request_response::Event::InboundFailure { .. } => {}
                    libp2p_request_response::Event::ResponseSent { .. } => {}
                },
                SwarmEvent::Behaviour(RustoryBehaviourEvent::Dcutr(event)) => match &event.result {
                    Ok(connection_id) => {
                        eprintln!(
                            "dcutr: upgraded to direct: peer={} connection_id={connection_id:?}",
                            event.remote_peer_id
                        );
                    }
                    Err(err) => {
                        eprintln!(
                            "dcutr: upgrade failed: peer={} error={err}",
                            event.remote_peer_id
                        );
                    }
                },
                _ => {}
            }
        }
    }

    async fn push_batch(&mut self, entries: Vec<crate::core::Entry>) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        self.ensure_connected().await?;

        let req = EntriesPush { entries };
        let request_id = self
            .swarm
            .behaviour_mut()
            .push
            .send_request(&self.peer_id, req);

        loop {
            let event = self.swarm.select_next_some().await;
            match event {
                SwarmEvent::Behaviour(RustoryBehaviourEvent::Push(event)) => match event {
                    libp2p_request_response::Event::Message { message, .. } => match message {
                        libp2p_request_response::Message::Response {
                            request_id: got_id,
                            response,
                        } => {
                            if got_id == request_id {
                                if response.ok {
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
                            anyhow::bail!("p2p outbound request failed: {error}");
                        }
                    }
                    libp2p_request_response::Event::InboundFailure { .. } => {}
                    libp2p_request_response::Event::ResponseSent { .. } => {}
                },
                SwarmEvent::Behaviour(RustoryBehaviourEvent::Dcutr(event)) => match &event.result {
                    Ok(connection_id) => {
                        eprintln!(
                            "dcutr: upgraded to direct: peer={} connection_id={connection_id:?}",
                            event.remote_peer_id
                        );
                    }
                    Err(err) => {
                        eprintln!(
                            "dcutr: upgrade failed: peer={} error={err}",
                            event.remote_peer_id
                        );
                    }
                },
                _ => {}
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
        Box::pin(self.pull_batch(cursor, limit))
    }
}

impl crate::sync::Pusher for P2pClient {
    fn push<'a>(
        &'a mut self,
        entries: Vec<crate::core::Entry>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + 'a>> {
        Box::pin(self.push_batch(entries))
    }
}

fn exp_duration(base: Duration, attempt: u32, cap: Option<Duration>) -> Duration {
    let factor = 1u32.checked_shl(attempt).unwrap_or(u32::MAX);
    let got = base.checked_mul(factor).unwrap_or(base);
    match cap {
        Some(cap) if got > cap => cap,
        _ => got,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Entry;
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
            version: "0.1.0".to_string(),
        }
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

        let direct = format!("/ip4/127.0.0.1/tcp/1234/p2p/{peer_id}");
        let relay = format!("/ip4/127.0.0.1/tcp/4001/p2p/{relay_id}/p2p-circuit/p2p/{peer_id}");
        let invalid = "not a multiaddr".to_string();

        let got = direct_candidate_addrs_from_tracker(&[direct, relay, invalid]);
        assert_eq!(got, vec!["/ip4/127.0.0.1/tcp/1234".parse().unwrap()]);
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
        store
            .upsert_peer_book(&PeerBookPeer {
                peer_id: peer_id.clone(),
                addrs: vec![format!("/ip4/127.0.0.1/tcp/1234/p2p/{peer_id}")],
                user_id: Some("u1".to_string()),
                device_id: Some("dev-remote".to_string()),
                last_seen_unix: OffsetDateTime::now_utc().unix_timestamp(),
            })
            .unwrap();

        let relay_id = PeerId::random();
        let cfg = SyncConfig {
            psk: libp2p::pnet::PreSharedKey::new([0; 32]),
            relay_addr: Some(
                format!("/ip4/127.0.0.1/tcp/4001/p2p/{relay_id}")
                    .parse()
                    .unwrap(),
            ),
            // connection refused should fail fast on loopback.
            trackers: vec!["http://127.0.0.1:1".to_string()],
            tracker_token: None,
            user_id: Some("u1".to_string()),
            device_id: Some("dev-local".to_string()),
        };

        let got = discover_targets(&store, &cfg).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].peer_key, peer_id);
        assert_eq!(
            got[0].direct_addrs,
            vec!["/ip4/127.0.0.1/tcp/1234".parse().unwrap()]
        );
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
                            let _ = server.behaviour_mut().push.send_response(channel, PushAck { ok: true });
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
}
