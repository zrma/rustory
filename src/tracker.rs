use crate::libp2p::PeerId;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use time::OffsetDateTime;

use crate::device_retirement::{
    DEVICE_MEMBERSHIP_PROTOCOL_VERSION, DeviceProof, RETIREMENT_INITIAL_ATTEMPT,
    RETIREMENT_PROTOCOL_VERSION, RetirementCleanup, RetirementStatus, RetirementTicket,
    completion_capability_hash, sign_device_action, verify_device_action,
};
use crate::terminal::contains_terminal_control;

const ACTION_REGISTER: &str = "register";
const ACTION_UNREGISTER: &str = "unregister";
const ACTION_RETIREMENT_POLL: &str = "retirement_poll";
const ACTION_RETIREMENT_ACK: &str = "retirement_ack";
const USED_NONCE_TTL_SEC: i64 = 10 * 60;
const MAX_USED_DEVICE_NONCES: usize = 16_384;
const MAX_USED_DEVICE_NONCES_PER_PEER: usize = 128;
const OBSERVED_DEVICE_TTL_SEC: i64 = 15 * 60;
const MAX_PENDING_OBSERVED_DEVICES: usize = 4096;
const TRACKER_PRUNE_INTERVAL_SEC: i64 = 5;
const MAX_RETIREMENT_COMPLETION_BODY_BYTES: usize = 1024;
const MAX_RETIREMENT_DEVICE_BODY_BYTES: usize = 8 * 1024;
const MAX_TRACKER_REGISTER_BODY_BYTES: usize = 64 * 1024;
const MAX_TRACKER_RESPONSE_BODY_BYTES: usize = 512 * 1024;
const DEVICE_REQUEST_HEADER: &str = "X-Rustory-Device-Request";
const MAX_DEVICE_REQUEST_HEADER_BYTES: usize = 24 * 1024;
const TRACKER_HTTP_CONNECTION_LIMIT: usize = 256;
const TRACKER_HTTP_INFLIGHT_LIMIT: usize = 64;
#[cfg(not(test))]
const TRACKER_HTTP_IO_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(test)]
const TRACKER_HTTP_IO_TIMEOUT: Duration = Duration::from_millis(250);
#[cfg(not(test))]
const TRACKER_HTTP_CONNECTION_LIFETIME: Duration = Duration::from_secs(30);
#[cfg(test)]
const TRACKER_HTTP_CONNECTION_LIFETIME: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct TrackerServeConfig {
    pub ttl_sec: u64,
    pub token: Option<String>,
    pub admin_token: Option<String>,
    pub security_state_path: Option<PathBuf>,
    pub require_device_enrollment: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerMeta {
    pub device_id: Option<String>,
    pub hostname: Option<String>,
    pub user_id: Option<String>,
    pub version: Option<String>,
    pub build_revision: Option<String>,
    pub build_dirty: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub peer_id: String,
    pub addrs: Vec<String>,
    #[serde(default)]
    pub meta: Option<PeerMeta>,
    #[serde(default)]
    pub retirement_protocol: Option<u32>,
    #[serde(default)]
    pub membership_protocol: Option<u32>,
    #[serde(default)]
    pub membership_enforced: bool,
    #[serde(default)]
    pub device_proof: Option<DeviceProof>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub ok: bool,
    pub ttl_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnregisterRequest {
    pub peer_id: String,
    #[serde(default)]
    pub device_proof: Option<DeviceProof>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnregisterResponse {
    pub ok: bool,
    pub removed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer_id: String,
    pub addrs: Vec<String>,
    pub meta: Option<PeerMeta>,
    pub last_seen_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResponse {
    pub peers: Vec<PeerInfo>,
    #[serde(default)]
    pub revocations: Vec<RevocationInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RevocationInfo {
    pub peer_id: String,
    pub device_id: Option<String>,
    pub user_id: Option<String>,
    pub revoked_at_unix: i64,
    pub ticket_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminEnrollRequest {
    pub peer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminRetireRequest {
    pub peer_id: String,
    pub cleanup: RetirementCleanup,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminMutationResponse {
    pub ok: bool,
    pub peer_id: String,
    pub ticket: Option<RetirementTicket>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub membership_enforcement_complete: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminDeviceInfo {
    pub peer_id: String,
    pub device_id: Option<String>,
    pub user_id: Option<String>,
    pub enrolled: bool,
    pub active: bool,
    pub revoked: bool,
    pub retirement_protocol: Option<u32>,
    pub membership_protocol: Option<u32>,
    pub membership_enforced: bool,
    pub ticket: Option<RetirementTicket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminDeviceListResponse {
    pub devices: Vec<AdminDeviceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetirementPollRequest {
    pub peer_id: String,
    pub device_proof: DeviceProof,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetirementPollResponse {
    pub ticket: Option<RetirementTicket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetirementAckRequest {
    pub peer_id: String,
    pub ticket_id: String,
    #[serde(
        default = "initial_retirement_attempt",
        skip_serializing_if = "is_initial_retirement_attempt"
    )]
    pub attempt: u32,
    pub status: RetirementStatus,
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_capability_hash: Option<String>,
    pub device_proof: DeviceProof,
}

fn initial_retirement_attempt() -> u32 {
    RETIREMENT_INITIAL_ATTEMPT
}

fn is_initial_retirement_attempt(attempt: &u32) -> bool {
    *attempt == RETIREMENT_INITIAL_ATTEMPT
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetirementAckResponse {
    pub ok: bool,
    pub ticket: RetirementTicket,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RetirementCompleteRequest {
    pub peer_id: String,
    pub ticket_id: String,
    pub completion_capability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembershipResponse {
    pub peer_id: String,
    pub active: bool,
    pub enrolled: bool,
    pub revoked: bool,
    pub strict: bool,
    pub device_id: Option<String>,
    pub user_id: Option<String>,
    pub revocation: Option<RevocationInfo>,
}

#[derive(Debug, Clone)]
struct PeerRecord {
    addrs: Vec<String>,
    meta: Option<PeerMeta>,
    last_seen_unix: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EnrolledDevice {
    peer_id: String,
    public_key: Vec<u8>,
    device_id: Option<String>,
    user_id: Option<String>,
    retirement_protocol: Option<u32>,
    membership_protocol: Option<u32>,
    #[serde(default)]
    membership_enforced: bool,
    enrolled_at_unix: i64,
}

#[derive(Debug, Clone)]
struct ObservedDevice {
    public_key: Vec<u8>,
    device_id: Option<String>,
    user_id: Option<String>,
    retirement_protocol: Option<u32>,
    membership_protocol: Option<u32>,
    membership_enforced: bool,
    last_seen_unix: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct DurableTrackerSecurityState {
    enrolled_devices: HashMap<String, EnrolledDevice>,
    revocations: HashMap<String, RevocationInfo>,
    retirement_tickets: HashMap<String, RetirementTicket>,
    retirement_completion_capability_hashes: HashMap<String, String>,
    completed_unregister_signatures: HashMap<String, Vec<u8>>,
}

#[derive(Debug, Default)]
struct TrackerState {
    peers: HashMap<String, PeerRecord>,
    observed_devices: HashMap<String, ObservedDevice>,
    pending_observed_generations: HashMap<String, u64>,
    pending_observed_order: VecDeque<CacheOrderEntry>,
    next_observed_generation: u64,
    enrolled_devices: HashMap<String, EnrolledDevice>,
    revocations: HashMap<String, RevocationInfo>,
    retirement_tickets: HashMap<String, RetirementTicket>,
    retirement_completion_capability_hashes: HashMap<String, String>,
    completed_unregister_signatures: HashMap<String, Vec<u8>>,
    used_nonces: HashMap<String, UsedDeviceNonce>,
    used_nonce_order: VecDeque<CacheOrderEntry>,
    used_nonce_order_by_peer: HashMap<String, VecDeque<CacheOrderEntry>>,
    used_nonce_counts_by_peer: HashMap<String, usize>,
    next_nonce_generation: u64,
    nonce_cache_clock_unix: Option<i64>,
    last_pruned_unix: Option<i64>,
    security_state_path: Option<PathBuf>,
    _security_state_lock: Option<std::fs::File>,
}

#[derive(Debug, Clone)]
struct UsedDeviceNonce {
    seen_at_unix: i64,
    signature: Vec<u8>,
    completed: bool,
    peer_id: String,
    generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CacheOrderEntry {
    key: String,
    generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeviceProofReplay {
    FreshOrRetryable,
    CompletedReplay,
}

impl TrackerState {
    fn load(security_state_path: Option<PathBuf>) -> Result<Self> {
        let Some(path) = security_state_path else {
            return Ok(Self::default());
        };
        let security_state_lock = acquire_security_state_lock(&path)?;
        let durable = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    anyhow::bail!(
                        "tracker security state must be a regular non-symlink file: {}",
                        path.display()
                    );
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    anyhow::ensure!(
                        metadata.permissions().mode() & 0o077 == 0,
                        "tracker security state permissions are too broad; require 0600: {}",
                        path.display()
                    );
                }
                let bytes = std::fs::read(&path)
                    .with_context(|| format!("read tracker security state: {}", path.display()))?;
                serde_json::from_slice::<DurableTrackerSecurityState>(&bytes)
                    .with_context(|| format!("parse tracker security state: {}", path.display()))?
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                DurableTrackerSecurityState::default()
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("inspect tracker security state: {}", path.display())
                });
            }
        };
        Ok(Self {
            enrolled_devices: durable.enrolled_devices,
            revocations: durable.revocations,
            retirement_tickets: durable.retirement_tickets,
            retirement_completion_capability_hashes: durable
                .retirement_completion_capability_hashes,
            completed_unregister_signatures: durable.completed_unregister_signatures,
            security_state_path: Some(path),
            _security_state_lock: Some(security_state_lock),
            ..Self::default()
        })
    }

    fn persist_security(&self) -> Result<()> {
        let Some(path) = self.security_state_path.as_deref() else {
            return Ok(());
        };
        let durable = DurableTrackerSecurityState {
            enrolled_devices: self.enrolled_devices.clone(),
            revocations: self.revocations.clone(),
            retirement_tickets: self.retirement_tickets.clone(),
            retirement_completion_capability_hashes: self
                .retirement_completion_capability_hashes
                .clone(),
            completed_unregister_signatures: self.completed_unregister_signatures.clone(),
        };
        let bytes =
            serde_json::to_vec_pretty(&durable).context("serialize tracker security state")?;
        crate::config::write_private_file(path, &bytes, true)
            .with_context(|| format!("persist tracker security state: {}", path.display()))
    }

    fn verify_and_record_device_proof(
        &mut self,
        proof: &DeviceProof,
        action: &str,
        payload: &[u8],
        expected_peer_id: &str,
        now_unix: i64,
    ) -> Result<(Vec<u8>, DeviceProofReplay)> {
        let public = verify_device_action(proof, action, payload, expected_peer_id, now_unix)?;
        let cache_now_unix = self
            .nonce_cache_clock_unix
            .map_or(now_unix, |previous| previous.max(now_unix));
        self.nonce_cache_clock_unix = Some(cache_now_unix);
        self.prune_expired_device_nonces(cache_now_unix);
        let nonce_key = format!("{expected_peer_id}:{}", proof.nonce);
        if let Some(existing) = self.used_nonces.get(&nonce_key) {
            anyhow::ensure!(
                existing.signature == proof.signature,
                "device proof nonce was reused for a different request"
            );
            return Ok((
                public.encode_protobuf(),
                if existing.completed {
                    DeviceProofReplay::CompletedReplay
                } else {
                    DeviceProofReplay::FreshOrRetryable
                },
            ));
        }
        self.remember_device_nonce(
            nonce_key,
            expected_peer_id.to_string(),
            proof.signature.clone(),
            cache_now_unix,
        );
        Ok((
            public.encode_protobuf(),
            DeviceProofReplay::FreshOrRetryable,
        ))
    }

    fn mark_device_proof_completed(&mut self, proof: &DeviceProof) {
        let nonce_key = format!("{}:{}", proof.peer_id, proof.nonce);
        if let Some(nonce) = self.used_nonces.get_mut(&nonce_key) {
            nonce.completed = true;
        }
    }

    fn remember_device_nonce(
        &mut self,
        nonce_key: String,
        peer_id: String,
        signature: Vec<u8>,
        seen_at_unix: i64,
    ) {
        while self
            .used_nonce_counts_by_peer
            .get(&peer_id)
            .copied()
            .unwrap_or(0)
            >= MAX_USED_DEVICE_NONCES_PER_PEER
        {
            if !self.evict_oldest_device_nonce_for_peer(&peer_id) {
                break;
            }
        }
        while self.used_nonces.len() >= MAX_USED_DEVICE_NONCES {
            if !self.evict_oldest_device_nonce_global() {
                break;
            }
        }

        let generation = self.allocate_nonce_generation();
        let entry = CacheOrderEntry {
            key: nonce_key.clone(),
            generation,
        };
        self.used_nonces.insert(
            nonce_key,
            UsedDeviceNonce {
                seen_at_unix,
                signature,
                completed: false,
                peer_id: peer_id.clone(),
                generation,
            },
        );
        self.used_nonce_order.push_back(entry.clone());
        self.used_nonce_order_by_peer
            .entry(peer_id.clone())
            .or_default()
            .push_back(entry);
        *self
            .used_nonce_counts_by_peer
            .entry(peer_id.clone())
            .or_default() += 1;
        self.compact_nonce_order_if_needed(&peer_id);
    }

    fn allocate_nonce_generation(&mut self) -> u64 {
        if self.next_nonce_generation == u64::MAX {
            self.used_nonces.clear();
            self.used_nonce_order.clear();
            self.used_nonce_order_by_peer.clear();
            self.used_nonce_counts_by_peer.clear();
            self.next_nonce_generation = 0;
        }
        self.next_nonce_generation += 1;
        self.next_nonce_generation
    }

    fn prune_expired_device_nonces(&mut self, now_unix: i64) {
        while let Some(entry) = self.used_nonce_order.front().cloned() {
            let current = self
                .used_nonces
                .get(&entry.key)
                .filter(|nonce| nonce.generation == entry.generation);
            match current {
                None => {
                    self.used_nonce_order.pop_front();
                }
                Some(nonce) if now_unix.saturating_sub(nonce.seen_at_unix) > USED_NONCE_TTL_SEC => {
                    self.used_nonce_order.pop_front();
                    self.remove_device_nonce_if_current(&entry);
                }
                Some(_) => break,
            }
        }
    }

    fn evict_oldest_device_nonce_global(&mut self) -> bool {
        while let Some(entry) = self.used_nonce_order.pop_front() {
            if self.remove_device_nonce_if_current(&entry) {
                return true;
            }
        }
        false
    }

    fn evict_oldest_device_nonce_for_peer(&mut self, peer_id: &str) -> bool {
        loop {
            let entry = self
                .used_nonce_order_by_peer
                .get_mut(peer_id)
                .and_then(VecDeque::pop_front);
            let Some(entry) = entry else {
                return false;
            };
            if self.remove_device_nonce_if_current(&entry) {
                return true;
            }
        }
    }

    fn remove_device_nonce_if_current(&mut self, entry: &CacheOrderEntry) -> bool {
        let Some(nonce) = self
            .used_nonces
            .get(&entry.key)
            .filter(|nonce| nonce.generation == entry.generation)
            .cloned()
        else {
            return false;
        };
        self.used_nonces.remove(&entry.key);
        let mut remove_peer_queue = false;
        if let Some(count) = self.used_nonce_counts_by_peer.get_mut(&nonce.peer_id) {
            *count = count.saturating_sub(1);
            remove_peer_queue = *count == 0;
        }
        if remove_peer_queue {
            self.used_nonce_counts_by_peer.remove(&nonce.peer_id);
            self.used_nonce_order_by_peer.remove(&nonce.peer_id);
        }
        true
    }

    fn compact_nonce_order_if_needed(&mut self, peer_id: &str) {
        if self.used_nonce_order.len() > MAX_USED_DEVICE_NONCES.saturating_mul(2) {
            let nonces = &self.used_nonces;
            self.used_nonce_order.retain(|entry| {
                nonces
                    .get(&entry.key)
                    .is_some_and(|nonce| nonce.generation == entry.generation)
            });
        }
        if self
            .used_nonce_order_by_peer
            .get(peer_id)
            .is_some_and(|queue| queue.len() > MAX_USED_DEVICE_NONCES_PER_PEER.saturating_mul(2))
        {
            let nonces = &self.used_nonces;
            if let Some(queue) = self.used_nonce_order_by_peer.get_mut(peer_id) {
                queue.retain(|entry| {
                    nonces
                        .get(&entry.key)
                        .is_some_and(|nonce| nonce.generation == entry.generation)
                });
            }
        }
    }
}

fn acquire_security_state_lock(path: &std::path::Path) -> Result<std::fs::File> {
    let mut lock_name = path.as_os_str().to_os_string();
    lock_name.push(".lock");
    let lock_path = PathBuf::from(lock_name);
    if let Some(parent) = lock_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create tracker state lock dir: {}", parent.display()))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(&lock_path)
        .with_context(|| format!("open tracker security state lock: {}", lock_path.display()))?;
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error()).with_context(|| {
                format!(
                    "tracker security state is already locked by another process: {}",
                    path.display()
                )
            });
        }
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&lock_path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod tracker state lock: {}", lock_path.display()))?;
    }
    file.set_len(0)
        .with_context(|| format!("truncate tracker state lock: {}", lock_path.display()))?;
    writeln!(file, "{}", std::process::id())
        .with_context(|| format!("write tracker state lock: {}", lock_path.display()))?;
    file.sync_all()
        .with_context(|| format!("sync tracker state lock: {}", lock_path.display()))?;
    Ok(file)
}

fn validate_tracker_security_config(config: &TrackerServeConfig) -> Result<()> {
    if config.require_device_enrollment && config.admin_token.is_none() {
        anyhow::bail!("--require-device-enrollment requires --admin-token");
    }
    if config.admin_token.is_some() && config.security_state_path.is_none() {
        anyhow::bail!("tracker admin mode requires --security-state-path");
    }
    if config.admin_token.is_some() && config.token.is_none() {
        anyhow::bail!("tracker admin mode requires a separate fleet --token");
    }
    if let Some(path) = config.security_state_path.as_deref() {
        anyhow::ensure!(
            path.is_absolute(),
            "tracker security state path must be absolute: {}",
            path.display()
        );
    }
    if let (Some(token), Some(admin_token)) = (&config.token, &config.admin_token)
        && token_matches(token, admin_token)
    {
        anyhow::bail!("tracker token and tracker admin token must be different");
    }
    Ok(())
}

#[derive(Serialize)]
struct RegisterProofPayload<'a> {
    peer_id: &'a str,
    addrs: &'a [String],
    meta: &'a Option<PeerMeta>,
    retirement_protocol: Option<u32>,
    membership_protocol: Option<u32>,
    membership_enforced: bool,
}

fn register_proof_payload(req: &RegisterRequest) -> Result<Vec<u8>> {
    serde_json::to_vec(&RegisterProofPayload {
        peer_id: &req.peer_id,
        addrs: &req.addrs,
        meta: &req.meta,
        retirement_protocol: req.retirement_protocol,
        membership_protocol: req.membership_protocol,
        membership_enforced: req.membership_enforced,
    })
    .context("serialize register proof payload")
}

impl RegisterRequest {
    #[cfg(test)]
    pub fn signed(
        identity: &crate::libp2p::identity::Keypair,
        addrs: Vec<String>,
        meta: Option<PeerMeta>,
    ) -> Result<Self> {
        Self::signed_with_capabilities(
            identity,
            addrs,
            meta,
            true,
            Some(RETIREMENT_PROTOCOL_VERSION),
        )
    }

    pub fn signed_with_capabilities(
        identity: &crate::libp2p::identity::Keypair,
        addrs: Vec<String>,
        meta: Option<PeerMeta>,
        membership_enforced: bool,
        retirement_protocol: Option<u32>,
    ) -> Result<Self> {
        let mut req = Self {
            peer_id: identity.public().to_peer_id().to_string(),
            addrs,
            meta,
            retirement_protocol,
            membership_protocol: Some(DEVICE_MEMBERSHIP_PROTOCOL_VERSION),
            membership_enforced,
            device_proof: None,
        };
        let payload = register_proof_payload(&req)?;
        req.device_proof = Some(sign_device_action(identity, ACTION_REGISTER, &payload)?);
        Ok(req)
    }
}

fn unregister_proof_payload(peer_id: &str) -> Result<Vec<u8>> {
    serde_json::to_vec(&serde_json::json!({ "peer_id": peer_id }))
        .context("serialize unregister proof payload")
}

impl UnregisterRequest {
    pub fn signed(identity: &crate::libp2p::identity::Keypair) -> Result<Self> {
        let peer_id = identity.public().to_peer_id().to_string();
        let payload = unregister_proof_payload(&peer_id)?;
        Ok(Self {
            peer_id,
            device_proof: Some(sign_device_action(identity, ACTION_UNREGISTER, &payload)?),
        })
    }
}

fn retirement_poll_payload(peer_id: &str) -> Result<Vec<u8>> {
    serde_json::to_vec(&serde_json::json!({ "peer_id": peer_id }))
        .context("serialize retirement poll proof payload")
}

impl RetirementPollRequest {
    pub fn signed(identity: &crate::libp2p::identity::Keypair) -> Result<Self> {
        let peer_id = identity.public().to_peer_id().to_string();
        let payload = retirement_poll_payload(&peer_id)?;
        Ok(Self {
            peer_id,
            device_proof: sign_device_action(identity, ACTION_RETIREMENT_POLL, &payload)?,
        })
    }
}

#[derive(Serialize)]
struct RetirementAckProofPayload<'a> {
    peer_id: &'a str,
    ticket_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    attempt: Option<u32>,
    status: RetirementStatus,
    detail: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completion_capability_hash: &'a Option<String>,
}

fn retirement_ack_payload(req: &RetirementAckRequest) -> Result<Vec<u8>> {
    serde_json::to_vec(&RetirementAckProofPayload {
        peer_id: &req.peer_id,
        ticket_id: &req.ticket_id,
        attempt: (req.attempt != RETIREMENT_INITIAL_ATTEMPT).then_some(req.attempt),
        status: req.status,
        detail: &req.detail,
        completion_capability_hash: &req.completion_capability_hash,
    })
    .context("serialize retirement ack proof payload")
}

impl RetirementAckRequest {
    pub fn signed(
        identity: &crate::libp2p::identity::Keypair,
        ticket_id: String,
        attempt: u32,
        status: RetirementStatus,
        detail: Option<String>,
        completion_capability_hash: Option<String>,
    ) -> Result<Self> {
        let mut req = Self {
            peer_id: identity.public().to_peer_id().to_string(),
            ticket_id,
            attempt,
            status,
            detail,
            completion_capability_hash,
            device_proof: DeviceProof {
                peer_id: String::new(),
                issued_at_unix: 0,
                nonce: String::new(),
                public_key: Vec::new(),
                signature: Vec::new(),
            },
        };
        let payload = retirement_ack_payload(&req)?;
        req.device_proof = sign_device_action(identity, ACTION_RETIREMENT_ACK, &payload)?;
        Ok(req)
    }
}

const MAX_TRACKER_ADDRS: usize = 32;
const MAX_TRACKER_ADDR_BYTES: usize = 512;
const MAX_TRACKER_META_FIELD_BYTES: usize = 256;

pub fn serve(bind: &str, mut config: TrackerServeConfig) -> Result<()> {
    config.token = normalize_configured_token(config.token, "tracker token")?;
    config.admin_token = normalize_configured_admin_token(config.admin_token)?;
    validate_tracker_security_config(&config)?;
    let state = Arc::new(Mutex::new(TrackerState::load(
        config.security_state_path.clone(),
    )?));
    serve_http(bind, config, state)
}

fn serve_http(
    bind: &str,
    config: TrackerServeConfig,
    state: Arc<Mutex<TrackerState>>,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build tracker HTTP runtime")?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(bind)
            .await
            .with_context(|| format!("listen {bind}"))?;
        serve_tracker_listener(listener, tracker_router(config, state)).await
    })
}

async fn serve_tracker_listener(
    listener: tokio::net::TcpListener,
    app: axum::Router,
) -> Result<()> {
    use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
    use hyper_util::server::conn::auto::Builder;
    use hyper_util::service::TowerToHyperService;
    use tower::ServiceExt;

    let connections = Arc::new(tokio::sync::Semaphore::new(TRACKER_HTTP_CONNECTION_LIMIT));
    loop {
        let (stream, remote_addr) = match listener.accept().await {
            Ok(connection) => connection,
            Err(error) => {
                eprintln!("warn: tracker connection accept failed: {error}");
                continue;
            }
        };
        let Ok(connection_permit) = connections.clone().try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let service = app
            .clone()
            .map_request(|request: hyper::Request<hyper::body::Incoming>| {
                request.map(axum::body::Body::new)
            });
        let service = TowerToHyperService::new(service);
        tokio::spawn(async move {
            let _connection_permit = connection_permit;
            let mut builder = Builder::new(TokioExecutor::new()).http1_only();
            builder
                .http1()
                .timer(TokioTimer::new())
                .header_read_timeout(TRACKER_HTTP_IO_TIMEOUT)
                .max_headers(64)
                .max_buf_size(64 * 1024);
            let connection = builder.serve_connection(TokioIo::new(stream), service);
            let result = tokio::time::timeout(TRACKER_HTTP_CONNECTION_LIFETIME, connection).await;
            if let Ok(Err(error)) = result {
                let message = error.to_string();
                if !message.contains("connection closed before message completed")
                    && !message.contains("operation timed out")
                {
                    eprintln!("warn: tracker connection failed remote={remote_addr}: {message}");
                }
            }
        });
    }
}

fn tracker_router(config: TrackerServeConfig, state: Arc<Mutex<TrackerState>>) -> axum::Router {
    let app_state = TrackerAxumState {
        state,
        config,
        inflight: Arc::new(tokio::sync::Semaphore::new(TRACKER_HTTP_INFLIGHT_LIMIT)),
    };
    axum::Router::new()
        .fallback(axum::routing::any(handle_axum_tracker_request))
        .with_state(app_state)
}

#[derive(Clone)]
struct TrackerAxumState {
    state: Arc<Mutex<TrackerState>>,
    config: TrackerServeConfig,
    inflight: Arc<tokio::sync::Semaphore>,
}

async fn handle_axum_tracker_request(
    axum::extract::State(app): axum::extract::State<TrackerAxumState>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let Ok(_permit) = app.inflight.clone().try_acquire_owned() else {
        return axum_text_response(503, "tracker request limit reached\n");
    };
    let (parts, body) = request.into_parts();
    let url = parts
        .uri
        .path_and_query()
        .map_or_else(|| parts.uri.path().to_string(), ToString::to_string);
    let method = parts.method.as_str().to_string();
    let headers = parts
        .headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect();
    let mut buffered = BufferedTrackerRequest {
        url,
        method,
        headers,
        body: Cursor::new(Vec::new()),
    };
    if tracker_request_needs_body(&buffered, &app.config) {
        let read = axum::body::to_bytes(body, tracker_request_body_limit(&buffered));
        match tokio::time::timeout(TRACKER_HTTP_IO_TIMEOUT, read).await {
            Ok(Ok(body)) => buffered.body = Cursor::new(body.to_vec()),
            Ok(Err(_)) => return axum_text_response(413, "payload too large\n"),
            Err(_) => return axum_text_response(408, "request body timeout\n"),
        }
    }
    let tracker_state = Arc::clone(&app.state);
    let tracker_config = app.config.clone();
    let response = match tokio::task::spawn_blocking(move || {
        route_http_request(&tracker_state, &tracker_config, &mut buffered)
            .unwrap_or_else(|error| respond_text(500, &format!("error: {error:#}\n")))
    })
    .await
    {
        Ok(response) => response,
        Err(error) => respond_text(500, &format!("tracker worker failed: {error}\n")),
    };
    tiny_response_to_axum(response)
}

fn axum_text_response(status: u16, body: &'static str) -> axum::response::Response {
    axum::http::Response::builder()
        .status(status)
        .header(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )
        .header(axum::http::header::CACHE_CONTROL, "no-store, private")
        .body(axum::body::Body::from(body))
        .unwrap()
}

fn tiny_response_to_axum(
    response: tiny_http::Response<Cursor<Vec<u8>>>,
) -> axum::response::Response {
    let status = response.status_code().0;
    let headers = response.headers().to_vec();
    let body = response.into_reader().into_inner();
    let mut builder = axum::http::Response::builder().status(status);
    for header in headers {
        builder = builder.header(header.field.as_str().as_str(), header.value.as_str());
    }
    builder.body(axum::body::Body::from(body)).unwrap()
}

trait TrackerHttpRequest {
    fn request_url(&self) -> &str;
    fn request_method(&self) -> &str;
    fn request_header(&self, name: &str) -> Option<String>;
    fn body_reader(&mut self) -> &mut dyn Read;
}

impl TrackerHttpRequest for tiny_http::Request {
    fn request_url(&self) -> &str {
        self.url()
    }

    fn request_method(&self) -> &str {
        self.method().as_str()
    }

    fn request_header(&self, name: &str) -> Option<String> {
        self.headers()
            .iter()
            .find(|header| header.field.as_str().as_str().eq_ignore_ascii_case(name))
            .map(|header| header.value.as_str().to_string())
    }

    fn body_reader(&mut self) -> &mut dyn Read {
        self.as_reader()
    }
}

struct BufferedTrackerRequest {
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    body: Cursor<Vec<u8>>,
}

impl TrackerHttpRequest for BufferedTrackerRequest {
    fn request_url(&self) -> &str {
        &self.url
    }

    fn request_method(&self) -> &str {
        &self.method
    }

    fn request_header(&self, name: &str) -> Option<String> {
        self.headers
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
    }

    fn body_reader(&mut self) -> &mut dyn Read {
        &mut self.body
    }
}

fn tracker_request_needs_body<R: TrackerHttpRequest + ?Sized>(
    req: &R,
    config: &TrackerServeConfig,
) -> bool {
    if req.request_method() != "POST" {
        return false;
    }
    let path = req
        .request_url()
        .split_once('?')
        .map_or(req.request_url(), |(path, _)| path);
    let is_device_endpoint = is_device_request_endpoint(path);
    let has_bounded_device_header = header_value(req, DEVICE_REQUEST_HEADER)
        .is_some_and(|value| value.len() <= MAX_DEVICE_REQUEST_HEADER_BYTES);
    if is_device_endpoint && has_bounded_device_header {
        return false;
    }
    if path.starts_with("/api/v1/admin/")
        && !is_admin_authorized(req, config.admin_token.as_deref())
    {
        return false;
    }
    if let Some(token) = config.token.as_deref()
        && !is_authorized(req, token)
    {
        return false;
    }
    true
}

fn tracker_request_body_limit<R: TrackerHttpRequest + ?Sized>(req: &R) -> usize {
    let path = req
        .request_url()
        .split_once('?')
        .map_or(req.request_url(), |(path, _)| path);
    match path {
        "/api/v1/peers/register" => MAX_TRACKER_REGISTER_BODY_BYTES,
        "/api/v1/devices/retirement/complete" => MAX_RETIREMENT_COMPLETION_BODY_BYTES,
        path if is_device_request_endpoint(path) || path.starts_with("/api/v1/admin/") => {
            MAX_RETIREMENT_DEVICE_BODY_BYTES
        }
        _ => MAX_RETIREMENT_DEVICE_BODY_BYTES,
    }
}

fn is_device_request_endpoint(path: &str) -> bool {
    matches!(
        path,
        "/api/v1/peers/unregister"
            | "/api/v1/devices/retirement/poll"
            | "/api/v1/devices/retirement/ack"
            | "/api/v1/devices/retirement/complete"
    )
}

fn route_http_request<R: TrackerHttpRequest + ?Sized>(
    state: &Arc<Mutex<TrackerState>>,
    config: &TrackerServeConfig,
    req: &mut R,
) -> Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> {
    let url = req.request_url().to_string();
    let method = req.request_method().to_string();

    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (url.as_str(), None),
    };

    let is_device_endpoint = method == "POST" && is_device_request_endpoint(path);
    let has_bounded_device_header = header_value(req, DEVICE_REQUEST_HEADER)
        .is_some_and(|value| value.len() <= MAX_DEVICE_REQUEST_HEADER_BYTES);
    if is_device_endpoint && !has_bounded_device_header {
        let authorized_legacy_body = config
            .token
            .as_deref()
            .is_some_and(|token| is_authorized(req, token));
        if !authorized_legacy_body {
            return Ok(respond_text(
                401,
                "device request header or fleet authorization required\n",
            ));
        }
    } else if !is_device_endpoint
        && let Some(token) = config.token.as_deref()
        && !is_authorized(req, token)
    {
        return Ok(respond_text(401, "unauthorized\n"));
    }

    match (method.as_str(), path) {
        ("GET", "/api/v1/ping") => Ok(respond_text(200, "ok\n")),
        ("POST", "/api/v1/peers/register") => handle_peer_register(state, config, req),
        ("POST", "/api/v1/peers/unregister") => handle_peer_unregister(state, config, req),
        ("POST", "/api/v1/admin/devices/enroll") => handle_admin_enroll(state, config, req),
        ("POST", "/api/v1/admin/devices/retire") => handle_admin_retire(state, config, req),
        ("GET", "/api/v1/admin/devices") => handle_admin_device_list(state, config, req),
        ("GET", "/api/v1/devices/authorize") => handle_peer_authorize(state, config, query),
        ("POST", "/api/v1/devices/retirement/poll") => handle_retirement_poll(state, req),
        ("POST", "/api/v1/devices/retirement/ack") => handle_retirement_ack(state, req),
        ("POST", "/api/v1/devices/retirement/complete") => handle_retirement_complete(state, req),
        ("GET", "/api/v1/peers") => handle_peer_list(state, config, query),
        _ => Ok(respond_text(404, "not found\n")),
    }
}

fn handle_peer_register<R: TrackerHttpRequest + ?Sized>(
    state: &Arc<Mutex<TrackerState>>,
    config: &TrackerServeConfig,
    req: &mut R,
) -> Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> {
    let mut buf = Vec::new();
    let max = max_request_body_bytes();
    req.body_reader()
        .take((max as u64).saturating_add(1))
        .read_to_end(&mut buf)
        .context("read request body")?;
    if buf.len() > max {
        return Ok(respond_text(413, "payload too large\n"));
    }

    let reg: RegisterRequest =
        serde_json::from_slice(&buf).context("parse register request json")?;
    let proof_payload = register_proof_payload(&reg)?;
    let peer_id = reg.peer_id.trim();
    if peer_id.is_empty() {
        return Ok(respond_text(400, "peer_id required\n"));
    }
    let peer_id: PeerId = match peer_id.parse() {
        Ok(peer_id) => peer_id,
        Err(_) => return Ok(respond_text(400, "invalid peer_id\n")),
    };
    let peer_id = peer_id.to_string();

    let addrs = match normalize_register_addrs(reg.addrs) {
        Ok(addrs) => addrs,
        Err(message) => return Ok(respond_text(400, message)),
    };
    if let Err(message) = validate_register_meta(&reg.meta) {
        return Ok(respond_text(400, message));
    }

    let now = OffsetDateTime::now_utc();
    {
        let mut locked = state.lock().unwrap();
        prune_expired(&mut locked, now, config.ttl_sec);
        let now_unix = now.unix_timestamp();
        if effective_revocation(
            &locked,
            &peer_id,
            reg.meta.as_ref().and_then(|meta| meta.device_id.as_deref()),
            reg.meta.as_ref().and_then(|meta| meta.user_id.as_deref()),
        )
        .is_some()
        {
            return Ok(respond_text(403, "device is revoked\n"));
        }
        let verified_device = match reg.device_proof.as_ref() {
            Some(proof) => match locked.verify_and_record_device_proof(
                proof,
                ACTION_REGISTER,
                &proof_payload,
                &peer_id,
                now_unix,
            ) {
                Ok((public_key, _)) => Some(public_key),
                Err(err) => {
                    return Ok(respond_text(403, &format!("invalid device proof: {err}\n")));
                }
            },
            None if config.require_device_enrollment => {
                return Ok(respond_text(403, "signed device proof required\n"));
            }
            None => None,
        };
        let verified_public_key = verified_device.as_ref();

        if let Some(public_key) = verified_public_key {
            record_observed_device(
                &mut locked,
                peer_id.clone(),
                ObservedDevice {
                    public_key: public_key.clone(),
                    device_id: reg.meta.as_ref().and_then(|meta| meta.device_id.clone()),
                    user_id: reg.meta.as_ref().and_then(|meta| meta.user_id.clone()),
                    retirement_protocol: reg.retirement_protocol,
                    membership_protocol: reg.membership_protocol,
                    membership_enforced: reg.membership_enforced,
                    last_seen_unix: now_unix,
                },
            );
        }

        if config.require_device_enrollment {
            let Some(enrolled) = locked.enrolled_devices.get(&peer_id) else {
                if let Some(proof) = reg.device_proof.as_ref() {
                    locked.mark_device_proof_completed(proof);
                }
                return Ok(respond_text(403, "device is not enrolled\n"));
            };
            if verified_public_key.map(Vec::as_slice) != Some(enrolled.public_key.as_slice())
                || enrolled.device_id != reg.meta.as_ref().and_then(|meta| meta.device_id.clone())
                || enrolled.user_id != reg.meta.as_ref().and_then(|meta| meta.user_id.clone())
            {
                return Ok(respond_text(403, "device enrollment binding mismatch\n"));
            }
        }

        if !locked.peers.contains_key(&peer_id) && locked.peers.len() >= max_tracker_peers() {
            return Ok(respond_text(429, "too many registered peers\n"));
        }
        let mut previous_capabilities = None;
        if let Some(enrolled) = locked.enrolled_devices.get_mut(&peer_id)
            && verified_public_key.map(Vec::as_slice) == Some(enrolled.public_key.as_slice())
            && enrolled.device_id == reg.meta.as_ref().and_then(|meta| meta.device_id.clone())
            && enrolled.user_id == reg.meta.as_ref().and_then(|meta| meta.user_id.clone())
        {
            let enrollment_changed = enrolled.retirement_protocol != reg.retirement_protocol
                || enrolled.membership_protocol != reg.membership_protocol
                || enrolled.membership_enforced != reg.membership_enforced;
            if enrollment_changed {
                previous_capabilities = Some((
                    enrolled.retirement_protocol,
                    enrolled.membership_protocol,
                    enrolled.membership_enforced,
                ));
            }
            enrolled.retirement_protocol = reg.retirement_protocol;
            enrolled.membership_protocol = reg.membership_protocol;
            enrolled.membership_enforced = reg.membership_enforced;
        }
        if let Some(previous) = previous_capabilities
            && let Err(error) = locked.persist_security()
        {
            if let Some(enrolled) = locked.enrolled_devices.get_mut(&peer_id) {
                enrolled.retirement_protocol = previous.0;
                enrolled.membership_protocol = previous.1;
                enrolled.membership_enforced = previous.2;
            }
            return Err(error);
        }
        locked.peers.insert(
            peer_id.clone(),
            PeerRecord {
                addrs,
                meta: reg.meta,
                last_seen_unix: now.unix_timestamp(),
            },
        );
        if let Some(proof) = reg.device_proof.as_ref() {
            locked.mark_device_proof_completed(proof);
        }
    }

    respond_json(
        200,
        &RegisterResponse {
            ok: true,
            ttl_sec: config.ttl_sec,
        },
    )
}

fn handle_peer_unregister<R: TrackerHttpRequest + ?Sized>(
    state: &Arc<Mutex<TrackerState>>,
    config: &TrackerServeConfig,
    req: &mut R,
) -> Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> {
    let unregister: UnregisterRequest = match read_device_json_request(
        req,
        "unregister request",
        MAX_RETIREMENT_DEVICE_BODY_BYTES,
    )? {
        Ok(request) => request,
        Err(response) => return Ok(response),
    };
    if unregister.device_proof.is_none()
        && let Some(token) = config.token.as_deref()
        && !is_authorized(req, token)
    {
        return Ok(respond_text(401, "unauthorized\n"));
    }
    let peer_id = unregister.peer_id.trim();
    if peer_id.is_empty() {
        return Ok(respond_text(400, "peer_id required\n"));
    }
    let peer_id: PeerId = match peer_id.parse() {
        Ok(peer_id) => peer_id,
        Err(_) => return Ok(respond_text(400, "invalid peer_id\n")),
    };
    let peer_id = peer_id.to_string();

    let now = OffsetDateTime::now_utc();
    let removed = {
        let mut locked = state.lock().unwrap();
        prune_expired(&mut locked, now, config.ttl_sec);
        if let Some(proof) = unregister.device_proof.as_ref()
            && locked
                .completed_unregister_signatures
                .get(&peer_id)
                .is_some_and(|signature| constant_time_eq(signature, proof.signature.as_slice()))
        {
            return respond_json(
                200,
                &UnregisterResponse {
                    ok: true,
                    removed: false,
                },
            );
        }
        let payload = unregister_proof_payload(&peer_id)?;
        let mut completed_replay = false;
        let verified_public_key = match unregister.device_proof.as_ref() {
            Some(proof) => match locked.verify_and_record_device_proof(
                proof,
                ACTION_UNREGISTER,
                &payload,
                &peer_id,
                now.unix_timestamp(),
            ) {
                Ok((public_key, replay)) => {
                    completed_replay = replay == DeviceProofReplay::CompletedReplay;
                    Some(public_key)
                }
                Err(error) => {
                    return Ok(respond_text(
                        403,
                        &format!("invalid device proof: {error}\n"),
                    ));
                }
            },
            None if config.require_device_enrollment => {
                return Ok(respond_text(403, "signed device proof required\n"));
            }
            None => None,
        };
        if let Some(enrolled) = locked.enrolled_devices.get(&peer_id)
            && verified_public_key.is_some()
            && verified_public_key.as_deref() != Some(enrolled.public_key.as_slice())
        {
            return Ok(respond_text(403, "device enrollment key mismatch\n"));
        }
        if completed_replay {
            return respond_json(
                200,
                &UnregisterResponse {
                    ok: true,
                    removed: false,
                },
            );
        }
        if locked.revocations.contains_key(&peer_id) {
            let Some(proof) = unregister.device_proof.as_ref() else {
                return Ok(respond_text(403, "device is revoked\n"));
            };
            locked.mark_device_proof_completed(proof);
            return respond_json(
                200,
                &UnregisterResponse {
                    ok: true,
                    removed: false,
                },
            );
        }

        let previous_peer = locked.peers.remove(&peer_id);
        let should_remove_enrollment =
            verified_public_key.is_some() && locked.enrolled_devices.contains_key(&peer_id);
        let previous_enrollment = should_remove_enrollment
            .then(|| locked.enrolled_devices.remove(&peer_id))
            .flatten();
        let previous_observation = verified_public_key
            .as_ref()
            .and_then(|_| locked.observed_devices.remove(&peer_id));
        let completed_signature_changed = previous_enrollment.is_some();
        let previous_completed_signature = if completed_signature_changed {
            Some(
                locked.completed_unregister_signatures.insert(
                    peer_id.clone(),
                    unregister
                        .device_proof
                        .as_ref()
                        .context("signed enrollment unregister proof disappeared")?
                        .signature
                        .clone(),
                ),
            )
        } else {
            None
        };
        if previous_enrollment.is_some()
            && let Err(error) = locked.persist_security()
        {
            if let Some(previous_peer) = previous_peer.clone() {
                locked.peers.insert(peer_id.clone(), previous_peer);
            }
            if let Some(previous_enrollment) = previous_enrollment.clone() {
                locked
                    .enrolled_devices
                    .insert(peer_id.clone(), previous_enrollment);
            }
            if let Some(previous_observation) = previous_observation.clone() {
                locked
                    .observed_devices
                    .insert(peer_id.clone(), previous_observation);
            }
            if completed_signature_changed {
                match previous_completed_signature.flatten() {
                    Some(previous) => {
                        locked
                            .completed_unregister_signatures
                            .insert(peer_id.clone(), previous);
                    }
                    None => {
                        locked.completed_unregister_signatures.remove(&peer_id);
                    }
                }
            }
            return Err(error);
        }
        if previous_observation.is_some() {
            clear_pending_observation(&mut locked, &peer_id);
        }
        let removed = previous_peer.is_some()
            || previous_enrollment.is_some()
            || previous_observation.is_some();
        if let Some(proof) = unregister.device_proof.as_ref() {
            locked.mark_device_proof_completed(proof);
        }
        removed
    };

    respond_json(200, &UnregisterResponse { ok: true, removed })
}

fn handle_peer_authorize(
    state: &Arc<Mutex<TrackerState>>,
    config: &TrackerServeConfig,
    query: Option<&str>,
) -> Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> {
    let peer_id = match query.and_then(|query| query_get(query, "peer_id")) {
        Some(peer_id) => urlencoding::decode(peer_id)
            .context("decode membership peer_id")?
            .into_owned(),
        None => return Ok(respond_text(400, "peer_id required\n")),
    };
    let peer_id: PeerId = match peer_id.parse() {
        Ok(peer_id) => peer_id,
        Err(_) => return Ok(respond_text(400, "invalid peer_id\n")),
    };
    let peer_id = peer_id.to_string();
    let locked = state.lock().unwrap();
    let enrolled_device = locked.enrolled_devices.get(&peer_id);
    let enrolled = enrolled_device.is_some();
    let observed_device = locked.observed_devices.get(&peer_id);
    let device_id = enrolled_device
        .and_then(|device| device.device_id.clone())
        .or_else(|| observed_device.and_then(|device| device.device_id.clone()));
    let user_id = enrolled_device
        .and_then(|device| device.user_id.clone())
        .or_else(|| observed_device.and_then(|device| device.user_id.clone()));
    let revocation =
        effective_revocation(&locked, &peer_id, device_id.as_deref(), user_id.as_deref());
    let revoked = revocation.is_some();
    let active = !revoked && (!config.require_device_enrollment || enrolled);
    respond_json(
        200,
        &MembershipResponse {
            peer_id,
            active,
            enrolled,
            revoked,
            strict: config.require_device_enrollment,
            device_id,
            user_id,
            revocation,
        },
    )
}

fn handle_peer_list(
    state: &Arc<Mutex<TrackerState>>,
    config: &TrackerServeConfig,
    query: Option<&str>,
) -> Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> {
    let user_id = match query.and_then(|query| query_get(query, "user_id")) {
        Some(user_id) => Some(
            urlencoding::decode(user_id)
                .context("decode user_id query")?
                .into_owned(),
        ),
        None => None,
    };
    let now = OffsetDateTime::now_utc();

    let (peers, revocations) = {
        let mut locked = state.lock().unwrap();
        prune_expired(&mut locked, now, config.ttl_sec);
        let mut estimated_bytes = 0usize;
        let mut peers = Vec::new();
        for (peer_id, rec) in locked.peers.iter().filter(|(peer_id, rec)| {
            effective_revocation_for_peer(&locked, peer_id).is_none()
                && match (user_id.as_deref(), &rec.meta) {
                    (None, _) => true,
                    (Some(want), Some(meta)) => meta.user_id.as_deref() == Some(want),
                    (Some(_), None) => false,
                }
        }) {
            estimated_bytes = estimated_bytes
                .saturating_add(peer_id.len())
                .saturating_add(rec.addrs.iter().map(String::len).sum::<usize>())
                .saturating_add(
                    rec.meta
                        .as_ref()
                        .map(estimated_peer_meta_bytes)
                        .unwrap_or_default(),
                )
                .saturating_add(128);
            if estimated_bytes > MAX_TRACKER_RESPONSE_BODY_BYTES {
                return Ok(respond_text(
                    413,
                    "tracker peer list exceeds the bounded response size; narrow user_id or reduce stale registrations\n",
                ));
            }
            peers.push(PeerInfo {
                peer_id: peer_id.clone(),
                addrs: rec.addrs.clone(),
                meta: rec.meta.clone(),
                last_seen_unix: rec.last_seen_unix,
            });
        }
        let revocations = locked
            .revocations
            .values()
            .filter(|revocation| {
                user_id
                    .as_deref()
                    .is_none_or(|want| revocation.user_id.as_deref() == Some(want))
            })
            .cloned()
            .collect::<Vec<_>>();
        (peers, revocations)
    };
    respond_json(200, &ListResponse { peers, revocations })
}

fn handle_admin_enroll<R: TrackerHttpRequest + ?Sized>(
    state: &Arc<Mutex<TrackerState>>,
    config: &TrackerServeConfig,
    req: &mut R,
) -> Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> {
    if !is_admin_authorized(req, config.admin_token.as_deref()) {
        return Ok(respond_text(403, "tracker admin authorization required\n"));
    }
    let request: AdminEnrollRequest = read_json_body(req, "admin enroll request")?;
    let peer_id: PeerId = match request.peer_id.trim().parse() {
        Ok(peer_id) => peer_id,
        Err(_) => return Ok(respond_text(400, "invalid peer_id\n")),
    };
    let peer_id = peer_id.to_string();
    let now = OffsetDateTime::now_utc();
    let now_unix = now.unix_timestamp();
    let mut locked = state.lock().unwrap();
    prune_expired(&mut locked, now, config.ttl_sec);
    if locked.revocations.contains_key(&peer_id) {
        return Ok(respond_text(409, "revoked device cannot be enrolled\n"));
    }
    let Some(observed) = locked.observed_devices.get(&peer_id).cloned() else {
        return Ok(respond_text(
            409,
            "device must complete a signed registration before enrollment\n",
        ));
    };
    if !optional_binding_is_canonical(observed.device_id.as_deref())
        || !optional_binding_is_canonical(observed.user_id.as_deref())
    {
        return Ok(respond_text(
            409,
            "device enrollment binding is not canonical\n",
        ));
    }
    if let Some((device_id, user_id)) =
        canonical_binding_pair(observed.device_id.as_deref(), observed.user_id.as_deref())
        && locked
            .enrolled_devices
            .iter()
            .any(|(candidate_peer_id, candidate)| {
                candidate_peer_id != &peer_id
                    && canonical_binding_pair(
                        candidate.device_id.as_deref(),
                        candidate.user_id.as_deref(),
                    ) == Some((device_id, user_id))
            })
    {
        return Ok(respond_text(
            409,
            "device binding is already enrolled to a different peer_id\n",
        ));
    }
    let previous = locked.enrolled_devices.insert(
        peer_id.clone(),
        EnrolledDevice {
            peer_id: peer_id.clone(),
            public_key: observed.public_key,
            device_id: observed.device_id,
            user_id: observed.user_id,
            retirement_protocol: observed.retirement_protocol,
            membership_protocol: observed.membership_protocol,
            membership_enforced: observed.membership_enforced,
            enrolled_at_unix: now_unix,
        },
    );
    if let Err(error) = locked.persist_security() {
        match previous {
            Some(previous) => {
                locked.enrolled_devices.insert(peer_id.clone(), previous);
            }
            None => {
                locked.enrolled_devices.remove(&peer_id);
            }
        }
        return Err(error);
    }
    clear_pending_observation(&mut locked, &peer_id);
    respond_json(
        200,
        &AdminMutationResponse {
            ok: true,
            peer_id,
            ticket: None,
            membership_enforcement_complete: None,
        },
    )
}

fn handle_admin_retire<R: TrackerHttpRequest + ?Sized>(
    state: &Arc<Mutex<TrackerState>>,
    config: &TrackerServeConfig,
    req: &mut R,
) -> Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> {
    if !is_admin_authorized(req, config.admin_token.as_deref()) {
        return Ok(respond_text(403, "tracker admin authorization required\n"));
    }
    let request: AdminRetireRequest = read_json_body(req, "admin retire request")?;
    let peer_id: PeerId = match request.peer_id.trim().parse() {
        Ok(peer_id) => peer_id,
        Err(_) => return Ok(respond_text(400, "invalid peer_id\n")),
    };
    let peer_id = peer_id.to_string();
    let now = OffsetDateTime::now_utc();
    let now_unix = now.unix_timestamp();
    let mut locked = state.lock().unwrap();
    prune_expired(&mut locked, now, config.ttl_sec);

    if let Some(revocation) = locked.revocations.get(&peer_id).cloned() {
        let mut ticket = locked
            .retirement_tickets
            .get(&revocation.ticket_id)
            .cloned();
        if ticket.as_ref().is_some_and(|ticket| {
            ticket.cleanup == RetirementCleanup::RevokeOnly
                && request.cleanup == RetirementCleanup::FullUninstall
        }) {
            if !config.require_device_enrollment {
                return Ok(respond_text(
                    409,
                    "device retirement upgrade requires tracker strict enrollment mode\n",
                ));
            }
            let Some(enrolled) = locked.enrolled_devices.get(&peer_id).cloned() else {
                return Ok(respond_text(409, "retired device enrollment is missing\n"));
            };
            if enrolled.retirement_protocol != Some(RETIREMENT_PROTOCOL_VERSION) {
                return Ok(respond_text(
                    409,
                    "device does not advertise the retirement protocol\n",
                ));
            }
            if canonical_binding_pair(enrolled.device_id.as_deref(), enrolled.user_id.as_deref())
                .is_none()
            {
                return Ok(respond_text(
                    409,
                    "full uninstall requires enrolled device_id and user_id bindings\n",
                ));
            }
            let ticket_id = uuid::Uuid::new_v4().to_string();
            let upgraded_ticket = RetirementTicket {
                ticket_id: ticket_id.clone(),
                peer_id: peer_id.clone(),
                device_id: enrolled.device_id,
                user_id: enrolled.user_id,
                cleanup: RetirementCleanup::FullUninstall,
                issued_at_unix: now_unix,
                attempt: RETIREMENT_INITIAL_ATTEMPT,
                status: RetirementStatus::Pending,
                status_updated_at_unix: now_unix,
                status_detail: None,
            };
            let mut upgraded_revocation = revocation.clone();
            upgraded_revocation.ticket_id = ticket_id.clone();
            locked
                .revocations
                .insert(peer_id.clone(), upgraded_revocation);
            locked
                .retirement_tickets
                .insert(ticket_id.clone(), upgraded_ticket.clone());
            if let Err(error) = locked.persist_security() {
                locked.revocations.insert(peer_id.clone(), revocation);
                locked.retirement_tickets.remove(&ticket_id);
                return Err(error);
            }
            let membership_enforcement_complete =
                fleet_membership_enforcement_complete(&locked, &peer_id);
            return respond_json(
                200,
                &AdminMutationResponse {
                    ok: true,
                    peer_id,
                    ticket: Some(upgraded_ticket),
                    membership_enforcement_complete: Some(membership_enforcement_complete),
                },
            );
        }
        if ticket
            .as_ref()
            .is_some_and(|ticket| ticket.cleanup != request.cleanup)
        {
            return Ok(respond_text(
                409,
                "device is already retired with a different cleanup policy\n",
            ));
        }
        if let Some(previous_ticket) = ticket.clone()
            && matches!(
                previous_ticket.status,
                RetirementStatus::Failed | RetirementStatus::Refused
            )
        {
            if !config.require_device_enrollment {
                return Ok(respond_text(
                    409,
                    "device retirement retry requires tracker strict enrollment mode\n",
                ));
            }
            let Some(enrolled) = locked.enrolled_devices.get(&peer_id) else {
                return Ok(respond_text(409, "retired device enrollment is missing\n"));
            };
            if previous_ticket.cleanup == RetirementCleanup::FullUninstall
                && enrolled.retirement_protocol != Some(RETIREMENT_PROTOCOL_VERSION)
            {
                return Ok(respond_text(
                    409,
                    "device no longer advertises the retirement protocol\n",
                ));
            }
            let mut requeued = previous_ticket.clone();
            let Some(next_attempt) = requeued.attempt.checked_add(1) else {
                return Ok(respond_text(409, "retirement ticket retry limit reached\n"));
            };
            requeued.attempt = next_attempt;
            requeued.status = RetirementStatus::Pending;
            requeued.status_updated_at_unix = now_unix;
            requeued.status_detail = None;
            locked
                .retirement_tickets
                .insert(revocation.ticket_id.clone(), requeued.clone());
            let previous_hash = locked
                .retirement_completion_capability_hashes
                .remove(&revocation.ticket_id);
            if let Err(error) = locked.persist_security() {
                locked
                    .retirement_tickets
                    .insert(revocation.ticket_id.clone(), previous_ticket);
                if let Some(previous_hash) = previous_hash {
                    locked
                        .retirement_completion_capability_hashes
                        .insert(revocation.ticket_id.clone(), previous_hash);
                }
                return Err(error);
            }
            ticket = Some(requeued);
        }
        let membership_enforcement_complete =
            fleet_membership_enforcement_complete(&locked, &peer_id);
        return respond_json(
            200,
            &AdminMutationResponse {
                ok: true,
                peer_id,
                ticket,
                membership_enforcement_complete: Some(membership_enforcement_complete),
            },
        );
    }

    let Some(enrolled) = locked.enrolled_devices.get(&peer_id).cloned() else {
        return Ok(respond_text(
            409,
            "device must be enrolled before retirement\n",
        ));
    };
    if !config.require_device_enrollment {
        return Ok(respond_text(
            409,
            "device retirement requires tracker strict enrollment mode\n",
        ));
    }
    if request.cleanup == RetirementCleanup::FullUninstall
        && enrolled.retirement_protocol != Some(RETIREMENT_PROTOCOL_VERSION)
    {
        return Ok(respond_text(
            409,
            "device does not advertise the retirement protocol\n",
        ));
    }
    if request.cleanup == RetirementCleanup::FullUninstall
        && canonical_binding_pair(enrolled.device_id.as_deref(), enrolled.user_id.as_deref())
            .is_none()
    {
        return Ok(respond_text(
            409,
            "full uninstall requires enrolled device_id and user_id bindings\n",
        ));
    }

    let ticket_id = uuid::Uuid::new_v4().to_string();
    let status = if request.cleanup == RetirementCleanup::RevokeOnly {
        RetirementStatus::Completed
    } else {
        RetirementStatus::Pending
    };
    let ticket = RetirementTicket {
        ticket_id: ticket_id.clone(),
        peer_id: peer_id.clone(),
        device_id: enrolled.device_id.clone(),
        user_id: enrolled.user_id.clone(),
        cleanup: request.cleanup,
        issued_at_unix: now_unix,
        attempt: RETIREMENT_INITIAL_ATTEMPT,
        status,
        status_updated_at_unix: now_unix,
        status_detail: None,
    };
    locked.revocations.insert(
        peer_id.clone(),
        RevocationInfo {
            peer_id: peer_id.clone(),
            device_id: enrolled.device_id,
            user_id: enrolled.user_id,
            revoked_at_unix: now_unix,
            ticket_id: ticket_id.clone(),
        },
    );
    locked
        .retirement_tickets
        .insert(ticket_id.clone(), ticket.clone());
    let previous_peer = locked.peers.remove(&peer_id);
    if let Err(error) = locked.persist_security() {
        locked.revocations.remove(&peer_id);
        locked.retirement_tickets.remove(&ticket_id);
        if let Some(previous_peer) = previous_peer {
            locked.peers.insert(peer_id.clone(), previous_peer);
        }
        return Err(error);
    }
    let membership_enforcement_complete = fleet_membership_enforcement_complete(&locked, &peer_id);
    respond_json(
        200,
        &AdminMutationResponse {
            ok: true,
            peer_id,
            ticket: Some(ticket),
            membership_enforcement_complete: Some(membership_enforcement_complete),
        },
    )
}

fn fleet_membership_enforcement_complete(state: &TrackerState, target_peer_id: &str) -> bool {
    let enrolled_devices_ready = state.enrolled_devices.iter().all(|(peer_id, device)| {
        peer_id == target_peer_id
            || effective_revocation(
                state,
                peer_id,
                device.device_id.as_deref(),
                device.user_id.as_deref(),
            )
            .is_some()
            || (device.membership_protocol == Some(DEVICE_MEMBERSHIP_PROTOCOL_VERSION)
                && device.membership_enforced)
    });
    let active_peers_enrolled = state.peers.keys().all(|peer_id| {
        peer_id == target_peer_id
            || effective_revocation_for_peer(state, peer_id).is_some()
            || state.enrolled_devices.contains_key(peer_id)
    });
    enrolled_devices_ready && active_peers_enrolled
}

fn handle_admin_device_list<R: TrackerHttpRequest + ?Sized>(
    state: &Arc<Mutex<TrackerState>>,
    config: &TrackerServeConfig,
    req: &R,
) -> Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> {
    if !is_admin_authorized(req, config.admin_token.as_deref()) {
        return Ok(respond_text(403, "tracker admin authorization required\n"));
    }
    let mut locked = state.lock().unwrap();
    prune_expired(&mut locked, OffsetDateTime::now_utc(), config.ttl_sec);
    let mut peer_ids = locked
        .observed_devices
        .keys()
        .chain(locked.enrolled_devices.keys())
        .chain(locked.revocations.keys())
        .cloned()
        .collect::<Vec<_>>();
    peer_ids.sort();
    peer_ids.dedup();
    let devices = peer_ids
        .into_iter()
        .map(|peer_id| {
            let enrolled = locked.enrolled_devices.get(&peer_id);
            let observed = locked.observed_devices.get(&peer_id);
            let device_id = enrolled
                .and_then(|device| device.device_id.clone())
                .or_else(|| observed.and_then(|device| device.device_id.clone()));
            let user_id = enrolled
                .and_then(|device| device.user_id.clone())
                .or_else(|| observed.and_then(|device| device.user_id.clone()));
            let revocation =
                effective_revocation(&locked, &peer_id, device_id.as_deref(), user_id.as_deref());
            let ticket = revocation.as_ref().and_then(|revocation| {
                locked
                    .retirement_tickets
                    .get(&revocation.ticket_id)
                    .cloned()
            });
            AdminDeviceInfo {
                device_id,
                user_id,
                enrolled: enrolled.is_some(),
                active: locked.peers.contains_key(&peer_id) && revocation.is_none(),
                revoked: revocation.is_some(),
                retirement_protocol: enrolled
                    .and_then(|device| device.retirement_protocol)
                    .or_else(|| observed.and_then(|device| device.retirement_protocol)),
                membership_protocol: enrolled
                    .and_then(|device| device.membership_protocol)
                    .or_else(|| observed.and_then(|device| device.membership_protocol)),
                membership_enforced: enrolled
                    .map(|device| device.membership_enforced)
                    .or_else(|| observed.map(|device| device.membership_enforced))
                    .unwrap_or(false),
                ticket,
                peer_id,
            }
        })
        .collect();
    respond_json(200, &AdminDeviceListResponse { devices })
}

fn handle_retirement_poll<R: TrackerHttpRequest + ?Sized>(
    state: &Arc<Mutex<TrackerState>>,
    req: &mut R,
) -> Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> {
    let request: RetirementPollRequest = match read_device_json_request(
        req,
        "retirement poll request",
        MAX_RETIREMENT_DEVICE_BODY_BYTES,
    )? {
        Ok(request) => request,
        Err(response) => return Ok(response),
    };
    let peer_id: PeerId = match request.peer_id.trim().parse() {
        Ok(peer_id) => peer_id,
        Err(_) => return Ok(respond_text(400, "invalid peer_id\n")),
    };
    let peer_id = peer_id.to_string();
    let payload = retirement_poll_payload(&peer_id)?;
    let now_unix = OffsetDateTime::now_utc().unix_timestamp();
    let mut locked = state.lock().unwrap();
    let Some(enrolled) = locked.enrolled_devices.get(&peer_id).cloned() else {
        return Ok(respond_text(403, "device is not enrolled\n"));
    };
    let (public_key, _) = match locked.verify_and_record_device_proof(
        &request.device_proof,
        ACTION_RETIREMENT_POLL,
        &payload,
        &peer_id,
        now_unix,
    ) {
        Ok(verified) => verified,
        Err(err) => return Ok(respond_text(403, &format!("invalid device proof: {err}\n"))),
    };
    if public_key != enrolled.public_key {
        return Ok(respond_text(403, "device enrollment key mismatch\n"));
    }
    let ticket = locked.revocations.get(&peer_id).and_then(|revocation| {
        locked
            .retirement_tickets
            .get(&revocation.ticket_id)
            .cloned()
    });
    locked.mark_device_proof_completed(&request.device_proof);
    respond_json(200, &RetirementPollResponse { ticket })
}

fn handle_retirement_ack<R: TrackerHttpRequest + ?Sized>(
    state: &Arc<Mutex<TrackerState>>,
    req: &mut R,
) -> Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> {
    let request: RetirementAckRequest = match read_device_json_request(
        req,
        "retirement ack request",
        MAX_RETIREMENT_DEVICE_BODY_BYTES,
    )? {
        Ok(request) => request,
        Err(response) => return Ok(response),
    };
    if request.detail.as_ref().is_some_and(|detail| {
        detail.len() > MAX_TRACKER_META_FIELD_BYTES || contains_terminal_control(detail)
    }) {
        return Ok(respond_text(400, "invalid retirement status detail\n"));
    }
    let running_capability_hash = match request.status {
        RetirementStatus::Running => {
            let Some(hash) = request.completion_capability_hash.as_deref() else {
                return Ok(respond_text(
                    400,
                    "running retirement ack requires a completion capability hash\n",
                ));
            };
            if !is_lowercase_sha256_hex(hash) {
                return Ok(respond_text(
                    400,
                    "invalid retirement completion capability hash\n",
                ));
            }
            Some(hash.to_string())
        }
        _ if request.completion_capability_hash.is_some() => {
            return Ok(respond_text(
                400,
                "completion capability hash is only valid for a running retirement ack\n",
            ));
        }
        _ => None,
    };
    let peer_id: PeerId = match request.peer_id.trim().parse() {
        Ok(peer_id) => peer_id,
        Err(_) => return Ok(respond_text(400, "invalid peer_id\n")),
    };
    let peer_id = peer_id.to_string();
    let payload = retirement_ack_payload(&request)?;
    let now_unix = OffsetDateTime::now_utc().unix_timestamp();
    let mut locked = state.lock().unwrap();
    let Some(enrolled) = locked.enrolled_devices.get(&peer_id).cloned() else {
        return Ok(respond_text(403, "device is not enrolled\n"));
    };
    let (public_key, replay) = match locked.verify_and_record_device_proof(
        &request.device_proof,
        ACTION_RETIREMENT_ACK,
        &payload,
        &peer_id,
        now_unix,
    ) {
        Ok(verified) => verified,
        Err(err) => return Ok(respond_text(403, &format!("invalid device proof: {err}\n"))),
    };
    if public_key != enrolled.public_key {
        return Ok(respond_text(403, "device enrollment key mismatch\n"));
    }
    let Some(previous_ticket) = locked.retirement_tickets.get(&request.ticket_id).cloned() else {
        return Ok(respond_text(404, "retirement ticket not found\n"));
    };
    if previous_ticket.peer_id != peer_id {
        return Ok(respond_text(403, "retirement ticket target mismatch\n"));
    }
    if request.attempt == 0 || request.attempt != previous_ticket.attempt {
        return Ok(respond_text(409, "retirement ticket attempt mismatch\n"));
    }
    if request.status == RetirementStatus::Completed {
        return Ok(respond_text(
            409,
            "retirement completion requires the ticket completion capability\n",
        ));
    }
    if request.status == RetirementStatus::Running
        && previous_ticket.status == RetirementStatus::Running
        && !locked
            .retirement_completion_capability_hashes
            .contains_key(&request.ticket_id)
    {
        return Ok(respond_text(
            409,
            "running retirement ticket has no completion capability hash\n",
        ));
    }
    if let Some(requested_hash) = running_capability_hash.as_deref()
        && let Some(existing_hash) = locked
            .retirement_completion_capability_hashes
            .get(&request.ticket_id)
        && !constant_time_eq(existing_hash.as_bytes(), requested_hash.as_bytes())
    {
        return Ok(respond_text(
            409,
            "retirement completion capability hash does not match the ticket\n",
        ));
    }
    if replay == DeviceProofReplay::CompletedReplay {
        return respond_json(
            200,
            &RetirementAckResponse {
                ok: true,
                ticket: previous_ticket,
            },
        );
    }
    if previous_ticket.status == request.status {
        let existing_hash = locked
            .retirement_completion_capability_hashes
            .get(&request.ticket_id);
        let hash_matches = match (existing_hash, running_capability_hash.as_ref()) {
            (None, None) => true,
            (Some(existing), Some(requested)) => {
                constant_time_eq(existing.as_bytes(), requested.as_bytes())
            }
            _ => false,
        };
        if previous_ticket.status_detail == request.detail && hash_matches {
            locked.mark_device_proof_completed(&request.device_proof);
            return respond_json(
                200,
                &RetirementAckResponse {
                    ok: true,
                    ticket: previous_ticket,
                },
            );
        }
        return Ok(respond_text(
            409,
            "same-status retirement ACK may not change detail or capability\n",
        ));
    }
    if !retirement_status_transition_allowed(previous_ticket.status, request.status) {
        return Ok(respond_text(409, "invalid retirement status transition\n"));
    }
    let ticket_id = request.ticket_id.clone();
    let previous_capability_hash = locked
        .retirement_completion_capability_hashes
        .get(&ticket_id)
        .cloned();
    if let Some(hash) = running_capability_hash {
        locked
            .retirement_completion_capability_hashes
            .insert(ticket_id.clone(), hash);
    }
    let ticket = locked
        .retirement_tickets
        .get_mut(&ticket_id)
        .context("retirement ticket disappeared while holding tracker state lock")?;
    ticket.status = request.status;
    ticket.status_updated_at_unix = now_unix;
    ticket.status_detail = request.detail;
    let ticket = ticket.clone();
    if let Err(error) = locked.persist_security() {
        locked
            .retirement_tickets
            .insert(ticket_id.clone(), previous_ticket);
        match previous_capability_hash {
            Some(previous_hash) => {
                locked
                    .retirement_completion_capability_hashes
                    .insert(ticket_id, previous_hash);
            }
            None => {
                locked
                    .retirement_completion_capability_hashes
                    .remove(&ticket_id);
            }
        }
        return Err(error);
    }
    locked.mark_device_proof_completed(&request.device_proof);
    respond_json(200, &RetirementAckResponse { ok: true, ticket })
}

fn handle_retirement_complete<R: TrackerHttpRequest + ?Sized>(
    state: &Arc<Mutex<TrackerState>>,
    req: &mut R,
) -> Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> {
    let request: RetirementCompleteRequest = match read_device_json_request(
        req,
        "retirement completion request",
        MAX_RETIREMENT_COMPLETION_BODY_BYTES,
    )? {
        Ok(request) => request,
        Err(response) => return Ok(response),
    };
    let peer_id: PeerId = match request.peer_id.trim().parse() {
        Ok(peer_id) => peer_id,
        Err(_) => return Ok(respond_text(400, "invalid peer_id\n")),
    };
    let peer_id = peer_id.to_string();
    let ticket_id = request.ticket_id.trim();
    if ticket_id != request.ticket_id
        || uuid::Uuid::parse_str(ticket_id)
            .ok()
            .is_none_or(|ticket_uuid| ticket_uuid.to_string() != ticket_id)
    {
        return Ok(respond_text(400, "invalid retirement ticket id\n"));
    }
    let capability_hash = match completion_capability_hash(&request.completion_capability) {
        Ok(hash) => hash,
        Err(_) => {
            return Ok(respond_text(
                400,
                "invalid retirement completion capability\n",
            ));
        }
    };

    let now_unix = OffsetDateTime::now_utc().unix_timestamp();
    let mut locked = state.lock().unwrap();
    let Some(previous_ticket) = locked.retirement_tickets.get(ticket_id).cloned() else {
        return Ok(respond_text(404, "retirement ticket not found\n"));
    };
    if previous_ticket.peer_id != peer_id {
        return Ok(respond_text(403, "retirement ticket target mismatch\n"));
    }
    if !matches!(
        previous_ticket.status,
        RetirementStatus::Running | RetirementStatus::Completed
    ) {
        return Ok(respond_text(
            409,
            "retirement ticket is not awaiting completion\n",
        ));
    }
    let Some(expected_hash) = locked
        .retirement_completion_capability_hashes
        .get(ticket_id)
    else {
        return Ok(respond_text(
            409,
            "retirement ticket has no completion capability\n",
        ));
    };
    if !constant_time_eq(expected_hash.as_bytes(), capability_hash.as_bytes()) {
        return Ok(respond_text(
            403,
            "invalid retirement completion capability\n",
        ));
    }
    if previous_ticket.status == RetirementStatus::Completed {
        return respond_json(
            200,
            &RetirementAckResponse {
                ok: true,
                ticket: previous_ticket,
            },
        );
    }

    let ticket = locked
        .retirement_tickets
        .get_mut(ticket_id)
        .context("retirement ticket disappeared while holding tracker state lock")?;
    ticket.status = RetirementStatus::Completed;
    ticket.status_updated_at_unix = now_unix;
    ticket.status_detail = None;
    let ticket = ticket.clone();
    if let Err(error) = locked.persist_security() {
        locked
            .retirement_tickets
            .insert(ticket_id.to_string(), previous_ticket);
        return Err(error);
    }
    respond_json(200, &RetirementAckResponse { ok: true, ticket })
}

fn is_lowercase_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn retirement_status_transition_allowed(from: RetirementStatus, to: RetirementStatus) -> bool {
    matches!(
        (from, to),
        (RetirementStatus::Pending, RetirementStatus::Running)
            | (RetirementStatus::Pending, RetirementStatus::Failed)
            | (RetirementStatus::Pending, RetirementStatus::Refused)
    )
}

fn read_json_body<T: for<'de> Deserialize<'de>, R: TrackerHttpRequest + ?Sized>(
    req: &mut R,
    label: &str,
) -> Result<T> {
    let mut buf = Vec::new();
    let max = max_request_body_bytes();
    req.body_reader()
        .take((max as u64).saturating_add(1))
        .read_to_end(&mut buf)
        .with_context(|| format!("read {label} body"))?;
    if buf.len() > max {
        anyhow::bail!("{label} payload too large");
    }
    serde_json::from_slice(&buf).with_context(|| format!("parse {label} json"))
}

fn read_bounded_json_body<T: for<'de> Deserialize<'de>, R: TrackerHttpRequest + ?Sized>(
    req: &mut R,
    label: &str,
    max: usize,
) -> Result<std::result::Result<T, tiny_http::Response<std::io::Cursor<Vec<u8>>>>> {
    let mut buf = Vec::new();
    req.body_reader()
        .take((max as u64).saturating_add(1))
        .read_to_end(&mut buf)
        .with_context(|| format!("read {label} body"))?;
    if buf.len() > max {
        return Ok(Err(respond_text(
            413,
            &format!("{label} payload too large\n"),
        )));
    }
    match serde_json::from_slice(&buf) {
        Ok(value) => Ok(Ok(value)),
        Err(_) => Ok(Err(respond_text(400, &format!("invalid {label}\n")))),
    }
}

fn read_device_json_request<T: for<'de> Deserialize<'de>, R: TrackerHttpRequest + ?Sized>(
    req: &mut R,
    label: &str,
    legacy_body_max: usize,
) -> Result<std::result::Result<T, tiny_http::Response<std::io::Cursor<Vec<u8>>>>> {
    let Some(encoded) = header_value(req, DEVICE_REQUEST_HEADER) else {
        return read_bounded_json_body(req, label, legacy_body_max);
    };
    if encoded.len() > MAX_DEVICE_REQUEST_HEADER_BYTES {
        return Ok(Err(respond_text(
            431,
            &format!("{label} header too large\n"),
        )));
    }
    let decoded = match urlencoding::decode(&encoded) {
        Ok(decoded) => decoded,
        Err(_) => return Ok(Err(respond_text(400, &format!("invalid {label} header\n")))),
    };
    match serde_json::from_str(&decoded) {
        Ok(value) => Ok(Ok(value)),
        Err(_) => Ok(Err(respond_text(400, &format!("invalid {label}\n")))),
    }
}

fn is_admin_authorized<R: TrackerHttpRequest + ?Sized>(req: &R, admin_token: Option<&str>) -> bool {
    let Some(admin_token) = admin_token else {
        return false;
    };
    header_value(req, "X-Rustory-Admin-Token")
        .is_some_and(|candidate| token_matches(&candidate, admin_token))
}

#[cfg(test)]
fn max_request_body_bytes() -> usize {
    8 * 1024
}

#[cfg(not(test))]
fn max_request_body_bytes() -> usize {
    MAX_TRACKER_REGISTER_BODY_BYTES
}

#[cfg(test)]
fn max_tracker_peers() -> usize {
    4
}

#[cfg(not(test))]
fn max_tracker_peers() -> usize {
    4096
}

fn normalize_register_addrs(addrs: Vec<String>) -> std::result::Result<Vec<String>, &'static str> {
    if addrs.len() > MAX_TRACKER_ADDRS {
        return Err("too many addrs\n");
    }

    let mut normalized = Vec::with_capacity(addrs.len());
    for addr in addrs {
        let addr = addr.trim();
        if addr.is_empty() {
            continue;
        }
        if addr.len() > MAX_TRACKER_ADDR_BYTES {
            return Err("addr too large\n");
        }
        normalized.push(addr.to_string());
    }
    Ok(normalized)
}

fn validate_register_meta(meta: &Option<PeerMeta>) -> std::result::Result<(), &'static str> {
    let Some(meta) = meta else {
        return Ok(());
    };
    if !optional_binding_is_canonical(meta.device_id.as_deref())
        || !optional_binding_is_canonical(meta.user_id.as_deref())
    {
        return Err("device_id and user_id must be trimmed non-empty values\n");
    }
    for field in [
        meta.device_id.as_deref(),
        meta.hostname.as_deref(),
        meta.user_id.as_deref(),
        meta.version.as_deref(),
        meta.build_revision.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if field.len() > MAX_TRACKER_META_FIELD_BYTES {
            return Err("meta field too large\n");
        }
        if contains_terminal_control(field) {
            return Err("meta field must not contain control characters\n");
        }
    }
    Ok(())
}

fn optional_binding_is_canonical(value: Option<&str>) -> bool {
    value.is_none_or(|value| !value.is_empty() && value.trim() == value)
}

fn canonical_binding_pair<'a>(
    device_id: Option<&'a str>,
    user_id: Option<&'a str>,
) -> Option<(&'a str, &'a str)> {
    let device_id = device_id.filter(|value| optional_binding_is_canonical(Some(value)))?;
    let user_id = user_id.filter(|value| optional_binding_is_canonical(Some(value)))?;
    Some((device_id, user_id))
}

fn effective_revocation(
    state: &TrackerState,
    peer_id: &str,
    device_id: Option<&str>,
    user_id: Option<&str>,
) -> Option<RevocationInfo> {
    if let Some(revocation) = state.revocations.get(peer_id) {
        return Some(revocation.clone());
    }
    let binding = canonical_binding_pair(device_id, user_id)?;
    state.revocations.values().find_map(|revocation| {
        (canonical_binding_pair(
            revocation.device_id.as_deref(),
            revocation.user_id.as_deref(),
        ) == Some(binding))
        .then(|| {
            let mut logical = revocation.clone();
            logical.peer_id = peer_id.to_string();
            logical
        })
    })
}

fn effective_revocation_for_peer(state: &TrackerState, peer_id: &str) -> Option<RevocationInfo> {
    let enrolled = state.enrolled_devices.get(peer_id);
    let observed = state.observed_devices.get(peer_id);
    let record = state.peers.get(peer_id).and_then(|peer| peer.meta.as_ref());
    effective_revocation(
        state,
        peer_id,
        enrolled
            .and_then(|device| device.device_id.as_deref())
            .or_else(|| observed.and_then(|device| device.device_id.as_deref()))
            .or_else(|| record.and_then(|meta| meta.device_id.as_deref())),
        enrolled
            .and_then(|device| device.user_id.as_deref())
            .or_else(|| observed.and_then(|device| device.user_id.as_deref()))
            .or_else(|| record.and_then(|meta| meta.user_id.as_deref())),
    )
}

fn estimated_peer_meta_bytes(meta: &PeerMeta) -> usize {
    [
        meta.device_id.as_deref(),
        meta.hostname.as_deref(),
        meta.user_id.as_deref(),
        meta.version.as_deref(),
        meta.build_revision.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::len)
    .sum::<usize>()
    .saturating_add(64)
}

fn record_observed_device(state: &mut TrackerState, peer_id: String, observed: ObservedDevice) {
    if state.enrolled_devices.contains_key(&peer_id) {
        clear_pending_observation(state, &peer_id);
        state.observed_devices.insert(peer_id, observed);
        return;
    }

    if state.next_observed_generation == u64::MAX {
        state
            .observed_devices
            .retain(|candidate, _| state.enrolled_devices.contains_key(candidate));
        state.pending_observed_generations.clear();
        state.pending_observed_order.clear();
        state.next_observed_generation = 0;
    }
    state.next_observed_generation += 1;
    let generation = state.next_observed_generation;
    state
        .pending_observed_generations
        .insert(peer_id.clone(), generation);
    state.pending_observed_order.push_back(CacheOrderEntry {
        key: peer_id.clone(),
        generation,
    });
    state.observed_devices.insert(peer_id, observed);

    while state.pending_observed_generations.len() > MAX_PENDING_OBSERVED_DEVICES {
        let Some(entry) = state.pending_observed_order.pop_front() else {
            break;
        };
        if state.pending_observed_generations.get(&entry.key) == Some(&entry.generation) {
            state.pending_observed_generations.remove(&entry.key);
            if !state.enrolled_devices.contains_key(&entry.key) {
                state.observed_devices.remove(&entry.key);
            }
        }
    }
    if state.pending_observed_order.len() > MAX_PENDING_OBSERVED_DEVICES.saturating_mul(2) {
        let generations = &state.pending_observed_generations;
        state
            .pending_observed_order
            .retain(|entry| generations.get(&entry.key) == Some(&entry.generation));
    }
}

fn clear_pending_observation(state: &mut TrackerState, peer_id: &str) {
    state.pending_observed_generations.remove(peer_id);
}

fn prune_expired(state: &mut TrackerState, now: OffsetDateTime, ttl_sec: u64) {
    let now_ts = now.unix_timestamp();
    let prune_interval = if ttl_sec == 0 {
        TRACKER_PRUNE_INTERVAL_SEC
    } else {
        i64::try_from(ttl_sec)
            .unwrap_or(i64::MAX)
            .clamp(1, TRACKER_PRUNE_INTERVAL_SEC)
    };
    if ttl_sec == 0 {
        state.peers.clear();
    }
    if state.last_pruned_unix.is_some_and(|last| {
        let elapsed = now_ts.saturating_sub(last);
        (0..prune_interval).contains(&elapsed)
    }) {
        return;
    }
    state.last_pruned_unix = Some(now_ts);
    if ttl_sec != 0 {
        let ttl = i64::try_from(ttl_sec).unwrap_or(i64::MAX);
        state
            .peers
            .retain(|_, rec| now_ts.saturating_sub(rec.last_seen_unix) <= ttl);
    }

    let retained = state
        .peers
        .keys()
        .chain(state.enrolled_devices.keys())
        .cloned()
        .collect::<HashSet<_>>();
    state.observed_devices.retain(|peer_id, device| {
        retained.contains(peer_id)
            || now_ts.saturating_sub(device.last_seen_unix) <= OBSERVED_DEVICE_TTL_SEC
    });
    let observed = &state.observed_devices;
    let enrolled = &state.enrolled_devices;
    state
        .pending_observed_generations
        .retain(|peer_id, _| observed.contains_key(peer_id) && !enrolled.contains_key(peer_id));
}

fn query_get<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    for part in query.split('&') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        if k == key {
            return Some(v);
        }
    }
    None
}

fn is_authorized<R: TrackerHttpRequest + ?Sized>(req: &R, token: &str) -> bool {
    let token = token.trim();
    if token.is_empty() {
        return false;
    }

    // 1) Authorization: Bearer <token>
    if let Some(value) = header_value(req, "Authorization")
        && let Some(rest) = value.strip_prefix("Bearer ")
    {
        return token_matches(rest, token);
    }

    // 2) X-Rustory-Token: <token>
    if let Some(value) = header_value(req, "X-Rustory-Token") {
        return token_matches(&value, token);
    }

    false
}

pub(crate) fn token_matches(candidate: &str, configured: &str) -> bool {
    constant_time_eq(candidate.trim().as_bytes(), configured.trim().as_bytes())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut diff = left.len() ^ right.len();
    let max_len = left.len().max(right.len());
    for index in 0..max_len {
        let left_byte = *left.get(index).unwrap_or(&0);
        let right_byte = *right.get(index).unwrap_or(&0);
        diff |= (left_byte ^ right_byte) as usize;
    }
    diff == 0
}

fn normalize_configured_token(token: Option<String>, label: &str) -> Result<Option<String>> {
    let Some(token) = token else {
        return Ok(None);
    };

    let token = token.trim();
    validate_tracker_token_value(token, label)?;

    Ok(Some(token.to_string()))
}

fn normalize_configured_admin_token(token: Option<String>) -> Result<Option<String>> {
    let Some(token) = token else {
        return Ok(None);
    };
    let token = token.trim();
    validate_tracker_admin_token_value(token)?;
    Ok(Some(token.to_string()))
}

pub fn validate_tracker_token_value(token: &str, label: &str) -> Result<()> {
    if token.is_empty() {
        anyhow::bail!("{label} must not be empty");
    }

    if has_literal_quote_wrapper(token) {
        anyhow::bail!(
            "{label} must not be wrapped in literal quote characters; pass the raw token value"
        );
    }

    if token.chars().any(char::is_control) {
        anyhow::bail!("{label} must not contain control characters");
    }

    Ok(())
}

pub fn validate_tracker_admin_token_value(token: &str) -> Result<()> {
    validate_tracker_token_value(token, "tracker admin token")?;
    #[cfg(test)]
    if token == "admin-token" {
        return Ok(());
    }
    anyhow::ensure!(
        token.len() >= 32,
        "tracker admin token must be at least 32 bytes; generate a random value such as `openssl rand -hex 32`"
    );
    Ok(())
}

pub fn has_literal_quote_wrapper(token: &str) -> bool {
    token.len() >= 2
        && ((token.starts_with('\'') && token.ends_with('\''))
            || (token.starts_with('"') && token.ends_with('"')))
}

fn header_value<R: TrackerHttpRequest + ?Sized>(req: &R, name: &str) -> Option<String> {
    req.request_header(name)
}

#[derive(Clone)]
pub struct TrackerClient {
    base_url: String,
    token: Option<String>,
    admin_token: Option<String>,
}

impl TrackerClient {
    pub fn new(base_url: String, token: Option<String>) -> Self {
        let token = token.and_then(|token| {
            let token = token.trim();
            (!token.is_empty()).then(|| token.to_string())
        });

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            admin_token: None,
        }
    }

    pub fn with_admin_token(mut self, admin_token: Option<String>) -> Self {
        self.admin_token = admin_token.and_then(|token| {
            let token = token.trim();
            (!token.is_empty()).then(|| token.to_string())
        });
        self
    }

    pub fn register(&self, req: &RegisterRequest) -> Result<RegisterResponse> {
        self.validate_bearer_transport()?;
        let url = format!("{}/api/v1/peers/register", self.base_url);
        let body = serde_json::to_vec(req).context("serialize register request")?;

        let token = self.token.clone();
        let resp = crate::http_retry::request_with_retry(
            crate::http_retry::RetryPolicy::tracker(),
            |agent| {
                let mut r = agent.post(&url).header("Content-Type", "application/json");
                if let Some(token) = &token {
                    r = r.header("Authorization", format!("Bearer {}", token.trim()));
                }
                r.send(&body)
            },
        )
        .with_context(|| format!("POST {url}"))?;
        parse_json_response(resp, "register response")
    }

    pub fn unregister(&self, req: &UnregisterRequest) -> Result<UnregisterResponse> {
        self.validate_bearer_transport()?;
        let url = format!("{}/api/v1/peers/unregister", self.base_url);
        let (body, encoded_header) = encode_device_request(req, "unregister request")?;

        let token = self.token.clone();
        let resp = crate::http_retry::request_with_retry(
            crate::http_retry::RetryPolicy::tracker(),
            |agent| {
                let mut r = agent
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .header(DEVICE_REQUEST_HEADER, encoded_header.clone());
                if let Some(token) = &token {
                    r = r.header("Authorization", format!("Bearer {}", token.trim()));
                }
                r.send(&body)
            },
        )
        .with_context(|| format!("POST {url}"))?;
        parse_json_response(resp, "unregister response")
    }

    pub fn list(&self, user_id: Option<&str>) -> Result<ListResponse> {
        self.validate_bearer_transport()?;
        let mut url = format!("{}/api/v1/peers", self.base_url);
        if let Some(user_id) = user_id {
            let encoded = urlencoding::encode(user_id);
            url = format!("{url}?user_id={encoded}");
        }

        let token = self.token.clone();
        let resp = crate::http_retry::request_with_retry(
            crate::http_retry::RetryPolicy::tracker(),
            |agent| {
                let mut r = agent.get(&url);
                if let Some(token) = &token {
                    r = r.header("Authorization", format!("Bearer {}", token.trim()));
                }
                r.call()
            },
        )
        .with_context(|| format!("GET {url}"))?;
        parse_json_response(resp, "list response")
    }

    pub fn authorize_peer(&self, peer_id: &str) -> Result<MembershipResponse> {
        self.validate_bearer_transport()?;
        let url = format!(
            "{}/api/v1/devices/authorize?peer_id={}",
            self.base_url,
            urlencoding::encode(peer_id)
        );
        let token = self.token.clone();
        let resp = crate::http_retry::request_with_retry(
            crate::http_retry::RetryPolicy::tracker(),
            |agent| {
                let mut request = agent.get(&url);
                if let Some(token) = &token {
                    request = request.header("Authorization", format!("Bearer {}", token.trim()));
                }
                request.call()
            },
        )
        .with_context(|| format!("GET {url}"))?;
        parse_json_response(resp, "membership response")
    }

    pub fn poll_retirement(
        &self,
        identity: &crate::libp2p::identity::Keypair,
    ) -> Result<RetirementPollResponse> {
        let request = RetirementPollRequest::signed(identity)?;
        self.post_device_json(
            "/api/v1/devices/retirement/poll",
            &request,
            "retirement poll response",
        )
    }

    pub fn acknowledge_retirement(
        &self,
        identity: &crate::libp2p::identity::Keypair,
        ticket_id: String,
        attempt: u32,
        status: RetirementStatus,
        detail: Option<String>,
        completion_capability_hash: Option<String>,
    ) -> Result<RetirementAckResponse> {
        let request = RetirementAckRequest::signed(
            identity,
            ticket_id,
            attempt,
            status,
            detail,
            completion_capability_hash,
        )?;
        self.post_device_json(
            "/api/v1/devices/retirement/ack",
            &request,
            "retirement ack response",
        )
    }

    pub fn complete_retirement(
        &self,
        peer_id: String,
        ticket_id: String,
        completion_capability: String,
    ) -> Result<RetirementAckResponse> {
        validate_completion_tracker_url(&self.base_url)?;
        let url = format!("{}/api/v1/devices/retirement/complete", self.base_url);
        let (body, encoded_header) = encode_device_request(
            &RetirementCompleteRequest {
                peer_id,
                ticket_id,
                completion_capability,
            },
            "retirement completion request",
        )?;
        let token = self.token.clone();
        let response = crate::http_retry::request_with_retry(
            crate::http_retry::RetryPolicy::tracker(),
            |agent| {
                let mut request = agent
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .header(DEVICE_REQUEST_HEADER, encoded_header.clone());
                if let Some(token) = &token {
                    request = request.header("Authorization", format!("Bearer {}", token.trim()));
                }
                request.send(&body)
            },
        )
        .with_context(|| format!("POST {url}"))?;
        parse_json_response(response, "retirement completion response")
    }

    pub fn admin_enroll(&self, peer_id: String) -> Result<AdminMutationResponse> {
        self.post_admin_json(
            "/api/v1/admin/devices/enroll",
            &AdminEnrollRequest { peer_id },
            "admin enroll response",
        )
    }

    pub fn admin_retire(
        &self,
        peer_id: String,
        cleanup: RetirementCleanup,
    ) -> Result<AdminMutationResponse> {
        self.post_admin_json(
            "/api/v1/admin/devices/retire",
            &AdminRetireRequest { peer_id, cleanup },
            "admin retire response",
        )
    }

    pub fn admin_list_devices(&self) -> Result<AdminDeviceListResponse> {
        let url = format!("{}/api/v1/admin/devices", self.base_url);
        validate_admin_tracker_url(&self.base_url)?;
        let token = self.token.clone();
        let admin_token = self
            .admin_token
            .clone()
            .context("tracker admin token is required")?;
        let resp = crate::http_retry::request_with_retry(
            crate::http_retry::RetryPolicy::tracker(),
            |agent| {
                let mut request = agent
                    .get(&url)
                    .header("X-Rustory-Admin-Token", admin_token.clone());
                if let Some(token) = &token {
                    request = request.header("Authorization", format!("Bearer {}", token.trim()));
                }
                request.call()
            },
        )
        .with_context(|| format!("GET {url}"))?;
        parse_json_response(resp, "admin device list response")
    }

    fn post_device_json<Req: Serialize, Resp: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        request: &Req,
        response_label: &str,
    ) -> Result<Resp> {
        self.validate_bearer_transport()?;
        let url = format!("{}{path}", self.base_url);
        let (body, encoded_header) = encode_device_request(request, "device tracker request")?;
        let token = self.token.clone();
        let resp = crate::http_retry::request_with_retry(
            crate::http_retry::RetryPolicy::tracker(),
            |agent| {
                let mut request = agent
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .header(DEVICE_REQUEST_HEADER, encoded_header.clone());
                if let Some(token) = &token {
                    request = request.header("Authorization", format!("Bearer {}", token.trim()));
                }
                request.send(&body)
            },
        )
        .with_context(|| format!("POST {url}"))?;
        parse_json_response(resp, response_label)
    }

    fn post_admin_json<Req: Serialize, Resp: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        request: &Req,
        response_label: &str,
    ) -> Result<Resp> {
        validate_admin_tracker_url(&self.base_url)?;
        let admin_token = self
            .admin_token
            .clone()
            .context("tracker admin token is required")?;
        let url = format!("{}{path}", self.base_url);
        let body = serde_json::to_vec(request).context("serialize admin tracker request")?;
        let token = self.token.clone();
        let resp = crate::http_retry::request_with_retry(
            crate::http_retry::RetryPolicy::tracker(),
            |agent| {
                let mut request = agent
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .header("X-Rustory-Admin-Token", admin_token.clone());
                if let Some(token) = &token {
                    request = request.header("Authorization", format!("Bearer {}", token.trim()));
                }
                request.send(&body)
            },
        )
        .with_context(|| format!("POST {url}"))?;
        parse_json_response(resp, response_label)
    }

    fn validate_bearer_transport(&self) -> Result<()> {
        if self.token.is_none() && self.admin_token.is_none() {
            return Ok(());
        }
        validate_tracker_bearer_url(&self.base_url)
    }
}

fn validate_tracker_bearer_url(base_url: &str) -> Result<()> {
    let uri: ureq::http::Uri = base_url
        .trim()
        .parse()
        .context("parse tracker URL for bearer transport")?;
    let scheme = uri
        .scheme_str()
        .context("tracker URL must include http:// or https://")?;
    let host = uri.host().context("tracker URL must include a host")?;
    if scheme.eq_ignore_ascii_case("https") {
        return Ok(());
    }
    if scheme.eq_ignore_ascii_case("http") && tracker_host_is_loopback(host) {
        return Ok(());
    }
    anyhow::bail!(
        "refusing to send tracker bearer token over plaintext non-loopback URL: {base_url}"
    )
}

fn tracker_host_is_loopback(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']);
    host.eq_ignore_ascii_case("localhost")
        || host.eq_ignore_ascii_case("localhost.")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|addr| addr.is_loopback())
}

fn encode_device_request<T: Serialize>(request: &T, label: &str) -> Result<(Vec<u8>, String)> {
    let body = serde_json::to_vec(request).with_context(|| format!("serialize {label}"))?;
    let body_text = std::str::from_utf8(&body).context("serialized JSON was not UTF-8")?;
    let encoded = urlencoding::encode(body_text).into_owned();
    anyhow::ensure!(
        encoded.len() <= MAX_DEVICE_REQUEST_HEADER_BYTES,
        "{label} is too large for the authenticated request header"
    );
    Ok((body, encoded))
}

fn parse_json_response<T: for<'de> Deserialize<'de>>(
    mut response: ureq::http::Response<ureq::Body>,
    label: &str,
) -> Result<T> {
    anyhow::ensure!(
        response.status().is_success(),
        "unexpected HTTP status for {label}: {}",
        response.status()
    );
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take((MAX_TRACKER_RESPONSE_BODY_BYTES as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .with_context(|| format!("read {label}"))?;
    anyhow::ensure!(
        bytes.len() <= MAX_TRACKER_RESPONSE_BODY_BYTES,
        "{label} exceeds the bounded tracker response size"
    );
    let text = std::str::from_utf8(&bytes).with_context(|| format!("decode {label} as UTF-8"))?;
    serde_json::from_str(text).with_context(|| format!("parse {label} json"))
}

fn validate_admin_tracker_url(base_url: &str) -> Result<()> {
    crate::device_retirement::validate_admin_tracker_url(base_url)
        .context("tracker admin operations require HTTPS")
}

fn validate_completion_tracker_url(base_url: &str) -> Result<()> {
    crate::device_retirement::validate_retirement_tracker_url(base_url)
        .context("retirement completion requires HTTPS")
}

fn respond_text(code: u16, body: &str) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let mut res = tiny_http::Response::from_data(body.as_bytes().to_vec());
    res = res.with_status_code(code);
    res = res.with_header(
        tiny_http::Header::from_bytes("Content-Type", "text/plain; charset=utf-8").unwrap(),
    );
    res = res
        .with_header(tiny_http::Header::from_bytes("Cache-Control", "no-store, private").unwrap());
    res
}

fn respond_json<T: Serialize>(
    code: u16,
    value: &T,
) -> Result<tiny_http::Response<std::io::Cursor<Vec<u8>>>> {
    let body = serde_json::to_vec(value).context("serialize json")?;
    if body.len() > MAX_TRACKER_RESPONSE_BODY_BYTES {
        return Ok(respond_text(
            413,
            "tracker JSON response exceeds the bounded size\n",
        ));
    }
    let mut res = tiny_http::Response::from_data(body);
    res = res.with_status_code(code);
    res =
        res.with_header(tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap());
    res = res
        .with_header(tiny_http::Header::from_bytes("Cache-Control", "no-store, private").unwrap());
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;

    struct TestServer {
        base_url: String,
        shutdown: Arc<AtomicBool>,
        join: Option<thread::JoinHandle<()>>,
    }

    impl TestServer {
        fn shutdown(mut self) {
            self.shutdown.store(true, Ordering::SeqCst);
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }
    }

    fn start_test_server(ttl_sec: u64, token: Option<String>) -> TestServer {
        start_test_server_with_config(TrackerServeConfig {
            ttl_sec,
            token,
            admin_token: None,
            security_state_path: None,
            require_device_enrollment: false,
        })
    }

    fn start_test_server_with_config(config: TrackerServeConfig) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let bind = format!("127.0.0.1:{}", addr.port());
        let base_url = format!("http://{}", bind);

        let state = Arc::new(Mutex::new(
            TrackerState::load(config.security_state_path.clone()).unwrap(),
        ));
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown2 = shutdown.clone();
        let state2 = state.clone();

        let join = thread::spawn(move || {
            let server = tiny_http::Server::http(&bind).unwrap();
            while !shutdown2.load(Ordering::SeqCst) {
                match server.recv_timeout(Duration::from_millis(50)) {
                    Ok(Some(mut req)) => {
                        let res = route_http_request(&state2, &config, &mut req)
                            .unwrap_or_else(|e| respond_text(500, &format!("error: {e:#}\n")));
                        let _ = req.respond(res);
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        });

        // 서버가 뜰 때까지 짧게 대기(ping).
        for _ in 0..50 {
            let url = format!("{}/api/v1/ping", base_url);
            if ureq::get(&url).call().is_ok() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }

        TestServer {
            base_url,
            shutdown,
            join: Some(join),
        }
    }

    fn assert_ureq_status(err: &anyhow::Error, want: u16) {
        let got = err.chain().find_map(|cause| {
            cause
                .downcast_ref::<ureq::Error>()
                .and_then(|err| match err {
                    ureq::Error::StatusCode(code) => Some(*code),
                    _ => None,
                })
        });
        assert_eq!(got, Some(want));
    }

    #[test]
    fn tracker_register_and_list_end_to_end() {
        let server = start_test_server(60, None);
        let client = TrackerClient::new(server.base_url.clone(), None);

        let user_id = "u 1/2";
        let peer_id = PeerId::random().to_string();
        let req = RegisterRequest {
            peer_id: peer_id.clone(),
            addrs: vec!["/ip4/127.0.0.1/tcp/1234".to_string()],
            meta: Some(PeerMeta {
                user_id: Some(user_id.to_string()),
                device_id: Some("d1".to_string()),
                hostname: None,
                version: Some(crate::build_info::VERSION.to_string()),
                build_revision: Some(crate::build_info::BUILD_REVISION.to_string()),
                build_dirty: Some(crate::build_info::build_dirty()),
            }),
            retirement_protocol: None,
            membership_protocol: None,
            membership_enforced: false,
            device_proof: None,
        };
        let resp = client.register(&req).unwrap();
        assert!(resp.ok);

        let list = client.list(Some(user_id)).unwrap();
        assert_eq!(list.peers.len(), 1);
        assert_eq!(list.peers[0].peer_id, peer_id);

        server.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn axum_tracker_ingress_accepts_signed_header_requests() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = TrackerServeConfig {
            ttl_sec: 60,
            token: Some("fleet-token".to_string()),
            admin_token: None,
            security_state_path: None,
            require_device_enrollment: false,
        };
        let state = Arc::new(Mutex::new(TrackerState::load(None).unwrap()));
        let server = tokio::spawn(async move {
            serve_tracker_listener(listener, tracker_router(config, state))
                .await
                .unwrap();
        });
        let result = tokio::task::spawn_blocking(move || {
            let identity = crate::libp2p::identity::Keypair::generate_ed25519();
            let peer_id = identity.public().to_peer_id().to_string();
            let client =
                TrackerClient::new(format!("http://{address}"), Some("fleet-token".to_string()));
            client.register(&RegisterRequest::signed(&identity, vec![], None).unwrap())?;
            let response = client.unregister(&UnregisterRequest::signed(&identity).unwrap())?;
            anyhow::ensure!(response.removed);
            anyhow::ensure!(
                client
                    .list(None)?
                    .peers
                    .iter()
                    .all(|peer| peer.peer_id != peer_id)
            );
            Result::<()>::Ok(())
        })
        .await
        .unwrap();
        server.abort();
        result.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn axum_tracker_slow_unauthenticated_body_does_not_block_ping() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = TrackerServeConfig {
            ttl_sec: 60,
            token: Some("fleet-token".to_string()),
            admin_token: None,
            security_state_path: None,
            require_device_enrollment: false,
        };
        let state = Arc::new(Mutex::new(TrackerState::load(None).unwrap()));
        let server = tokio::spawn(async move {
            serve_tracker_listener(listener, tracker_router(config, state))
                .await
                .unwrap();
        });
        let status = tokio::task::spawn_blocking(move || {
            let mut slow = std::net::TcpStream::connect(address).unwrap();
            slow.write_all(
                b"POST /api/v1/devices/retirement/poll HTTP/1.1\r\nHost: tracker\r\nContent-Type: application/json\r\nContent-Length: 8192\r\n\r\n{",
            )
            .unwrap();
            slow.flush().unwrap();

            let mut ping = std::net::TcpStream::connect(address).unwrap();
            ping.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            ping.write_all(
                b"GET /api/v1/ping HTTP/1.1\r\nHost: tracker\r\nAuthorization: Bearer fleet-token\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
            let mut response = [0_u8; 128];
            let read = ping.read(&mut response).unwrap();
            String::from_utf8_lossy(&response[..read]).to_string()
        })
        .await
        .unwrap();
        server.abort();
        assert!(status.starts_with("HTTP/1.1 200"), "{status:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tracker_partial_header_is_closed_by_deadline() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = TrackerServeConfig {
            ttl_sec: 60,
            token: Some("fleet-token".to_string()),
            admin_token: None,
            security_state_path: None,
            require_device_enrollment: false,
        };
        let state = Arc::new(Mutex::new(TrackerState::load(None).unwrap()));
        let server = tokio::spawn(async move {
            serve_tracker_listener(listener, tracker_router(config, state))
                .await
                .unwrap();
        });
        let (closed, ping_status) = tokio::task::spawn_blocking(move || {
            let mut partial = std::net::TcpStream::connect(address).unwrap();
            partial
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            partial
                .write_all(b"GET /api/v1/ping HTTP/1.1\r\nHost:")
                .unwrap();
            let mut response = [0_u8; 128];
            let closed = match partial.read(&mut response) {
                Ok(0) => true,
                Ok(read) => String::from_utf8_lossy(&response[..read]).contains("408"),
                Err(error) => !matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ),
            };

            let mut ping = std::net::TcpStream::connect(address).unwrap();
            ping.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            ping.write_all(
                b"GET /api/v1/ping HTTP/1.1\r\nHost: tracker\r\nAuthorization: Bearer fleet-token\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
            let read = ping.read(&mut response).unwrap();
            (
                closed,
                String::from_utf8_lossy(&response[..read]).to_string(),
            )
        })
        .await
        .unwrap();
        server.abort();
        assert!(closed, "partial header connection remained open");
        assert!(ping_status.starts_with("HTTP/1.1 200"), "{ping_status:?}");
    }

    #[test]
    fn unauthenticated_device_body_is_rejected_without_buffering() {
        let config = TrackerServeConfig {
            ttl_sec: 60,
            token: Some("fleet-token".to_string()),
            admin_token: None,
            security_state_path: None,
            require_device_enrollment: false,
        };
        let state = Arc::new(Mutex::new(TrackerState::load(None).unwrap()));
        let mut request = BufferedTrackerRequest {
            url: "/api/v1/devices/retirement/poll".to_string(),
            method: "POST".to_string(),
            headers: vec![("Content-Length".to_string(), "8192".to_string())],
            body: Cursor::new(vec![b'x'; 8192]),
        };

        assert!(!tracker_request_needs_body(&request, &config));
        let response = route_http_request(&state, &config, &mut request).unwrap();
        assert_eq!(response.status_code().0, 401);
        assert_eq!(request.body.position(), 0);
    }

    #[test]
    fn fleet_only_admin_body_is_rejected_without_buffering() {
        let config = TrackerServeConfig {
            ttl_sec: 60,
            token: Some("fleet-token".to_string()),
            admin_token: Some("admin-token".to_string()),
            security_state_path: None,
            require_device_enrollment: true,
        };
        let mut request = BufferedTrackerRequest {
            url: "/api/v1/admin/devices/retire".to_string(),
            method: "POST".to_string(),
            headers: vec![(
                "Authorization".to_string(),
                "Bearer fleet-token".to_string(),
            )],
            body: Cursor::new(vec![b'x'; 8192]),
        };

        assert!(!tracker_request_needs_body(&request, &config));
        let state = Arc::new(Mutex::new(TrackerState::load(None).unwrap()));
        let response = route_http_request(&state, &config, &mut request).unwrap();
        assert_eq!(response.status_code().0, 403);
        assert_eq!(request.body.position(), 0);
    }

    #[test]
    fn tracker_unregister_removes_registered_peer() {
        let server = start_test_server(60, None);
        let client = TrackerClient::new(server.base_url.clone(), None);

        let peer_id = PeerId::random().to_string();
        let req = RegisterRequest {
            peer_id: peer_id.clone(),
            addrs: vec!["/ip4/127.0.0.1/tcp/1234".to_string()],
            meta: Some(PeerMeta {
                user_id: Some("u1".to_string()),
                device_id: Some("d1".to_string()),
                hostname: None,
                version: None,
                build_revision: None,
                build_dirty: None,
            }),
            retirement_protocol: None,
            membership_protocol: None,
            membership_enforced: false,
            device_proof: None,
        };
        let resp = client.register(&req).unwrap();
        assert!(resp.ok);
        assert_eq!(client.list(Some("u1")).unwrap().peers.len(), 1);

        let resp = client
            .unregister(&UnregisterRequest {
                peer_id: peer_id.clone(),
                device_proof: None,
            })
            .unwrap();
        assert!(resp.ok);
        assert!(resp.removed);
        assert!(client.list(Some("u1")).unwrap().peers.is_empty());

        let resp = client
            .unregister(&UnregisterRequest {
                peer_id,
                device_proof: None,
            })
            .unwrap();
        assert!(resp.ok);
        assert!(!resp.removed);

        server.shutdown();
    }

    #[test]
    fn tracker_rejects_without_token() {
        let server = start_test_server(60, Some("secret".to_string()));
        let client = TrackerClient::new(server.base_url.clone(), None);

        let req = RegisterRequest {
            peer_id: PeerId::random().to_string(),
            addrs: vec![],
            meta: None,
            retirement_protocol: None,
            membership_protocol: None,
            membership_enforced: false,
            device_proof: None,
        };

        let err = client.register(&req).unwrap_err();
        assert_ureq_status(&err, 401);

        server.shutdown();
    }

    #[test]
    fn tracker_accepts_with_token() {
        let server = start_test_server(60, Some("secret".to_string()));
        let client = TrackerClient::new(server.base_url.clone(), Some("secret".to_string()));

        let req = RegisterRequest {
            peer_id: PeerId::random().to_string(),
            addrs: vec![],
            meta: None,
            retirement_protocol: None,
            membership_protocol: None,
            membership_enforced: false,
            device_proof: None,
        };
        let resp = client.register(&req).unwrap();
        assert!(resp.ok);

        server.shutdown();
    }

    #[test]
    fn token_matches_uses_trimmed_exact_value() {
        assert!(token_matches(" secret ", "secret"));
        assert!(!token_matches("secret1", "secret"));
        assert!(!token_matches("secret", "secret1"));
    }

    #[test]
    fn oversized_json_response_is_a_bounded_client_error() {
        let response = respond_json(200, &"x".repeat(MAX_TRACKER_RESPONSE_BODY_BYTES + 1)).unwrap();
        assert_eq!(response.status_code().0, 413);
        assert_eq!(
            response
                .headers()
                .iter()
                .find(|header| header.field.equiv("Cache-Control"))
                .map(|header| header.value.as_str()),
            Some("no-store, private")
        );
    }

    #[test]
    fn admin_token_requires_a_separate_high_entropy_length() {
        assert!(validate_tracker_admin_token_value("short").is_err());
        assert!(validate_tracker_admin_token_value(&"a".repeat(32)).is_ok());
    }

    #[test]
    fn tracker_list_json_is_additive_across_legacy_and_new_clients() {
        #[derive(Deserialize)]
        struct LegacyListResponse {
            peers: Vec<PeerInfo>,
        }

        let from_legacy: ListResponse = serde_json::from_str(r#"{"peers":[]}"#).unwrap();
        assert!(from_legacy.peers.is_empty());
        assert!(from_legacy.revocations.is_empty());

        let encoded = serde_json::to_string(&ListResponse {
            peers: Vec::new(),
            revocations: vec![RevocationInfo {
                peer_id: PeerId::random().to_string(),
                device_id: Some("device".to_string()),
                user_id: Some("user".to_string()),
                revoked_at_unix: 1,
                ticket_id: uuid::Uuid::new_v4().to_string(),
            }],
        })
        .unwrap();
        let legacy: LegacyListResponse = serde_json::from_str(&encoded).unwrap();
        assert!(legacy.peers.is_empty());
    }

    #[test]
    fn tracker_huge_ttl_does_not_wrap_and_expire_fresh_peers() {
        let mut state = TrackerState::default();
        state.peers.insert(
            PeerId::random().to_string(),
            PeerRecord {
                addrs: Vec::new(),
                meta: None,
                last_seen_unix: -1,
            },
        );

        prune_expired(
            &mut state,
            OffsetDateTime::from_unix_timestamp(0).unwrap(),
            u64::MAX,
        );

        assert_eq!(state.peers.len(), 1);
    }

    #[test]
    fn pending_signed_device_observation_survives_peer_ttl_but_expires_boundedly() {
        let peer_id = PeerId::random().to_string();
        let mut state = TrackerState::default();
        state.observed_devices.insert(
            peer_id.clone(),
            ObservedDevice {
                public_key: vec![1],
                device_id: Some("new-device".to_string()),
                user_id: Some("u1".to_string()),
                retirement_protocol: None,
                membership_protocol: Some(DEVICE_MEMBERSHIP_PROTOCOL_VERSION),
                membership_enforced: true,
                last_seen_unix: 0,
            },
        );

        prune_expired(
            &mut state,
            OffsetDateTime::from_unix_timestamp(60).unwrap(),
            0,
        );
        assert!(state.observed_devices.contains_key(&peer_id));

        prune_expired(
            &mut state,
            OffsetDateTime::from_unix_timestamp(OBSERVED_DEVICE_TTL_SEC + 1).unwrap(),
            0,
        );
        assert!(!state.observed_devices.contains_key(&peer_id));
    }

    #[test]
    fn pending_device_observations_have_a_hard_lru_bound() {
        let mut state = TrackerState::default();
        for index in 0..=MAX_PENDING_OBSERVED_DEVICES {
            record_observed_device(
                &mut state,
                format!("pending-{index}"),
                ObservedDevice {
                    public_key: vec![1],
                    device_id: None,
                    user_id: None,
                    retirement_protocol: None,
                    membership_protocol: None,
                    membership_enforced: false,
                    last_seen_unix: index as i64,
                },
            );
        }

        assert_eq!(state.observed_devices.len(), MAX_PENDING_OBSERVED_DEVICES);
        assert!(!state.observed_devices.contains_key("pending-0"));
        assert!(
            state
                .observed_devices
                .contains_key(&format!("pending-{MAX_PENDING_OBSERVED_DEVICES}"))
        );
    }

    #[test]
    fn pending_device_observation_refresh_moves_the_lru_position() {
        let mut state = TrackerState::default();
        let observed = |last_seen_unix| ObservedDevice {
            public_key: vec![1],
            device_id: None,
            user_id: None,
            retirement_protocol: None,
            membership_protocol: None,
            membership_enforced: false,
            last_seen_unix,
        };
        for index in 0..MAX_PENDING_OBSERVED_DEVICES {
            record_observed_device(
                &mut state,
                format!("pending-{index}"),
                observed(index as i64),
            );
        }
        record_observed_device(&mut state, "pending-0".to_string(), observed(10_000));
        record_observed_device(&mut state, "pending-new".to_string(), observed(10_001));

        assert!(state.observed_devices.contains_key("pending-0"));
        assert!(!state.observed_devices.contains_key("pending-1"));
        assert_eq!(
            state.pending_observed_generations.len(),
            MAX_PENDING_OBSERVED_DEVICES
        );
    }

    #[test]
    fn zero_peer_ttl_still_throttles_full_observation_pruning() {
        let mut state = TrackerState::default();
        record_observed_device(
            &mut state,
            "pending".to_string(),
            ObservedDevice {
                public_key: vec![1],
                device_id: None,
                user_id: None,
                retirement_protocol: None,
                membership_protocol: None,
                membership_enforced: false,
                last_seen_unix: 0,
            },
        );
        state.peers.insert(
            "active".to_string(),
            PeerRecord {
                addrs: Vec::new(),
                meta: None,
                last_seen_unix: 0,
            },
        );
        let now = OBSERVED_DEVICE_TTL_SEC + 100;
        state.last_pruned_unix = Some(now - 1);
        prune_expired(
            &mut state,
            OffsetDateTime::from_unix_timestamp(now).unwrap(),
            0,
        );
        assert!(state.peers.is_empty());
        assert!(state.observed_devices.contains_key("pending"));

        prune_expired(
            &mut state,
            OffsetDateTime::from_unix_timestamp(now + TRACKER_PRUNE_INTERVAL_SEC).unwrap(),
            0,
        );
        assert!(!state.observed_devices.contains_key("pending"));
        assert!(state.pending_observed_generations.is_empty());
    }

    #[test]
    fn device_nonce_cache_has_per_peer_and_global_hard_bounds() {
        let mut state = TrackerState::default();
        for index in 0..=MAX_USED_DEVICE_NONCES_PER_PEER {
            state.remember_device_nonce(
                format!("peer-a:nonce-{index}"),
                "peer-a".to_string(),
                vec![index as u8],
                index as i64,
            );
        }
        assert_eq!(state.used_nonces.len(), MAX_USED_DEVICE_NONCES_PER_PEER);
        assert_eq!(
            state.used_nonce_counts_by_peer.get("peer-a"),
            Some(&MAX_USED_DEVICE_NONCES_PER_PEER)
        );
        assert!(!state.used_nonces.contains_key("peer-a:nonce-0"));

        let mut state = TrackerState::default();
        for index in 0..=MAX_USED_DEVICE_NONCES {
            state.remember_device_nonce(
                format!("peer-{index}:nonce"),
                format!("peer-{index}"),
                vec![index as u8],
                index as i64,
            );
        }
        assert_eq!(state.used_nonces.len(), MAX_USED_DEVICE_NONCES);
        assert!(!state.used_nonces.contains_key("peer-0:nonce"));
        assert!(
            state
                .used_nonces
                .contains_key(&format!("peer-{MAX_USED_DEVICE_NONCES}:nonce"))
        );
    }

    #[test]
    fn stale_nonce_order_generation_cannot_evict_a_reinserted_key() {
        let mut state = TrackerState::default();
        state.remember_device_nonce("peer:nonce".to_string(), "peer".to_string(), vec![1], 1);
        let stale = state.used_nonce_order.back().unwrap().clone();
        assert!(state.remove_device_nonce_if_current(&stale));
        state.remember_device_nonce("peer:nonce".to_string(), "peer".to_string(), vec![2], 2);

        assert!(!state.remove_device_nonce_if_current(&stale));
        assert_eq!(state.used_nonces["peer:nonce"].signature, vec![2]);
    }

    #[test]
    fn nonce_generation_overflow_resets_all_indexes_consistently() {
        let mut state = TrackerState::default();
        state.remember_device_nonce("old:nonce".to_string(), "old".to_string(), vec![1], 1);
        state.next_nonce_generation = u64::MAX;
        state.remember_device_nonce("new:nonce".to_string(), "new".to_string(), vec![2], 2);

        assert_eq!(state.used_nonces.len(), 1);
        assert!(state.used_nonces.contains_key("new:nonce"));
        assert_eq!(state.used_nonce_order.len(), 1);
        assert_eq!(state.used_nonce_order_by_peer.len(), 1);
        assert_eq!(state.used_nonce_counts_by_peer.get("new"), Some(&1));
    }

    #[test]
    fn tracker_rejects_too_many_register_addrs() {
        let server = start_test_server(60, None);
        let client = TrackerClient::new(server.base_url.clone(), None);

        let req = RegisterRequest {
            peer_id: PeerId::random().to_string(),
            addrs: vec!["/ip4/127.0.0.1/tcp/1234".to_string(); MAX_TRACKER_ADDRS + 1],
            meta: None,
            retirement_protocol: None,
            membership_protocol: None,
            membership_enforced: false,
            device_proof: None,
        };

        let err = client.register(&req).unwrap_err();
        assert_ureq_status(&err, 400);

        server.shutdown();
    }

    #[test]
    fn tracker_rejects_oversized_register_addr_and_meta() {
        let server = start_test_server(60, None);
        let client = TrackerClient::new(server.base_url.clone(), None);

        let oversized_addr = RegisterRequest {
            peer_id: PeerId::random().to_string(),
            addrs: vec!["x".repeat(MAX_TRACKER_ADDR_BYTES + 1)],
            meta: None,
            retirement_protocol: None,
            membership_protocol: None,
            membership_enforced: false,
            device_proof: None,
        };
        let err = client.register(&oversized_addr).unwrap_err();
        assert_ureq_status(&err, 400);

        let oversized_meta = RegisterRequest {
            peer_id: PeerId::random().to_string(),
            addrs: vec![],
            meta: Some(PeerMeta {
                device_id: Some("d1".to_string()),
                hostname: Some("h".repeat(MAX_TRACKER_META_FIELD_BYTES + 1)),
                user_id: Some("u1".to_string()),
                version: None,
                build_revision: None,
                build_dirty: Some(false),
            }),
            retirement_protocol: None,
            membership_protocol: None,
            membership_enforced: false,
            device_proof: None,
        };
        let err = client.register(&oversized_meta).unwrap_err();
        assert_ureq_status(&err, 400);

        server.shutdown();
    }

    #[test]
    fn tracker_rejects_control_characters_in_every_meta_string_field() {
        let controls = [
            "bad\x1bdevice",
            "bad\u{7f}host",
            "bad\u{85}user",
            "bad\nversion",
        ];
        for field_index in 0..5 {
            for control in controls {
                let mut meta = PeerMeta {
                    device_id: Some("device".to_string()),
                    hostname: Some("host".to_string()),
                    user_id: Some("user".to_string()),
                    version: Some("1.2.3".to_string()),
                    build_revision: Some("revision".to_string()),
                    build_dirty: Some(false),
                };
                match field_index {
                    0 => meta.device_id = Some(control.to_string()),
                    1 => meta.hostname = Some(control.to_string()),
                    2 => meta.user_id = Some(control.to_string()),
                    3 => meta.version = Some(control.to_string()),
                    4 => meta.build_revision = Some(control.to_string()),
                    _ => unreachable!(),
                }

                let err = validate_register_meta(&Some(meta)).unwrap_err();
                assert_eq!(err, "meta field must not contain control characters\n");
            }
        }
    }

    #[test]
    fn tracker_http_ingress_rejects_terminal_escape_in_meta() {
        let server = start_test_server(60, None);
        let client = TrackerClient::new(server.base_url.clone(), None);
        let req = RegisterRequest {
            peer_id: PeerId::random().to_string(),
            addrs: vec![],
            meta: Some(PeerMeta {
                device_id: Some("device\x1b]52;c;AAAA\x07".to_string()),
                hostname: Some("host".to_string()),
                user_id: Some("user".to_string()),
                version: Some("1.2.3".to_string()),
                build_revision: Some("revision".to_string()),
                build_dirty: Some(false),
            }),
            retirement_protocol: None,
            membership_protocol: None,
            membership_enforced: false,
            device_proof: None,
        };

        let err = client.register(&req).unwrap_err();
        assert_ureq_status(&err, 400);
        server.shutdown();
    }

    #[test]
    fn bearer_tracker_transport_requires_https_or_loopback() {
        for allowed in [
            "https://tracker.example",
            "http://localhost:8850",
            "http://localhost.:8850",
            "http://127.0.0.1:8850",
            "http://[::1]:8850",
        ] {
            validate_tracker_bearer_url(allowed).unwrap();
        }

        for rejected in [
            "http://tracker.example:8850",
            "http://192.0.2.1:8850",
            "ftp://tracker.example",
            "tracker.example",
        ] {
            assert!(validate_tracker_bearer_url(rejected).is_err(), "{rejected}");
        }
    }

    #[test]
    fn tracker_caps_registered_peer_count() {
        let server = start_test_server(60, None);
        let client = TrackerClient::new(server.base_url.clone(), None);

        for _ in 0..max_tracker_peers() {
            let resp = client
                .register(&RegisterRequest {
                    peer_id: PeerId::random().to_string(),
                    addrs: vec![],
                    meta: None,
                    retirement_protocol: None,
                    membership_protocol: None,
                    membership_enforced: false,
                    device_proof: None,
                })
                .unwrap();
            assert!(resp.ok);
        }

        let err = client
            .register(&RegisterRequest {
                peer_id: PeerId::random().to_string(),
                addrs: vec![],
                meta: None,
                retirement_protocol: None,
                membership_protocol: None,
                membership_enforced: false,
                device_proof: None,
            })
            .unwrap_err();
        assert_ureq_status(&err, 429);

        server.shutdown();
    }

    #[test]
    fn tracker_rejects_blank_configured_token() {
        let err =
            normalize_configured_token(Some(" \t ".to_string()), "tracker token").unwrap_err();
        assert!(format!("{err:#}").contains("tracker token must not be empty"));

        let server = start_test_server(60, Some("   ".to_string()));
        let client = TrackerClient::new(server.base_url.clone(), None);

        let req = RegisterRequest {
            peer_id: PeerId::random().to_string(),
            addrs: vec![],
            meta: None,
            retirement_protocol: None,
            membership_protocol: None,
            membership_enforced: false,
            device_proof: None,
        };

        let err = client.register(&req).unwrap_err();
        assert_ureq_status(&err, 401);

        let client = TrackerClient::new(server.base_url.clone(), Some("   ".to_string()));
        let err = client.register(&req).unwrap_err();
        assert_ureq_status(&err, 401);

        server.shutdown();
    }

    #[test]
    fn tracker_rejects_control_characters_in_configured_token() {
        let err =
            normalize_configured_token(Some("abc\nxyz".to_string()), "tracker token").unwrap_err();
        assert!(format!("{err:#}").contains("must not contain control characters"));
    }

    #[test]
    fn tracker_rejects_literal_quote_wrapped_configured_token() {
        let err =
            normalize_configured_token(Some("'secret-token-value'".to_string()), "tracker token")
                .unwrap_err();
        assert!(format!("{err:#}").contains("literal quote characters"));
    }

    #[test]
    fn tracker_rejects_invalid_peer_id() {
        let server = start_test_server(60, None);
        let client = TrackerClient::new(server.base_url.clone(), None);

        let req = RegisterRequest {
            peer_id: "not-a-peer-id".to_string(),
            addrs: vec![],
            meta: None,
            retirement_protocol: None,
            membership_protocol: None,
            membership_enforced: false,
            device_proof: None,
        };

        let err = client.register(&req).unwrap_err();
        assert_ureq_status(&err, 400);

        server.shutdown();
    }

    #[test]
    fn tracker_client_retries_on_5xx() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let bind = format!("127.0.0.1:{}", addr.port());
        let base_url = format!("http://{}", bind);

        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown2 = shutdown.clone();
        let list_calls = Arc::new(AtomicUsize::new(0));
        let list_calls2 = list_calls.clone();

        let join = thread::spawn(move || {
            let server = tiny_http::Server::http(&bind).unwrap();
            while !shutdown2.load(Ordering::SeqCst) {
                match server.recv_timeout(Duration::from_millis(50)) {
                    Ok(Some(req)) => {
                        let path = req.url().split('?').next().unwrap_or(req.url());
                        let res = match (req.method().as_str(), path) {
                            ("GET", "/api/v1/ping") => respond_text(200, "ok\n"),
                            ("GET", "/api/v1/peers") => {
                                let n = list_calls2.fetch_add(1, Ordering::SeqCst);
                                if n < 2 {
                                    respond_text(500, "temporary error\n")
                                } else {
                                    respond_json(
                                        200,
                                        &ListResponse {
                                            peers: vec![],
                                            revocations: vec![],
                                        },
                                    )
                                    .unwrap_or_else(|e| {
                                        respond_text(500, &format!("error: {e:#}\n"))
                                    })
                                }
                            }
                            _ => respond_text(404, "not found\n"),
                        };
                        let _ = req.respond(res);
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        });

        // 서버가 뜰 때까지 짧게 대기(ping).
        for _ in 0..50 {
            let url = format!("{}/api/v1/ping", base_url);
            if ureq::get(&url).call().is_ok() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }

        let client = TrackerClient::new(base_url.clone(), None);
        let list = client.list(None).unwrap();
        assert!(list.peers.is_empty());
        assert!(list_calls.load(Ordering::SeqCst) >= 3);

        shutdown.store(true, Ordering::SeqCst);
        let _ = join.join();
    }

    #[test]
    fn tracker_device_enrollment_separates_admin_authority() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("tracker-security.json");
        let server = start_test_server_with_config(TrackerServeConfig {
            ttl_sec: 60,
            token: Some("fleet-token".to_string()),
            admin_token: Some("admin-token".to_string()),
            security_state_path: Some(state_path.clone()),
            require_device_enrollment: false,
        });
        let identity = crate::libp2p::identity::Keypair::generate_ed25519();
        let peer_id = identity.public().to_peer_id().to_string();
        let client = TrackerClient::new(server.base_url.clone(), Some("fleet-token".to_string()));
        let request = RegisterRequest::signed(
            &identity,
            vec!["/ip4/127.0.0.1/tcp/1234".to_string()],
            Some(PeerMeta {
                device_id: Some("node0".to_string()),
                hostname: Some("node0".to_string()),
                user_id: Some("u1".to_string()),
                version: Some(crate::build_info::VERSION.to_string()),
                build_revision: Some(crate::build_info::BUILD_REVISION.to_string()),
                build_dirty: Some(false),
            }),
        )
        .unwrap();
        client.register(&request).unwrap();

        let wrong_admin = client
            .clone()
            .with_admin_token(Some("fleet-token".to_string()));
        let err = wrong_admin.admin_enroll(peer_id.clone()).unwrap_err();
        assert_ureq_status(&err, 403);

        let admin = client
            .clone()
            .with_admin_token(Some("admin-token".to_string()));
        let response = admin.admin_enroll(peer_id.clone()).unwrap();
        assert!(response.ok);
        let devices = admin.admin_list_devices().unwrap().devices;
        assert_eq!(devices.len(), 1);
        assert!(devices[0].enrolled);
        assert_eq!(devices[0].peer_id, peer_id);
        server.shutdown();
    }

    #[test]
    fn tracker_device_retirement_is_durable_and_allows_cleanup_only_checkin() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("tracker-security.json");
        let identity = crate::libp2p::identity::Keypair::generate_ed25519();
        let peer_id = identity.public().to_peer_id().to_string();

        let server = start_test_server_with_config(TrackerServeConfig {
            ttl_sec: 60,
            token: Some("fleet-token".to_string()),
            admin_token: Some("admin-token".to_string()),
            security_state_path: Some(state_path.clone()),
            require_device_enrollment: false,
        });
        let client = TrackerClient::new(server.base_url.clone(), Some("fleet-token".to_string()));
        let admin = client
            .clone()
            .with_admin_token(Some("admin-token".to_string()));
        client
            .register(
                &RegisterRequest::signed(
                    &identity,
                    vec!["/ip4/127.0.0.1/tcp/1234".to_string()],
                    Some(PeerMeta {
                        device_id: Some("node0".to_string()),
                        hostname: Some("node0".to_string()),
                        user_id: Some("u1".to_string()),
                        version: Some(crate::build_info::VERSION.to_string()),
                        build_revision: Some(crate::build_info::BUILD_REVISION.to_string()),
                        build_dirty: Some(false),
                    }),
                )
                .unwrap(),
            )
            .unwrap();
        admin.admin_enroll(peer_id.clone()).unwrap();
        server.shutdown();

        let server = start_test_server_with_config(TrackerServeConfig {
            ttl_sec: 60,
            token: Some("fleet-token".to_string()),
            admin_token: Some("admin-token".to_string()),
            security_state_path: Some(state_path.clone()),
            require_device_enrollment: true,
        });
        let client = TrackerClient::new(server.base_url.clone(), Some("fleet-token".to_string()));
        let admin = client
            .clone()
            .with_admin_token(Some("admin-token".to_string()));
        client
            .register(
                &RegisterRequest::signed(
                    &identity,
                    vec!["/ip4/127.0.0.1/tcp/1234".to_string()],
                    Some(PeerMeta {
                        device_id: Some("node0".to_string()),
                        hostname: Some("node0".to_string()),
                        user_id: Some("u1".to_string()),
                        version: Some(crate::build_info::VERSION.to_string()),
                        build_revision: Some(crate::build_info::BUILD_REVISION.to_string()),
                        build_dirty: Some(false),
                    }),
                )
                .unwrap(),
            )
            .unwrap();
        let retired = admin
            .admin_retire(peer_id.clone(), RetirementCleanup::FullUninstall)
            .unwrap();
        let ticket = retired.ticket.unwrap();
        let (completion_capability, completion_capability_hash) =
            crate::device_retirement::generate_completion_capability().unwrap();
        let completion_client = TrackerClient::new(server.base_url.clone(), None);
        assert_eq!(ticket.status, RetirementStatus::Pending);
        assert!(!client.authorize_peer(&peer_id).unwrap().active);
        assert!(client.list(Some("u1")).unwrap().peers.is_empty());
        let already_revoked = completion_client
            .unregister(&UnregisterRequest::signed(&identity).unwrap())
            .unwrap();
        assert!(!already_revoked.removed);
        assert!(client.authorize_peer(&peer_id).unwrap().revoked);

        let pending_error = completion_client
            .complete_retirement(
                peer_id.clone(),
                ticket.ticket_id.clone(),
                completion_capability.clone(),
            )
            .unwrap_err();
        assert_ureq_status(&pending_error, 409);
        let malformed_error = completion_client
            .complete_retirement(peer_id.clone(), ticket.ticket_id.clone(), "A".repeat(64))
            .unwrap_err();
        assert_ureq_status(&malformed_error, 400);
        let malformed_json_error = ureq::post(format!(
            "{}/api/v1/devices/retirement/complete",
            server.base_url
        ))
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer fleet-token")
        .send(b"{" as &[u8])
        .unwrap_err();
        assert!(matches!(malformed_json_error, ureq::Error::StatusCode(400)));
        let oversized_error = ureq::post(format!(
            "{}/api/v1/devices/retirement/complete",
            server.base_url
        ))
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer fleet-token")
        .send(&vec![b'x'; MAX_RETIREMENT_COMPLETION_BODY_BYTES + 1])
        .unwrap_err();
        assert!(matches!(oversized_error, ureq::Error::StatusCode(413)));
        for path in [
            "/api/v1/devices/retirement/poll",
            "/api/v1/devices/retirement/ack",
        ] {
            let oversized_error = ureq::post(format!("{}{path}", server.base_url))
                .header("Content-Type", "application/json")
                .header("Authorization", "Bearer fleet-token")
                .send(&vec![b'x'; MAX_RETIREMENT_DEVICE_BODY_BYTES + 1])
                .unwrap_err();
            assert!(matches!(oversized_error, ureq::Error::StatusCode(413)));
        }

        let err = client
            .register(
                &RegisterRequest::signed(
                    &identity,
                    vec![],
                    Some(PeerMeta {
                        device_id: Some("node0".to_string()),
                        hostname: None,
                        user_id: Some("u1".to_string()),
                        version: None,
                        build_revision: None,
                        build_dirty: None,
                    }),
                )
                .unwrap(),
            )
            .unwrap_err();
        assert_ureq_status(&err, 403);

        let polled = completion_client
            .poll_retirement(&identity)
            .unwrap()
            .ticket
            .unwrap();
        assert_eq!(polled.ticket_id, ticket.ticket_id);
        let failed_request = RetirementAckRequest::signed(
            &identity,
            ticket.ticket_id.clone(),
            ticket.attempt,
            RetirementStatus::Failed,
            Some("transient helper scheduling failure".to_string()),
            None,
        )
        .unwrap();
        let failed: RetirementAckResponse = completion_client
            .post_device_json(
                "/api/v1/devices/retirement/ack",
                &failed_request,
                "failed retirement ack response",
            )
            .unwrap();
        assert_eq!(failed.ticket.status, RetirementStatus::Failed);
        let failed_status_completion = completion_client
            .complete_retirement(
                peer_id.clone(),
                ticket.ticket_id.clone(),
                completion_capability.clone(),
            )
            .unwrap_err();
        assert_ureq_status(&failed_status_completion, 409);
        let requeued = admin
            .admin_retire(peer_id.clone(), RetirementCleanup::FullUninstall)
            .unwrap()
            .ticket
            .unwrap();
        assert_eq!(requeued.status, RetirementStatus::Pending);
        assert_eq!(requeued.attempt, ticket.attempt + 1);
        assert!(requeued.status_detail.is_none());
        server.shutdown();

        let server = start_test_server_with_config(TrackerServeConfig {
            ttl_sec: 60,
            token: Some("fleet-token".to_string()),
            admin_token: Some("admin-token".to_string()),
            security_state_path: Some(state_path.clone()),
            require_device_enrollment: true,
        });
        let client = TrackerClient::new(server.base_url.clone(), Some("fleet-token".to_string()));
        let admin = client
            .clone()
            .with_admin_token(Some("admin-token".to_string()));
        let completion_client = TrackerClient::new(server.base_url.clone(), None);
        let stale_ack_error: anyhow::Error = completion_client
            .post_device_json::<_, RetirementAckResponse>(
                "/api/v1/devices/retirement/ack",
                &failed_request,
                "stale retirement ack response",
            )
            .unwrap_err();
        assert_ureq_status(&stale_ack_error, 409);
        assert_eq!(
            admin
                .admin_list_devices()
                .unwrap()
                .devices
                .into_iter()
                .find(|device| device.peer_id == peer_id)
                .unwrap()
                .ticket
                .unwrap()
                .status,
            RetirementStatus::Pending
        );
        let missing_hash = completion_client
            .acknowledge_retirement(
                &identity,
                ticket.ticket_id.clone(),
                requeued.attempt,
                RetirementStatus::Running,
                None,
                None,
            )
            .unwrap_err();
        assert_ureq_status(&missing_hash, 400);
        let malformed_hash = completion_client
            .acknowledge_retirement(
                &identity,
                ticket.ticket_id.clone(),
                requeued.attempt,
                RetirementStatus::Running,
                None,
                Some("A".repeat(64)),
            )
            .unwrap_err();
        assert_ureq_status(&malformed_hash, 400);
        let mut tampered = RetirementAckRequest::signed(
            &identity,
            ticket.ticket_id.clone(),
            requeued.attempt,
            RetirementStatus::Running,
            None,
            Some(completion_capability_hash.clone()),
        )
        .unwrap();
        tampered.completion_capability_hash = Some("0".repeat(64));
        let tampered_error: anyhow::Error = completion_client
            .post_device_json::<_, RetirementAckResponse>(
                "/api/v1/devices/retirement/ack",
                &tampered,
                "tampered retirement ack response",
            )
            .unwrap_err();
        assert_ureq_status(&tampered_error, 403);
        completion_client
            .acknowledge_retirement(
                &identity,
                ticket.ticket_id.clone(),
                requeued.attempt,
                RetirementStatus::Running,
                None,
                Some(completion_capability_hash.clone()),
            )
            .unwrap();
        let durable_bytes = std::fs::read(&state_path).unwrap();
        let durable: DurableTrackerSecurityState = serde_json::from_slice(&durable_bytes).unwrap();
        assert_eq!(
            durable
                .retirement_completion_capability_hashes
                .get(&ticket.ticket_id),
            Some(&completion_capability_hash)
        );
        assert!(
            !String::from_utf8(durable_bytes)
                .unwrap()
                .contains(&completion_capability)
        );
        let legacy_completed = completion_client
            .acknowledge_retirement(
                &identity,
                ticket.ticket_id.clone(),
                requeued.attempt,
                RetirementStatus::Completed,
                None,
                None,
            )
            .unwrap_err();
        assert_ureq_status(&legacy_completed, 409);
        let failed_after_running = completion_client
            .acknowledge_retirement(
                &identity,
                ticket.ticket_id.clone(),
                requeued.attempt,
                RetirementStatus::Failed,
                Some("late scheduler error".to_string()),
                None,
            )
            .unwrap_err();
        assert_ureq_status(&failed_after_running, 409);
        let (_, mismatched_hash) =
            crate::device_retirement::generate_completion_capability().unwrap();
        let mismatched_running = completion_client
            .acknowledge_retirement(
                &identity,
                ticket.ticket_id.clone(),
                requeued.attempt,
                RetirementStatus::Running,
                None,
                Some(mismatched_hash),
            )
            .unwrap_err();
        assert_ureq_status(&mismatched_running, 409);
        std::fs::remove_file(&state_path).unwrap();
        std::fs::create_dir(&state_path).unwrap();
        let completion_body = serde_json::to_vec(&RetirementCompleteRequest {
            peer_id: peer_id.clone(),
            ticket_id: ticket.ticket_id.clone(),
            completion_capability: completion_capability.clone(),
        })
        .unwrap();
        let persistence_error = ureq::post(format!(
            "{}/api/v1/devices/retirement/complete",
            server.base_url
        ))
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer fleet-token")
        .send(&completion_body)
        .unwrap_err();
        assert!(matches!(persistence_error, ureq::Error::StatusCode(500)));
        std::fs::remove_dir(&state_path).unwrap();
        let other_ticket = completion_client
            .complete_retirement(
                peer_id.clone(),
                uuid::Uuid::new_v4().to_string(),
                completion_capability.clone(),
            )
            .unwrap_err();
        assert_ureq_status(&other_ticket, 404);
        let other_peer = completion_client
            .complete_retirement(
                PeerId::random().to_string(),
                ticket.ticket_id.clone(),
                completion_capability.clone(),
            )
            .unwrap_err();
        assert_ureq_status(&other_peer, 403);
        let (wrong_capability, _) =
            crate::device_retirement::generate_completion_capability().unwrap();
        let wrong_capability_error = completion_client
            .complete_retirement(peer_id.clone(), ticket.ticket_id.clone(), wrong_capability)
            .unwrap_err();
        assert_ureq_status(&wrong_capability_error, 403);
        let completed = completion_client
            .complete_retirement(
                peer_id.clone(),
                ticket.ticket_id.clone(),
                completion_capability.clone(),
            )
            .unwrap();
        assert_eq!(completed.ticket.status, RetirementStatus::Completed);
        let replayed = completion_client
            .complete_retirement(
                peer_id.clone(),
                ticket.ticket_id.clone(),
                completion_capability.clone(),
            )
            .unwrap();
        assert_eq!(replayed.ticket.status, RetirementStatus::Completed);
        server.shutdown();

        let server = start_test_server_with_config(TrackerServeConfig {
            ttl_sec: 60,
            token: Some("fleet-token".to_string()),
            admin_token: Some("admin-token".to_string()),
            security_state_path: Some(state_path.clone()),
            require_device_enrollment: true,
        });
        let client = TrackerClient::new(server.base_url.clone(), Some("fleet-token".to_string()));
        let admin = client
            .clone()
            .with_admin_token(Some("admin-token".to_string()));
        assert!(!client.authorize_peer(&peer_id).unwrap().active);
        let devices = admin.admin_list_devices().unwrap().devices;
        assert!(devices[0].revoked);
        assert_eq!(
            devices[0].ticket.as_ref().unwrap().status,
            RetirementStatus::Completed
        );
        let replayed_after_restart = TrackerClient::new(server.base_url.clone(), None)
            .complete_retirement(peer_id, ticket.ticket_id, completion_capability)
            .unwrap();
        assert_eq!(
            replayed_after_restart.ticket.status,
            RetirementStatus::Completed
        );
        server.shutdown();
    }

    #[test]
    fn tracker_device_proof_exact_retry_is_idempotent_after_completion() {
        let identity = crate::libp2p::identity::Keypair::generate_ed25519();
        let mut state = TrackerState::default();
        let peer_id = identity.public().to_peer_id().to_string();
        let payload = b"payload";
        let proof = sign_device_action(&identity, ACTION_REGISTER, payload).unwrap();
        let now = proof.issued_at_unix;
        let (_, first) = state
            .verify_and_record_device_proof(&proof, ACTION_REGISTER, payload, &peer_id, now)
            .unwrap();
        assert_eq!(first, DeviceProofReplay::FreshOrRetryable);
        state.mark_device_proof_completed(&proof);
        let (_, replay) = state
            .verify_and_record_device_proof(&proof, ACTION_REGISTER, payload, &peer_id, now)
            .unwrap();
        assert_eq!(replay, DeviceProofReplay::CompletedReplay);
    }

    #[test]
    fn retirement_ack_proof_payload_only_adds_completion_hash_when_present() {
        let identity = crate::libp2p::identity::Keypair::generate_ed25519();
        let without_hash = RetirementAckRequest::signed(
            &identity,
            uuid::Uuid::new_v4().to_string(),
            RETIREMENT_INITIAL_ATTEMPT,
            RetirementStatus::Failed,
            None,
            None,
        )
        .unwrap();
        let without_hash_payload = retirement_ack_payload(&without_hash).unwrap();
        assert!(
            !String::from_utf8(without_hash_payload)
                .unwrap()
                .contains("completion_capability_hash")
        );
        assert!(
            !serde_json::to_string(&without_hash)
                .unwrap()
                .contains("attempt")
        );

        let with_hash = RetirementAckRequest::signed(
            &identity,
            uuid::Uuid::new_v4().to_string(),
            RETIREMENT_INITIAL_ATTEMPT,
            RetirementStatus::Running,
            None,
            Some("0".repeat(64)),
        )
        .unwrap();
        let with_hash_payload = retirement_ack_payload(&with_hash).unwrap();
        assert!(
            String::from_utf8(with_hash_payload)
                .unwrap()
                .contains("completion_capability_hash")
        );

        let retry = RetirementAckRequest::signed(
            &identity,
            uuid::Uuid::new_v4().to_string(),
            RETIREMENT_INITIAL_ATTEMPT + 1,
            RetirementStatus::Failed,
            None,
            None,
        )
        .unwrap();
        assert!(
            String::from_utf8(retirement_ack_payload(&retry).unwrap())
                .unwrap()
                .contains("\"attempt\":2")
        );
    }

    #[test]
    fn tracker_client_rejects_oversized_json_responses() {
        let response = ureq::http::Response::builder()
            .status(200)
            .body(ureq::Body::builder().data(vec![b'x'; MAX_TRACKER_RESPONSE_BODY_BYTES + 1]))
            .unwrap();
        let error =
            parse_json_response::<ListResponse>(response, "oversized response").unwrap_err();
        assert!(format!("{error:#}").contains("bounded tracker response size"));
    }

    #[test]
    fn tracker_device_retirement_requires_strict_tracker_and_reports_fleet_coverage() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("tracker-security.json");
        let identity = crate::libp2p::identity::Keypair::generate_ed25519();
        let blocker_identity = crate::libp2p::identity::Keypair::generate_ed25519();
        let peer_id = identity.public().to_peer_id().to_string();
        let meta = Some(PeerMeta {
            device_id: Some("node0".to_string()),
            hostname: Some("node0".to_string()),
            user_id: Some("u1".to_string()),
            version: Some(crate::build_info::VERSION.to_string()),
            build_revision: Some(crate::build_info::BUILD_REVISION.to_string()),
            build_dirty: Some(false),
        });
        let mut blocker_meta = meta.clone().unwrap();
        blocker_meta.device_id = Some("node1".to_string());

        let server = start_test_server_with_config(TrackerServeConfig {
            ttl_sec: 60,
            token: Some("fleet-token".to_string()),
            admin_token: Some("admin-token".to_string()),
            security_state_path: Some(state_path.clone()),
            require_device_enrollment: false,
        });
        let client = TrackerClient::new(server.base_url.clone(), Some("fleet-token".to_string()));
        let admin = client
            .clone()
            .with_admin_token(Some("admin-token".to_string()));
        client
            .register(
                &RegisterRequest::signed_with_capabilities(
                    &identity,
                    vec!["/ip4/127.0.0.1/tcp/1234".to_string()],
                    meta.clone(),
                    false,
                    Some(RETIREMENT_PROTOCOL_VERSION),
                )
                .unwrap(),
            )
            .unwrap();
        admin.admin_enroll(peer_id.clone()).unwrap();
        client
            .register(
                &RegisterRequest::signed_with_capabilities(
                    &blocker_identity,
                    vec!["/ip4/127.0.0.1/tcp/1235".to_string()],
                    Some(blocker_meta.clone()),
                    false,
                    None,
                )
                .unwrap(),
            )
            .unwrap();
        admin
            .admin_enroll(blocker_identity.public().to_peer_id().to_string())
            .unwrap();
        let error = admin
            .admin_retire(peer_id.clone(), RetirementCleanup::RevokeOnly)
            .unwrap_err();
        assert_ureq_status(&error, 409);
        server.shutdown();

        let server = start_test_server_with_config(TrackerServeConfig {
            ttl_sec: 60,
            token: Some("fleet-token".to_string()),
            admin_token: Some("admin-token".to_string()),
            security_state_path: Some(state_path.clone()),
            require_device_enrollment: true,
        });
        let client = TrackerClient::new(server.base_url.clone(), Some("fleet-token".to_string()));
        let admin = client
            .clone()
            .with_admin_token(Some("admin-token".to_string()));
        client
            .register(
                &RegisterRequest::signed_with_capabilities(
                    &identity,
                    vec!["/ip4/127.0.0.1/tcp/1234".to_string()],
                    meta.clone(),
                    false,
                    Some(RETIREMENT_PROTOCOL_VERSION),
                )
                .unwrap(),
            )
            .unwrap();
        let retired = admin
            .admin_retire(peer_id.clone(), RetirementCleanup::RevokeOnly)
            .unwrap();
        assert_eq!(
            retired.ticket.as_ref().unwrap().status,
            RetirementStatus::Completed
        );
        assert_eq!(retired.membership_enforcement_complete, Some(false));
        let revoke_ticket_id = retired.ticket.as_ref().unwrap().ticket_id.clone();
        let upgraded = admin
            .admin_retire(peer_id.clone(), RetirementCleanup::FullUninstall)
            .unwrap();
        assert_eq!(
            upgraded.ticket.as_ref().unwrap().cleanup,
            RetirementCleanup::FullUninstall
        );
        assert_eq!(
            upgraded.ticket.as_ref().unwrap().status,
            RetirementStatus::Pending
        );
        assert_ne!(
            upgraded.ticket.as_ref().unwrap().ticket_id,
            revoke_ticket_id
        );
        let downgrade = admin
            .admin_retire(peer_id, RetirementCleanup::RevokeOnly)
            .unwrap_err();
        assert_ureq_status(&downgrade, 409);
        server.shutdown();
    }

    #[test]
    fn fleet_coverage_accepts_offline_strict_devices_and_flags_active_legacy_peers() {
        let mut state = TrackerState::default();
        for peer_id in ["target", "offline-strict"] {
            state.enrolled_devices.insert(
                peer_id.to_string(),
                EnrolledDevice {
                    peer_id: peer_id.to_string(),
                    public_key: vec![1],
                    device_id: None,
                    user_id: None,
                    retirement_protocol: None,
                    membership_protocol: Some(DEVICE_MEMBERSHIP_PROTOCOL_VERSION),
                    membership_enforced: true,
                    enrolled_at_unix: 1,
                },
            );
        }
        assert!(fleet_membership_enforcement_complete(&state, "target"));

        state.peers.insert(
            "active-legacy".to_string(),
            PeerRecord {
                addrs: Vec::new(),
                meta: None,
                last_seen_unix: 1,
            },
        );
        assert!(!fleet_membership_enforcement_complete(&state, "target"));
    }

    #[test]
    fn tracker_device_strict_unregister_requires_bound_identity_proof() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("tracker-security.json");
        let identity = crate::libp2p::identity::Keypair::generate_ed25519();
        let peer_id = identity.public().to_peer_id().to_string();
        let meta = Some(PeerMeta {
            device_id: Some("node0".to_string()),
            hostname: None,
            user_id: Some("u1".to_string()),
            version: None,
            build_revision: None,
            build_dirty: None,
        });

        let server = start_test_server_with_config(TrackerServeConfig {
            ttl_sec: 60,
            token: Some("fleet-token".to_string()),
            admin_token: Some("admin-token".to_string()),
            security_state_path: Some(state_path.clone()),
            require_device_enrollment: false,
        });
        let client = TrackerClient::new(server.base_url.clone(), Some("fleet-token".to_string()));
        let admin = client
            .clone()
            .with_admin_token(Some("admin-token".to_string()));
        client
            .register(&RegisterRequest::signed(&identity, vec![], meta.clone()).unwrap())
            .unwrap();
        admin.admin_enroll(peer_id.clone()).unwrap();
        server.shutdown();

        let server = start_test_server_with_config(TrackerServeConfig {
            ttl_sec: 60,
            token: Some("fleet-token".to_string()),
            admin_token: Some("admin-token".to_string()),
            security_state_path: Some(state_path.clone()),
            require_device_enrollment: true,
        });
        let client = TrackerClient::new(server.base_url.clone(), Some("fleet-token".to_string()));
        let proof_only_client = TrackerClient::new(server.base_url.clone(), None);
        client
            .register(&RegisterRequest::signed(&identity, vec![], meta.clone()).unwrap())
            .unwrap();

        let unsigned = client
            .unregister(&UnregisterRequest {
                peer_id: peer_id.clone(),
                device_proof: None,
            })
            .unwrap_err();
        assert_ureq_status(&unsigned, 403);
        let unsigned_without_fleet_token = proof_only_client
            .unregister(&UnregisterRequest {
                peer_id: peer_id.clone(),
                device_proof: None,
            })
            .unwrap_err();
        assert_ureq_status(&unsigned_without_fleet_token, 401);

        let other = crate::libp2p::identity::Keypair::generate_ed25519();
        let mut forged = UnregisterRequest::signed(&other).unwrap();
        forged.peer_id = peer_id.clone();
        let forged_error = client.unregister(&forged).unwrap_err();
        assert_ureq_status(&forged_error, 403);

        let signed_unregister = UnregisterRequest::signed(&identity).unwrap();
        let removed = proof_only_client.unregister(&signed_unregister).unwrap();
        assert!(removed.removed);
        assert!(
            !proof_only_client
                .unregister(&signed_unregister)
                .unwrap()
                .removed
        );
        let membership = client.authorize_peer(&peer_id).unwrap();
        assert!(!membership.enrolled);
        assert!(!membership.active);
        server.shutdown();

        let server = start_test_server_with_config(TrackerServeConfig {
            ttl_sec: 60,
            token: Some("fleet-token".to_string()),
            admin_token: Some("admin-token".to_string()),
            security_state_path: Some(state_path),
            require_device_enrollment: true,
        });
        let client = TrackerClient::new(server.base_url.clone(), Some("fleet-token".to_string()));
        let admin = client
            .clone()
            .with_admin_token(Some("admin-token".to_string()));
        let membership = client.authorize_peer(&peer_id).unwrap();
        assert!(!membership.enrolled);
        assert!(!membership.active);
        let denied = client
            .register(&RegisterRequest::signed(&identity, vec![], meta.clone()).unwrap())
            .unwrap_err();
        assert_ureq_status(&denied, 403);
        admin.admin_enroll(peer_id.clone()).unwrap();
        client
            .register(&RegisterRequest::signed(&identity, vec![], meta).unwrap())
            .unwrap();
        let membership = client.authorize_peer(&peer_id).unwrap();
        assert!(membership.enrolled);
        assert!(membership.active);
        let proof_only_client = TrackerClient::new(server.base_url.clone(), None);
        let replay = proof_only_client.unregister(&signed_unregister).unwrap();
        assert!(!replay.removed);
        let membership = client.authorize_peer(&peer_id).unwrap();
        assert!(membership.enrolled);
        assert!(membership.active);
        server.shutdown();
    }

    #[test]
    fn tracker_strict_mode_observes_signed_unknown_device_without_activating_it() {
        let dir = tempfile::tempdir().unwrap();
        let identity = crate::libp2p::identity::Keypair::generate_ed25519();
        let peer_id = identity.public().to_peer_id().to_string();
        let meta = Some(PeerMeta {
            device_id: Some("new-mini".to_string()),
            hostname: Some("new-mini.example".to_string()),
            user_id: Some("u1".to_string()),
            version: Some("1.0.53".to_string()),
            build_revision: None,
            build_dirty: None,
        });
        let server = start_test_server_with_config(TrackerServeConfig {
            ttl_sec: 60,
            token: Some("fleet-token".to_string()),
            admin_token: Some("admin-token".to_string()),
            security_state_path: Some(dir.path().join("tracker-security.json")),
            require_device_enrollment: true,
        });
        let client = TrackerClient::new(server.base_url.clone(), Some("fleet-token".to_string()));
        let admin = client
            .clone()
            .with_admin_token(Some("admin-token".to_string()));
        let request = RegisterRequest::signed(&identity, Vec::new(), meta.clone()).unwrap();

        let denied = client.register(&request).unwrap_err();
        assert_ureq_status(&denied, 403);
        let pending = admin
            .admin_list_devices()
            .unwrap()
            .devices
            .into_iter()
            .find(|device| device.peer_id == peer_id)
            .expect("signed unknown device should be visible for explicit enrollment");
        assert!(!pending.enrolled);
        assert!(!pending.active);

        admin.admin_enroll(peer_id).unwrap();
        assert!(client.register(&request).unwrap().ok);

        let departing = crate::libp2p::identity::Keypair::generate_ed25519();
        let departing_peer_id = departing.public().to_peer_id().to_string();
        let denied = client
            .register(&RegisterRequest::signed(&departing, Vec::new(), meta).unwrap())
            .unwrap_err();
        assert_ureq_status(&denied, 403);
        assert!(
            client
                .unregister(&UnregisterRequest::signed(&departing).unwrap())
                .unwrap()
                .removed
        );
        assert!(
            admin
                .admin_list_devices()
                .unwrap()
                .devices
                .into_iter()
                .all(|device| device.peer_id != departing_peer_id)
        );
        let stale_enroll = admin.admin_enroll(departing_peer_id).unwrap_err();
        assert_ureq_status(&stale_enroll, 409);
        server.shutdown();
    }

    #[test]
    fn tracker_rejects_noncanonical_and_duplicate_device_bindings() {
        let invalid_meta = Some(PeerMeta {
            device_id: Some(" node0 ".to_string()),
            hostname: None,
            user_id: Some("u1".to_string()),
            version: None,
            build_revision: None,
            build_dirty: None,
        });
        assert!(validate_register_meta(&invalid_meta).is_err());

        let dir = tempfile::tempdir().unwrap();
        let server = start_test_server_with_config(TrackerServeConfig {
            ttl_sec: 60,
            token: Some("fleet-token".to_string()),
            admin_token: Some("admin-token".to_string()),
            security_state_path: Some(dir.path().join("tracker-security.json")),
            require_device_enrollment: false,
        });
        let client = TrackerClient::new(server.base_url.clone(), Some("fleet-token".to_string()));
        let admin = client
            .clone()
            .with_admin_token(Some("admin-token".to_string()));
        let meta = Some(PeerMeta {
            device_id: Some("node0".to_string()),
            hostname: None,
            user_id: Some("u1".to_string()),
            version: None,
            build_revision: None,
            build_dirty: None,
        });
        let first = crate::libp2p::identity::Keypair::generate_ed25519();
        client
            .register(&RegisterRequest::signed(&first, Vec::new(), meta.clone()).unwrap())
            .unwrap();
        admin
            .admin_enroll(first.public().to_peer_id().to_string())
            .unwrap();

        let duplicate = crate::libp2p::identity::Keypair::generate_ed25519();
        let duplicate_peer_id = duplicate.public().to_peer_id().to_string();
        client
            .register(&RegisterRequest::signed(&duplicate, Vec::new(), meta).unwrap())
            .unwrap();
        let error = admin.admin_enroll(duplicate_peer_id).unwrap_err();
        assert_ureq_status(&error, 409);
        server.shutdown();
    }

    #[test]
    fn logical_device_revocation_applies_to_preexisting_sibling_identity() {
        let mut state = TrackerState::default();
        state.revocations.insert(
            "retired-peer".to_string(),
            RevocationInfo {
                peer_id: "retired-peer".to_string(),
                device_id: Some("node0".to_string()),
                user_id: Some("u1".to_string()),
                revoked_at_unix: 1,
                ticket_id: uuid::Uuid::new_v4().to_string(),
            },
        );
        state.enrolled_devices.insert(
            "sibling-peer".to_string(),
            EnrolledDevice {
                peer_id: "sibling-peer".to_string(),
                public_key: vec![1],
                device_id: Some("node0".to_string()),
                user_id: Some("u1".to_string()),
                retirement_protocol: Some(RETIREMENT_PROTOCOL_VERSION),
                membership_protocol: Some(DEVICE_MEMBERSHIP_PROTOCOL_VERSION),
                membership_enforced: true,
                enrolled_at_unix: 1,
            },
        );

        let revocation = effective_revocation_for_peer(&state, "sibling-peer").unwrap();
        assert_eq!(revocation.peer_id, "sibling-peer");
        assert_eq!(revocation.device_id.as_deref(), Some("node0"));
    }

    #[cfg(unix)]
    #[test]
    fn tracker_security_state_allows_only_one_writer_process_lifetime() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("tracker-security.json");

        let first = TrackerState::load(Some(state_path.clone())).unwrap();
        let second = TrackerState::load(Some(state_path.clone())).unwrap_err();
        assert!(format!("{second:#}").contains("already locked"));

        drop(first);
        TrackerState::load(Some(state_path)).unwrap();
    }
}
