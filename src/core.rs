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
}
