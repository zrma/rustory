use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

pub type EntryId = String;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub entry_id: EntryId,
    pub device_id: String,
    pub user_id: String,
    pub ts: OffsetDateTime,
    pub cmd: String,
    pub cwd: String,
    pub exit_code: i32,
    pub duration_ms: i64,
    pub shell: String,
    pub hostname: String,
    pub version: String,
}

#[derive(Clone, Debug)]
pub struct EntryInput {
    pub device_id: String,
    pub user_id: String,
    pub ts: OffsetDateTime,
    pub cmd: String,
    pub cwd: String,
    pub exit_code: i32,
    pub duration_ms: i64,
    pub shell: String,
    pub hostname: String,
}

impl Entry {
    pub fn new(input: EntryInput) -> Self {
        Self {
            entry_id: new_entry_id(),
            device_id: input.device_id,
            user_id: input.user_id,
            ts: input.ts,
            cmd: input.cmd,
            cwd: input.cwd,
            exit_code: input.exit_code,
            duration_ms: input.duration_ms,
            shell: input.shell,
            hostname: input.hostname,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    pub fn new_with_id(entry_id: EntryId, input: EntryInput) -> Self {
        Self {
            entry_id,
            device_id: input.device_id,
            user_id: input.user_id,
            ts: input.ts,
            cmd: input.cmd,
            cwd: input.cwd,
            exit_code: input.exit_code,
            duration_ms: input.duration_ms,
            shell: input.shell,
            hostname: input.hostname,
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

pub fn new_entry_id() -> EntryId {
    uuid::Uuid::new_v4().to_string()
}

pub fn import_entry_id(
    user_id: &str,
    device_id: &str,
    shell: &str,
    ts_unix: i64,
    cmd: &str,
    source_index: u64,
) -> EntryId {
    // Deterministic UUIDv5 for idempotent history imports.
    //
    // Note: `source_index` intentionally participates to avoid collisions when multiple commands
    // have the same timestamp and identical command text.
    let name = format!(
        "rustory:import\0{user_id}\0{device_id}\0{shell}\0{ts_unix}\0{source_index}\0{cmd}"
    );
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, name.as_bytes()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn new_entry_id_returns_uuid() {
        let id = new_entry_id();
        let _uuid = Uuid::parse_str(&id).unwrap();
    }

    #[test]
    fn new_entry_id_is_unique_enough_for_poc() {
        let a = new_entry_id();
        let b = new_entry_id();
        assert_ne!(a, b);
    }

    #[test]
    fn entry_new_generates_id_and_sets_version() {
        let e = Entry::new(EntryInput {
            device_id: "dev1".to_string(),
            user_id: "user1".to_string(),
            ts: OffsetDateTime::from_unix_timestamp(1).unwrap(),
            cmd: "echo 1".to_string(),
            cwd: "/tmp".to_string(),
            exit_code: 0,
            duration_ms: 12,
            shell: "zsh".to_string(),
            hostname: "host".to_string(),
        });

        let _uuid = Uuid::parse_str(&e.entry_id).unwrap();
        assert_eq!(e.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(e.cmd, "echo 1");
    }

    #[test]
    fn import_entry_id_is_deterministic_and_sensitive() {
        let a = import_entry_id("u1", "d1", "zsh", 1, "echo 1", 0);
        let b = import_entry_id("u1", "d1", "zsh", 1, "echo 1", 0);
        assert_eq!(a, b);

        let c = import_entry_id("u1", "d1", "zsh", 1, "echo 1", 1);
        assert_ne!(a, c);

        let d = import_entry_id("u1", "d1", "zsh", 1, "echo 2", 0);
        assert_ne!(a, d);
    }
}
