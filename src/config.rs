use crate::libp2p;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const DEFAULT_CONFIG_PATH: &str = "~/.config/rustory/config.toml";
pub const DEFAULT_SWARM_KEY_PATH: &str = "~/.config/rustory/swarm.key";
pub const DEFAULT_P2P_IDENTITY_KEY_PATH: &str = "~/.config/rustory/identity.key";
pub const DEFAULT_RELAY_IDENTITY_KEY_PATH: &str = "~/.config/rustory/relay.key";

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FileConfig {
    pub db_path: Option<String>,
    pub user_id: Option<String>,
    pub device_id: Option<String>,

    pub trackers: Option<Vec<String>>,
    pub tracker_token: Option<String>,

    pub relay_addr: Option<String>,
    pub swarm_key_path: Option<String>,

    pub p2p_identity_key_path: Option<String>,
    pub relay_identity_key_path: Option<String>,

    pub p2p_watch_start_jitter_sec: Option<u64>,
    pub p2p_request_attempts: Option<u64>,
    pub p2p_request_timeout_base_sec: Option<u64>,
    pub p2p_request_timeout_cap_sec: Option<u64>,
    pub p2p_request_backoff_base_ms: Option<u64>,

    pub search_limit_default: Option<usize>,

    pub record_ignore_regex: Option<String>,

    pub async_upload: Option<bool>,
    pub async_upload_interval_sec: Option<u64>,
    pub async_upload_limit: Option<usize>,
    pub async_upload_marker_path: Option<String>,

    pub auto_prune: Option<bool>,
    pub auto_prune_days: Option<u64>,
    pub auto_prune_interval_sec: Option<u64>,
    pub auto_prune_keep_recent: Option<usize>,
    pub auto_prune_marker_path: Option<String>,

    pub auto_tombstone_gc: Option<bool>,
    pub auto_tombstone_gc_days: Option<u64>,
    pub auto_tombstone_gc_interval_sec: Option<u64>,
    pub auto_tombstone_gc_marker_path: Option<String>,
}

pub fn load_default() -> Result<FileConfig> {
    load_from_path(DEFAULT_CONFIG_PATH)
}

pub fn load_from_path(path: &str) -> Result<FileConfig> {
    let path = expand_home_path(path)?;
    let content = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(FileConfig::default()),
        Err(err) => return Err(err).with_context(|| format!("read config: {}", path.display())),
    };

    // 빈 파일은 "설정 없음"으로 취급한다.
    if content.trim().is_empty() {
        return Ok(FileConfig::default());
    }

    toml::from_str(&content).context("parse config toml")
}

pub fn load_or_generate_swarm_key(path: &str) -> Result<libp2p::pnet::PreSharedKey> {
    use libp2p::pnet::PreSharedKey;
    use rand::TryRng;

    let path = expand_home_path(path)?;
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let key: PreSharedKey = s.parse().context("parse swarm key")?;
            Ok(key)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            ensure_parent_dir(&path)?;

            let mut raw = [0u8; 32];
            rand::rngs::SysRng
                .try_fill_bytes(&mut raw)
                .context("generate swarm key")?;
            let key = PreSharedKey::new(raw);
            if install_private_file(&path, key.to_string().as_bytes(), false)? {
                return Ok(key);
            }

            let existing = std::fs::read_to_string(&path).with_context(|| {
                format!("read concurrently created swarm key: {}", path.display())
            })?;
            existing
                .parse()
                .context("parse concurrently created swarm key")
        }
        Err(err) => Err(err).with_context(|| format!("read swarm key: {}", path.display())),
    }
}

pub fn load_swarm_key(path: &str) -> Result<Option<libp2p::pnet::PreSharedKey>> {
    use libp2p::pnet::PreSharedKey;

    let path = expand_home_path(path)?;
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            if s.trim().is_empty() {
                anyhow::bail!("swarm key file is empty: {}", path.display());
            }
            let key: PreSharedKey = s.parse().context("parse swarm key")?;
            Ok(Some(key))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("read swarm key: {}", path.display())),
    }
}

pub fn load_or_generate_identity_keypair(path: &str) -> Result<libp2p::identity::Keypair> {
    let path = expand_home_path(path)?;
    match std::fs::read(&path) {
        Ok(bytes) => {
            if bytes.is_empty() {
                anyhow::bail!("identity keypair file is empty: {}", path.display());
            }

            libp2p::identity::Keypair::from_protobuf_encoding(&bytes)
                .context("parse identity keypair")
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            ensure_parent_dir(&path)?;
            let keypair = libp2p::identity::Keypair::generate_ed25519();
            let bytes = keypair
                .to_protobuf_encoding()
                .context("encode identity keypair")?;
            if install_private_file(&path, &bytes, false)? {
                return Ok(keypair);
            }

            let existing = std::fs::read(&path).with_context(|| {
                format!(
                    "read concurrently created identity keypair: {}",
                    path.display()
                )
            })?;
            libp2p::identity::Keypair::from_protobuf_encoding(&existing)
                .context("parse concurrently created identity keypair")
        }
        Err(err) => Err(err).with_context(|| format!("read identity keypair: {}", path.display())),
    }
}

pub fn load_identity_keypair(path: &str) -> Result<Option<libp2p::identity::Keypair>> {
    let path = expand_home_path(path)?;
    match std::fs::read(&path) {
        Ok(bytes) => {
            if bytes.is_empty() {
                anyhow::bail!("identity keypair file is empty: {}", path.display());
            }

            let keypair = libp2p::identity::Keypair::from_protobuf_encoding(&bytes)
                .context("parse identity keypair")?;
            Ok(Some(keypair))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("read identity keypair: {}", path.display())),
    }
}

pub fn expand_home_path(path: &str) -> Result<PathBuf> {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var_os("HOME").context("HOME env var not set")?;
        return Ok(Path::new(&home).join(rest));
    }
    Ok(PathBuf::from(path))
}

pub fn write_private_file(path: &Path, contents: &[u8], replace_existing: bool) -> Result<()> {
    if install_private_file(path, contents, replace_existing)? {
        return Ok(());
    }
    anyhow::bail!("private file already exists: {}", path.display())
}

fn install_private_file(path: &Path, contents: &[u8], replace_existing: bool) -> Result<bool> {
    ensure_parent_dir(path)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("rustory-private");
    let tmp_path = parent.join(format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));

    let write_result = (|| -> Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&tmp_path)
            .with_context(|| format!("create private temp file: {}", tmp_path.display()))?;
        file.write_all(contents)
            .with_context(|| format!("write private temp file: {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("sync private temp file: {}", tmp_path.display()))?;
        restrict_permissions(&tmp_path)?;
        Ok(())
    })();
    if let Err(err) = write_result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }

    let install_result = if replace_existing {
        std::fs::rename(&tmp_path, path)
            .map(|()| true)
            .with_context(|| {
                format!(
                    "atomically replace private file: {} -> {}",
                    tmp_path.display(),
                    path.display()
                )
            })
    } else {
        match std::fs::hard_link(&tmp_path, path) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(err) => Err(err).with_context(|| {
                format!(
                    "atomically install private file: {} -> {}",
                    tmp_path.display(),
                    path.display()
                )
            }),
        }
    };
    let _ = std::fs::remove_file(&tmp_path);
    install_result
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(parent).with_context(|| format!("create dir: {}", parent.display()))?;
    Ok(())
}

fn restrict_permissions(path: &Path) -> Result<()> {
    // 보안상 가능한 OS에서만 최소 권한으로 제한한다.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(path, perms)
            .with_context(|| format!("chmod 0600: {}", path.display()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_from_path_returns_default_when_missing() {
        let dir = tempdir().unwrap();
        let cfg = load_from_path(dir.path().join("missing.toml").to_str().unwrap()).unwrap();
        assert!(cfg.db_path.is_none());
    }

    #[test]
    fn load_from_path_parses_toml() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
db_path = "~/.rustory/history.db"
user_id = "user1"
device_id = "dev1"
trackers = ["http://127.0.0.1:8850"]
p2p_watch_start_jitter_sec = 5
p2p_request_attempts = 4
p2p_request_timeout_base_sec = 6
p2p_request_timeout_cap_sec = 40
p2p_request_backoff_base_ms = 250
record_ignore_regex = "(?i)token|password"
async_upload = true
async_upload_interval_sec = 30
async_upload_limit = 500
async_upload_marker_path = "~/.config/rustory/async-upload.custom.last"
auto_prune = true
auto_prune_days = 90
auto_prune_interval_sec = 3600
auto_prune_keep_recent = 1000
auto_prune_marker_path = "~/.config/rustory/auto-prune.custom.last"
auto_tombstone_gc = true
auto_tombstone_gc_days = 45
auto_tombstone_gc_interval_sec = 7200
auto_tombstone_gc_marker_path = "~/.config/rustory/auto-tombstone-gc.custom.last"
"#,
        )
        .unwrap();

        let cfg = load_from_path(path.to_str().unwrap()).unwrap();
        assert_eq!(cfg.user_id.as_deref(), Some("user1"));
        assert_eq!(cfg.trackers.as_ref().unwrap().len(), 1);
        assert_eq!(cfg.p2p_watch_start_jitter_sec, Some(5));
        assert_eq!(cfg.p2p_request_attempts, Some(4));
        assert_eq!(cfg.p2p_request_timeout_base_sec, Some(6));
        assert_eq!(cfg.p2p_request_timeout_cap_sec, Some(40));
        assert_eq!(cfg.p2p_request_backoff_base_ms, Some(250));
        assert_eq!(
            cfg.record_ignore_regex.as_deref(),
            Some("(?i)token|password")
        );
        assert_eq!(cfg.async_upload, Some(true));
        assert_eq!(cfg.async_upload_interval_sec, Some(30));
        assert_eq!(cfg.async_upload_limit, Some(500));
        assert_eq!(
            cfg.async_upload_marker_path.as_deref(),
            Some("~/.config/rustory/async-upload.custom.last")
        );
        assert_eq!(cfg.auto_prune, Some(true));
        assert_eq!(cfg.auto_prune_days, Some(90));
        assert_eq!(cfg.auto_prune_interval_sec, Some(3600));
        assert_eq!(cfg.auto_prune_keep_recent, Some(1000));
        assert_eq!(
            cfg.auto_prune_marker_path.as_deref(),
            Some("~/.config/rustory/auto-prune.custom.last")
        );
        assert_eq!(cfg.auto_tombstone_gc, Some(true));
        assert_eq!(cfg.auto_tombstone_gc_days, Some(45));
        assert_eq!(cfg.auto_tombstone_gc_interval_sec, Some(7200));
        assert_eq!(
            cfg.auto_tombstone_gc_marker_path.as_deref(),
            Some("~/.config/rustory/auto-tombstone-gc.custom.last")
        );
    }

    #[test]
    fn private_file_write_is_noclobber_by_default_and_supports_atomic_replace() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");

        write_private_file(&path, b"secret=one\n", false).unwrap();
        let err = write_private_file(&path, b"secret=two\n", false).unwrap_err();
        assert!(format!("{err:#}").contains("already exists"));
        assert_eq!(std::fs::read(&path).unwrap(), b"secret=one\n");

        write_private_file(&path, b"secret=two\n", true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"secret=two\n");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn private_file_replace_does_not_follow_existing_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let target = dir.path().join("target");
        let path = dir.path().join("config.toml");
        std::fs::write(&target, b"keep-target\n").unwrap();
        symlink(&target, &path).unwrap();

        write_private_file(&path, b"new-config\n", true).unwrap();

        assert!(
            !std::fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"new-config\n");
        assert_eq!(std::fs::read(&target).unwrap(), b"keep-target\n");
    }

    #[test]
    fn load_or_generate_swarm_key_creates_and_is_parseable() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("swarm.key");

        let k1 = load_or_generate_swarm_key(path.to_str().unwrap()).unwrap();
        let k2 = load_or_generate_swarm_key(path.to_str().unwrap()).unwrap();
        assert_eq!(k1, k2);
    }

    #[test]
    fn load_or_generate_identity_keypair_creates_and_is_stable() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("identity.key");

        let k1 = load_or_generate_identity_keypair(path.to_str().unwrap()).unwrap();
        let k2 = load_or_generate_identity_keypair(path.to_str().unwrap()).unwrap();

        assert_eq!(k1.public().to_peer_id(), k2.public().to_peer_id());
    }
}
