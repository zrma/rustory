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
}

pub fn new_entry_id() -> EntryId {
    uuid::Uuid::new_v4().to_string()
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
}
