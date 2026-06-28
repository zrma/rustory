use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=RUSTORY_BUILD_REVISION");
    println!("cargo:rerun-if-env-changed=RUSTORY_BUILD_REVISION_SOURCE");
    println!("cargo:rerun-if-env-changed=RUSTORY_BUILD_DIRTY");
    println!("cargo:rerun-if-changed=.jj/working_copy/checkout");
    println!("cargo:rerun-if-changed=.jj/working_copy/tree_state");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");

    let revision_override = std::env::var("RUSTORY_BUILD_REVISION")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let source_override = std::env::var("RUSTORY_BUILD_REVISION_SOURCE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let dirty_override = std::env::var("RUSTORY_BUILD_DIRTY")
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"));

    let (source, revision, dirty) = match revision_override {
        Some(revision) => (
            source_override.unwrap_or_else(|| "env".to_string()),
            revision,
            dirty_override.unwrap_or(false),
        ),
        None => detect_revision().unwrap_or_else(|| {
            (
                source_override.unwrap_or_else(|| "unknown".to_string()),
                "unknown".to_string(),
                dirty_override.unwrap_or(false),
            )
        }),
    };

    println!(
        "cargo:rustc-env=RUSTORY_BUILD_REVISION={}",
        sanitize(&revision)
    );
    println!(
        "cargo:rustc-env=RUSTORY_BUILD_REVISION_SOURCE={}",
        sanitize(&source)
    );
    println!(
        "cargo:rustc-env=RUSTORY_BUILD_DIRTY={}",
        if dirty { "1" } else { "0" }
    );
    println!(
        "cargo:rustc-env=RUSTORY_BUILD_DIRTY_SUFFIX={}",
        if dirty { "-dirty" } else { "" }
    );
}

fn detect_revision() -> Option<(String, String, bool)> {
    if let Some(revision) = command_stdout(
        "jj",
        &["log", "-r", "@", "--no-graph", "-T", "commit_id.short(12)"],
    ) {
        return Some(("jj".to_string(), revision, false));
    }

    let revision = command_stdout("git", &["rev-parse", "--short=12", "HEAD"])?;
    let dirty = Command::new("git")
        .args(["diff-index", "--quiet", "HEAD", "--"])
        .status()
        .map(|status| !status.success())
        .unwrap_or(false);

    Some(("git".to_string(), revision, dirty))
}

fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8(output.stdout).ok()?;
    let value = text.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\n' | '\r' | '\0' => '_',
            _ => ch,
        })
        .collect()
}
