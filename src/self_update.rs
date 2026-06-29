use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::Duration;

pub const DEFAULT_RELEASE_REPO: &str = "zrma/rustory";

const MAX_ASSET_BYTES: u64 = 128 * 1024 * 1024;
const MAX_CHECKSUM_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRequest {
    pub version: String,
    pub repo: String,
    pub asset_base_url: Option<String>,
    pub asset_url: Option<String>,
    pub checksum_url: Option<String>,
    pub sha256: Option<String>,
    pub install_path: Option<PathBuf>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdatePlan {
    version: String,
    repo: String,
    target: String,
    asset_name: String,
    asset_url: String,
    checksum_url: Option<String>,
    install_path: PathBuf,
}

pub fn run_update(request: UpdateRequest) -> Result<()> {
    let plan = build_update_plan(&request)?;

    println!(
        "update plan: current_version={} current_revision={} target={} version={} install_path={}",
        crate::build_info::VERSION,
        crate::build_info::BUILD_REVISION,
        plan.target,
        plan.version,
        plan.install_path.display()
    );
    println!("asset_url: {}", plan.asset_url);
    match (&request.sha256, &plan.checksum_url) {
        (Some(_), _) => println!("checksum: provided via --sha256"),
        (None, Some(url)) => println!("checksum_url: {url}"),
        (None, None) => println!("checksum: unavailable"),
    }

    if request.dry_run {
        println!("dry-run: no download or replacement performed");
        return Ok(());
    }

    let bytes = download_bytes(&plan.asset_url, MAX_ASSET_BYTES)
        .with_context(|| format!("download release asset: {}", plan.asset_url))?;
    let expected = resolve_expected_sha256(&request, &plan)?;
    verify_sha256(&bytes, &expected)?;
    install_binary(&bytes, &plan.install_path)?;

    println!("updated rr: {}", plan.install_path.display());
    Ok(())
}

fn build_update_plan(request: &UpdateRequest) -> Result<UpdatePlan> {
    if request.asset_base_url.is_some() && request.asset_url.is_some() {
        anyhow::bail!("pass only one of --asset-base-url or --asset-url");
    }

    let version = normalize_version(&request.version)?;
    let repo = normalize_repo(&request.repo)?;
    let target = current_release_target()?.to_string();
    let asset_name = release_asset_name(&target);
    let asset_url = match request.asset_url.as_deref().and_then(normalize_nonempty) {
        Some(url) => url,
        None => match request
            .asset_base_url
            .as_deref()
            .and_then(normalize_nonempty)
        {
            Some(base_url) => format!("{}/{}", base_url.trim_end_matches('/'), asset_name),
            None => github_release_asset_url(&repo, &version, &asset_name),
        },
    };
    let asset_name = asset_name_from_url(&asset_url).unwrap_or(asset_name);
    let checksum_url = request
        .checksum_url
        .as_deref()
        .and_then(normalize_nonempty)
        .or_else(|| {
            request
                .sha256
                .is_none()
                .then(|| format!("{asset_url}.sha256"))
        });
    let install_path = match request.install_path.clone() {
        Some(path) => path,
        None => std::env::current_exe().context("resolve current rr executable path")?,
    };

    Ok(UpdatePlan {
        version,
        repo,
        target,
        asset_name,
        asset_url,
        checksum_url,
        install_path,
    })
}

fn normalize_version(raw: &str) -> Result<String> {
    normalize_nonempty(raw).context("--version must not be empty")
}

fn normalize_repo(raw: &str) -> Result<String> {
    let repo = normalize_nonempty(raw).context("--repo must not be empty")?;
    if repo.chars().any(char::is_whitespace) || !repo.contains('/') || repo.contains("..") {
        anyhow::bail!("--repo must be a GitHub owner/name value");
    }
    Ok(repo)
}

fn normalize_nonempty(raw: &str) -> Option<String> {
    let value = raw.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn github_release_asset_url(repo: &str, version: &str, asset_name: &str) -> String {
    if version == "latest" {
        format!("https://github.com/{repo}/releases/latest/download/{asset_name}")
    } else {
        format!("https://github.com/{repo}/releases/download/{version}/{asset_name}")
    }
}

fn release_asset_name(target: &str) -> String {
    format!("rr-{target}")
}

fn asset_name_from_url(url: &str) -> Option<String> {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .and_then(normalize_nonempty)
}

fn current_release_target() -> Result<&'static str> {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        Ok("aarch64-apple-darwin")
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        Ok("x86_64-apple-darwin")
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Ok("x86_64-unknown-linux-gnu")
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        Ok("aarch64-unknown-linux-gnu")
    }
    #[cfg(not(any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64")
    )))]
    {
        anyhow::bail!(
            "unsupported update target: {}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    }
}

fn download_bytes(url: &str, limit: u64) -> Result<Vec<u8>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .timeout_connect(Some(Duration::from_secs(10)))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("User-Agent", concat!("rustory/", env!("CARGO_PKG_VERSION")))
        .call()?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()?;
    Ok(bytes)
}

fn resolve_expected_sha256(request: &UpdateRequest, plan: &UpdatePlan) -> Result<String> {
    if let Some(raw) = request.sha256.as_deref() {
        return normalize_sha256_hex(raw);
    }

    let checksum_url = plan
        .checksum_url
        .as_deref()
        .context("checksum URL unavailable; pass --sha256 explicitly")?;
    let bytes = download_bytes(checksum_url, MAX_CHECKSUM_BYTES)
        .with_context(|| format!("download checksum: {checksum_url}"))?;
    let text = String::from_utf8(bytes).context("checksum response is not utf-8")?;
    parse_sha256_checksum(&text, &plan.asset_name)
}

fn parse_sha256_checksum(text: &str, asset_name: &str) -> Result<String> {
    let mut first_valid = None;
    let mut saw_named_checksum = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(hash) = parts.next() else {
            continue;
        };
        if normalize_sha256_hex(hash).is_err() {
            continue;
        }
        let hash = normalize_sha256_hex(hash)?;
        let names = parts.collect::<Vec<_>>();
        if names.is_empty() {
            first_valid.get_or_insert(hash);
            continue;
        }
        saw_named_checksum = true;
        if names.iter().any(|name| {
            name.trim_start_matches('*')
                .trim_start_matches("./")
                .ends_with(asset_name)
        }) {
            return Ok(hash);
        }
    }

    if saw_named_checksum {
        anyhow::bail!("no SHA-256 checksum found for {asset_name}");
    }
    first_valid.with_context(|| format!("no SHA-256 checksum found for {asset_name}"))
}

fn normalize_sha256_hex(raw: &str) -> Result<String> {
    let value = raw.trim().to_ascii_lowercase();
    if value.len() != 64 || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        anyhow::bail!("SHA-256 must be exactly 64 hex characters");
    }
    Ok(value)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

fn verify_sha256(bytes: &[u8], expected: &str) -> Result<()> {
    let expected = normalize_sha256_hex(expected)?;
    let actual = sha256_hex(bytes);
    if actual != expected {
        anyhow::bail!("SHA-256 mismatch: expected {expected}, actual {actual}");
    }
    println!("checksum: ok sha256={actual}");
    Ok(())
}

fn install_binary(bytes: &[u8], install_path: &Path) -> Result<()> {
    let parent = install_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .with_context(|| format!("install path has no parent: {}", install_path.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create install dir: {}", parent.display()))?;

    let file_name = install_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("rr");
    let tmp_path = parent.join(format!(".{file_name}.download-{}", std::process::id()));
    {
        let mut file = std::fs::File::create(&tmp_path)
            .with_context(|| format!("create temporary binary: {}", tmp_path.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("write temporary binary: {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("sync temporary binary: {}", tmp_path.display()))?;
    }
    make_executable(&tmp_path)?;
    let result = verify_downloaded_binary(&tmp_path).and_then(|()| {
        std::fs::rename(&tmp_path, install_path).with_context(|| {
            format!(
                "replace {} with downloaded binary {}",
                install_path.display(),
                tmp_path.display()
            )
        })
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    result.with_context(|| format!("install downloaded binary to {}", install_path.display()))?;
    Ok(())
}

fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(path, perms)
            .with_context(|| format!("chmod 0755: {}", path.display()))?;
    }
    Ok(())
}

fn verify_downloaded_binary(path: &Path) -> Result<()> {
    let output = ProcessCommand::new(path)
        .arg("version")
        .output()
        .with_context(|| format!("run downloaded binary: {}", path.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("downloaded binary failed `version`: {stderr}");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout
        .lines()
        .next()
        .unwrap_or("version output unavailable");
    println!("downloaded binary check: {first_line}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_plan_defaults_to_github_release_asset() {
        let request = UpdateRequest {
            version: "v1.0.2".to_string(),
            repo: "zrma/rustory".to_string(),
            asset_base_url: None,
            asset_url: None,
            checksum_url: None,
            sha256: None,
            install_path: Some(PathBuf::from("/tmp/rr")),
            dry_run: true,
        };

        let plan = build_update_plan(&request).unwrap();
        assert_eq!(plan.version, "v1.0.2");
        assert_eq!(plan.asset_name, release_asset_name(&plan.target));
        assert!(plan.asset_url.contains("/releases/download/v1.0.2/rr-"));
        let expected_checksum_url = format!("{}.sha256", plan.asset_url);
        assert_eq!(
            plan.checksum_url.as_deref(),
            Some(expected_checksum_url.as_str())
        );
        assert_eq!(plan.install_path, PathBuf::from("/tmp/rr"));
    }

    #[test]
    fn update_plan_supports_latest_and_asset_base_url() {
        let request = UpdateRequest {
            version: " latest ".to_string(),
            repo: "zrma/rustory".to_string(),
            asset_base_url: Some("https://example.test/releases".to_string()),
            asset_url: None,
            checksum_url: None,
            sha256: Some("0".repeat(64)),
            install_path: Some(PathBuf::from("/tmp/rr")),
            dry_run: true,
        };

        let plan = build_update_plan(&request).unwrap();
        assert_eq!(plan.version, "latest");
        assert!(
            plan.asset_url
                .starts_with("https://example.test/releases/rr-")
        );
        assert!(plan.checksum_url.is_none());
    }

    #[test]
    fn update_plan_rejects_competing_asset_sources() {
        let request = UpdateRequest {
            version: "latest".to_string(),
            repo: "zrma/rustory".to_string(),
            asset_base_url: Some("https://example.test/releases".to_string()),
            asset_url: Some("https://example.test/rr".to_string()),
            checksum_url: None,
            sha256: None,
            install_path: Some(PathBuf::from("/tmp/rr")),
            dry_run: true,
        };

        assert!(build_update_plan(&request).is_err());
    }

    #[test]
    fn checksum_parser_accepts_raw_and_file_format() {
        let raw = "a".repeat(64);
        assert_eq!(parse_sha256_checksum(&raw, "rr-test").unwrap(), raw);

        let text = format!("{}  rr-test\n{}  other\n", "b".repeat(64), "c".repeat(64));
        assert_eq!(
            parse_sha256_checksum(&text, "rr-test").unwrap(),
            "b".repeat(64)
        );
    }

    #[test]
    fn checksum_parser_rejects_missing_named_asset() {
        let text = format!("{}  other\n", "b".repeat(64));
        assert!(parse_sha256_checksum(&text, "rr-test").is_err());
    }

    #[test]
    fn verify_sha256_detects_mismatch() {
        let expected = sha256_hex(b"hello");
        assert!(verify_sha256(b"hello", &expected).is_ok());
        assert!(verify_sha256(b"hello", &"0".repeat(64)).is_err());
    }
}
