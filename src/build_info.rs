pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const BUILD_REVISION: &str = env!("RUSTORY_BUILD_REVISION");
pub const BUILD_REVISION_SOURCE: &str = env!("RUSTORY_BUILD_REVISION_SOURCE");
pub const VERSION_DISPLAY: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (rev ",
    env!("RUSTORY_BUILD_REVISION"),
    env!("RUSTORY_BUILD_DIRTY_SUFFIX"),
    ")"
);

pub fn build_dirty() -> bool {
    env!("RUSTORY_BUILD_DIRTY") == "1"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_display_includes_revision() {
        assert!(VERSION_DISPLAY.starts_with(VERSION));
        assert!(VERSION_DISPLAY.contains(BUILD_REVISION));
        assert!(VERSION_DISPLAY.contains("rev "));
    }
}
