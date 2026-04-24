use crate::storage::{LocalStore, PullBatch};
use anyhow::{Context, Result};
use futures::StreamExt;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::SwarmEvent;
use libp2p::{Multiaddr, PeerId, StreamProtocol, Swarm, SwarmBuilder};
use libp2p_request_response::ProtocolSupport;
use std::time::Duration;

const SYNC_PULL_PROTOCOL: &str = "/rustory/sync-pull/1.0.0";

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

#[derive(libp2p::swarm::NetworkBehaviour)]
#[behaviour(prelude = "libp2p::swarm::derive_prelude")]
struct RustoryBehaviour {
    sync: libp2p_request_response::json::Behaviour<SyncPull, SyncBatch>,
}

fn build_swarm() -> Result<Swarm<RustoryBehaviour>> {
    let protocols = [(
        StreamProtocol::new(SYNC_PULL_PROTOCOL),
        ProtocolSupport::Full,
    )];

    let rr_cfg =
        libp2p_request_response::Config::default().with_request_timeout(Duration::from_secs(30));
    let rr =
        libp2p_request_response::json::Behaviour::<SyncPull, SyncBatch>::new(protocols, rr_cfg);

    let swarm = SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            Default::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )
        .context("build tcp transport")?
        .with_dns()
        .context("add dns transport")?
        .with_behaviour(|_key| RustoryBehaviour { sync: rr })
        .expect("infallible")
        .build();

    Ok(swarm)
}

pub fn serve(listen: &str, db_path: &str) -> Result<()> {
    let listen: Multiaddr = listen.parse().context("parse listen multiaddr")?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;

    rt.block_on(async move { serve_async(listen, db_path).await })
}

async fn serve_async(listen: Multiaddr, db_path: &str) -> Result<()> {
    let store = LocalStore::open(db_path)?;
    let mut swarm = build_swarm()?;

    swarm.listen_on(listen).context("listen_on")?;

    let local_peer_id = *swarm.local_peer_id();

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                println!("p2p listen: {}/p2p/{}", address, local_peer_id);
            }
            SwarmEvent::Behaviour(RustoryBehaviourEvent::Sync(event)) => match event {
                libp2p_request_response::Event::Message { message, .. } => match message {
                    libp2p_request_response::Message::Request {
                        request, channel, ..
                    } => {
                        let batch = store.pull_since_cursor(request.cursor, request.limit)?;
                        let resp = SyncBatch {
                            entries: batch.entries,
                            next_cursor: batch.next_cursor,
                        };
                        let _ = swarm.behaviour_mut().sync.send_response(channel, resp);
                    }
                    libp2p_request_response::Message::Response { .. } => {
                        // 서버는 response를 받을 일이 없다(향후 확장 가능).
                    }
                },
                libp2p_request_response::Event::OutboundFailure { .. } => {}
                libp2p_request_response::Event::InboundFailure { .. } => {}
                libp2p_request_response::Event::ResponseSent { .. } => {}
            },
            _ => {}
        }
    }
}

pub fn sync(peers: &[String], limit: usize, db_path: &str) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;

    rt.block_on(async move { sync_async(peers, limit, db_path).await })
}

async fn sync_async(peers: &[String], limit: usize, db_path: &str) -> Result<()> {
    if limit == 0 {
        return Ok(());
    }

    let store = LocalStore::open(db_path)?;

    for peer_addr in peers {
        let peer_key = peer_addr.trim().to_string();
        if peer_key.is_empty() {
            continue;
        }

        let (peer_id, base_addr) = split_peer_multiaddr(&peer_key)?;
        let mut client = P2pClient::new(peer_id, base_addr)?;

        let _pulled = crate::sync::sync_pull_from_peer_async(&store, &peer_key, limit, &mut client)
            .await
            .with_context(|| format!("p2p sync peer: {peer_key}"))?;
    }

    Ok(())
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
    swarm: Swarm<RustoryBehaviour>,
}

impl P2pClient {
    fn new(peer_id: PeerId, peer_addr: Multiaddr) -> Result<Self> {
        let mut swarm = build_swarm()?;
        swarm
            .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
            .context("listen_on ephemeral")?;
        swarm.add_peer_address(peer_id, peer_addr);

        Ok(Self { peer_id, swarm })
    }

    async fn pull_batch(&mut self, cursor: i64, limit: usize) -> Result<PullBatch> {
        let req = SyncPull { cursor, limit };
        let request_id = self
            .swarm
            .behaviour_mut()
            .sync
            .send_request(&self.peer_id, req);

        loop {
            let event = self.swarm.select_next_some().await;
            let SwarmEvent::Behaviour(RustoryBehaviourEvent::Sync(event)) = event else {
                continue;
            };

            match event {
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
                    libp2p_request_response::Message::Request { .. } => {
                        // client는 inbound 요청을 받지 않는다(향후 확장 가능).
                    }
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

    #[tokio::test(flavor = "current_thread")]
    async fn p2p_request_response_roundtrip_on_loopback() {
        let dir = tempdir().unwrap();
        let remote_db = dir.path().join("remote.db");
        let remote = LocalStore::open(remote_db.to_str().unwrap()).unwrap();
        remote
            .insert_entries(&[entry("id-1", 1, "echo 1"), entry("id-2", 2, "echo 2")])
            .unwrap();

        let mut server = build_swarm().unwrap();
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

        let mut client = build_swarm().unwrap();
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
}
