use anyhow::{Context, Result};
use serde::Deserialize;
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

    pub search_limit_default: Option<usize>,
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
    use rand::RngCore;

    let path = expand_home_path(path)?;
    match std::fs::read_to_string(&path) {
        Ok(s) => {
            let key: PreSharedKey = s.parse().context("parse swarm key")?;
            Ok(key)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            ensure_parent_dir(&path)?;

            let mut raw = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut raw);
            let key = PreSharedKey::new(raw);

            std::fs::write(&path, key.to_string())
                .with_context(|| format!("write swarm key: {}", path.display()))?;
            restrict_permissions(&path)?;
            Ok(key)
        }
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
            std::fs::write(&path, bytes)
                .with_context(|| format!("write identity keypair: {}", path.display()))?;
            restrict_permissions(&path)?;
            Ok(keypair)
        }
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
"#,
        )
        .unwrap();

        let cfg = load_from_path(path.to_str().unwrap()).unwrap();
        assert_eq!(cfg.user_id.as_deref(), Some("user1"));
        assert_eq!(cfg.trackers.as_ref().unwrap().len(), 1);
        assert_eq!(cfg.p2p_watch_start_jitter_sec, Some(5));
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
