use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const HISHTORY_DIRS_AND_FILES: &[&str] = &[".hishtory", ".config/hishtory", ".local/bin/hishtory"];

const STARTUP_FILES: &[&str] = &[
    ".zshrc",
    ".zprofile",
    ".zshenv",
    ".zlogin",
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".profile",
];

#[derive(Debug, Clone)]
pub struct CleanupOptions {
    pub home_dir: PathBuf,
    pub apply: bool,
    pub archive_dir: Option<PathBuf>,
    pub no_archive: bool,
    pub backup_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupReport {
    pub applied: bool,
    pub home_dir: PathBuf,
    pub archive_root: Option<PathBuf>,
    pub actions: Vec<CleanupAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupAction {
    ArchivePath { source: PathBuf, dest: PathBuf },
    RemovePath { path: PathBuf },
    RewriteStartupFile { path: PathBuf, removed_lines: usize },
    RemoveEmptyStartupFile { path: PathBuf, removed_lines: usize },
}

#[derive(Debug, Clone)]
enum PlannedAction {
    RemovePath {
        path: PathBuf,
    },
    RewriteStartupFile {
        path: PathBuf,
        removed_lines: usize,
        new_content: String,
    },
    RemoveEmptyStartupFile {
        path: PathBuf,
        removed_lines: usize,
    },
}

impl PlannedAction {
    fn affected_path(&self) -> &Path {
        match self {
            Self::RemovePath { path }
            | Self::RewriteStartupFile { path, .. }
            | Self::RemoveEmptyStartupFile { path, .. } => path,
        }
    }

    fn report_action(&self) -> CleanupAction {
        match self {
            Self::RemovePath { path } => CleanupAction::RemovePath { path: path.clone() },
            Self::RewriteStartupFile {
                path,
                removed_lines,
                ..
            } => CleanupAction::RewriteStartupFile {
                path: path.clone(),
                removed_lines: *removed_lines,
            },
            Self::RemoveEmptyStartupFile {
                path,
                removed_lines,
            } => CleanupAction::RemoveEmptyStartupFile {
                path: path.clone(),
                removed_lines: *removed_lines,
            },
        }
    }
}

pub fn cleanup_hishtory(opts: CleanupOptions) -> Result<CleanupReport> {
    if opts.apply && opts.archive_dir.is_none() && !opts.no_archive {
        anyhow::bail!("cleanup-hishtory --apply requires --archive-dir or --no-archive");
    }
    if opts.archive_dir.is_some() && opts.no_archive {
        anyhow::bail!("cleanup-hishtory cannot combine --archive-dir and --no-archive");
    }

    let home_dir = normalize_home_dir(opts.home_dir)?;
    let planned = plan_cleanup(&home_dir)?;
    let mut actions = Vec::new();
    let archive_root = if opts.apply {
        match opts.archive_dir.as_deref() {
            Some(archive_dir) if !planned.is_empty() => {
                let root = archive_dir.join(opts.backup_name.unwrap_or_else(|| {
                    format!("hishtory-backup-{}-{}", unix_now(), uuid::Uuid::new_v4())
                }));
                validate_archive_root(&planned, &root)?;
                create_private_archive_root(archive_dir, &root)?;
                archive_planned_paths(&planned, &home_dir, &root, &mut actions)?;
                Some(root)
            }
            _ => None,
        }
    } else {
        None
    };

    if opts.apply {
        for action in &planned {
            apply_planned_action(action)?;
            actions.push(action.report_action());
        }
    } else {
        actions.extend(planned.iter().map(PlannedAction::report_action));
    }

    Ok(CleanupReport {
        applied: opts.apply,
        home_dir,
        archive_root,
        actions,
    })
}

pub fn print_report(report: &CleanupReport, mut out: impl Write) -> io::Result<()> {
    let mode = if report.applied { "apply" } else { "dry-run" };
    writeln!(
        out,
        "cleanup-hishtory: mode={} home={}",
        mode,
        report.home_dir.display()
    )?;
    if let Some(root) = &report.archive_root {
        writeln!(out, "archive: path={}", root.display())?;
    }
    if report.actions.is_empty() {
        writeln!(
            out,
            "cleanup-hishtory: no Hishtory files or startup hooks found"
        )?;
        return Ok(());
    }

    for action in &report.actions {
        match action {
            CleanupAction::ArchivePath { source, dest } => writeln!(
                out,
                "{} archive source={} dest={}",
                mode,
                source.display(),
                dest.display()
            )?,
            CleanupAction::RemovePath { path } => {
                writeln!(out, "{} remove path={}", mode, path.display())?
            }
            CleanupAction::RewriteStartupFile {
                path,
                removed_lines,
            } => writeln!(
                out,
                "{} rewrite-startup-file path={} removed_lines={}",
                mode,
                path.display(),
                removed_lines
            )?,
            CleanupAction::RemoveEmptyStartupFile {
                path,
                removed_lines,
            } => writeln!(
                out,
                "{} remove-empty-startup-file path={} removed_lines={}",
                mode,
                path.display(),
                removed_lines
            )?,
        }
    }

    Ok(())
}

fn normalize_home_dir(path: PathBuf) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        anyhow::bail!("home directory must not be empty");
    }
    Ok(path)
}

fn plan_cleanup(home_dir: &Path) -> Result<Vec<PlannedAction>> {
    let mut actions = Vec::new();

    for rel in HISHTORY_DIRS_AND_FILES {
        let path = home_dir.join(rel);
        if path_exists_no_follow(&path)? {
            actions.push(PlannedAction::RemovePath { path });
        }
    }

    for rel in STARTUP_FILES {
        let path = home_dir.join(rel);
        if !path_exists_no_follow(&path)? {
            continue;
        }
        if let Some(action) = plan_startup_file_cleanup(path)? {
            actions.push(action);
        }
    }

    Ok(actions)
}

fn plan_startup_file_cleanup(path: PathBuf) -> Result<Option<PlannedAction>> {
    let metadata = fs::symlink_metadata(&path)
        .with_context(|| format!("inspect startup file: {}", path.display()))?;
    let content = fs::read_to_string(&path)
        .with_context(|| format!("read startup file: {}", path.display()))?;
    let had_trailing_newline = content.ends_with('\n');
    let mut kept = Vec::new();
    let mut removed_lines = 0usize;

    for line in content.lines() {
        if is_hishtory_startup_line(line) {
            removed_lines += 1;
        } else {
            kept.push(line);
        }
    }

    if removed_lines == 0 {
        return Ok(None);
    }

    if metadata.file_type().is_symlink() {
        anyhow::bail!(
            "refusing to rewrite symlinked startup file {}; edit its target manually",
            path.display()
        );
    }

    if kept.iter().all(|line| line.trim().is_empty()) {
        return Ok(Some(PlannedAction::RemoveEmptyStartupFile {
            path,
            removed_lines,
        }));
    }

    let mut new_content = kept.join("\n");
    if had_trailing_newline {
        new_content.push('\n');
    }

    Ok(Some(PlannedAction::RewriteStartupFile {
        path,
        removed_lines,
        new_content,
    }))
}

fn is_hishtory_startup_line(line: &str) -> bool {
    let stripped = line.trim();
    let folded = stripped.to_ascii_lowercase();
    if folded.trim_end_matches(':') == "# hishtory config" {
        return true;
    }
    if folded.is_empty() || folded.starts_with('#') {
        return false;
    }

    let references_hishtory_path = folded.contains(".hishtory")
        || folded.contains("hishtory/config")
        || folded.contains("hishtory config");
    let is_source = folded.starts_with("source ") || folded.starts_with(". ");
    let is_path_assignment = folded.starts_with("export path=") || folded.starts_with("path=");
    let is_eval_hook = folded.starts_with("eval ")
        && (folded.contains("$(hishtory ") || folded.contains("`hishtory "));
    let mut words = folded.split_whitespace();
    let is_direct_hook = words.next() == Some("hishtory")
        && matches!(words.next(), Some("init" | "enable" | "shell" | "daemon"));

    (references_hishtory_path && (is_source || is_path_assignment))
        || is_eval_hook
        || is_direct_hook
}

fn validate_archive_root(planned: &[PlannedAction], archive_root: &Path) -> Result<()> {
    let archive_root = normalize_boundary_path(archive_root)?;
    for source in planned.iter().map(PlannedAction::affected_path) {
        let source = normalize_boundary_path(source)?;
        if archive_root == source
            || archive_root.starts_with(&source)
            || source.starts_with(&archive_root)
        {
            anyhow::bail!(
                "archive path overlaps cleanup source: archive={} source={}",
                archive_root.display(),
                source.display()
            );
        }
    }
    Ok(())
}

fn normalize_boundary_path(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("resolve current directory for cleanup path")?
            .join(path)
    };
    let lexical = normalize_lexical_path(&absolute)?;
    let mut cursor = lexical.clone();
    let mut missing: Vec<OsString> = Vec::new();
    loop {
        match fs::canonicalize(&cursor) {
            Ok(mut resolved) => {
                for part in missing.iter().rev() {
                    resolved.push(part);
                }
                return Ok(resolved);
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                let name = cursor.file_name().with_context(|| {
                    format!("cleanup path has no existing ancestor: {}", path.display())
                })?;
                missing.push(name.to_os_string());
                if !cursor.pop() {
                    anyhow::bail!("cleanup path has no existing ancestor: {}", path.display());
                }
            }
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("canonicalize cleanup path: {}", path.display()));
            }
        }
    }
}

fn normalize_lexical_path(path: &Path) -> Result<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    anyhow::bail!("cleanup path escapes filesystem root: {}", path.display());
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    Ok(normalized)
}

fn archive_planned_paths(
    planned: &[PlannedAction],
    home_dir: &Path,
    archive_root: &Path,
    actions: &mut Vec<CleanupAction>,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    for source in planned.iter().map(PlannedAction::affected_path) {
        if !seen.insert(source.to_path_buf()) {
            continue;
        }
        let rel = source
            .strip_prefix(home_dir)
            .with_context(|| format!("archive path outside home: {}", source.display()))?;
        let dest = archive_root.join(rel);
        copy_path_no_follow(source, &dest)?;
        actions.push(CleanupAction::ArchivePath {
            source: source.to_path_buf(),
            dest,
        });
    }
    Ok(())
}

fn apply_planned_action(action: &PlannedAction) -> Result<()> {
    match action {
        PlannedAction::RemovePath { path } | PlannedAction::RemoveEmptyStartupFile { path, .. } => {
            remove_path_no_follow(path)
        }
        PlannedAction::RewriteStartupFile {
            path, new_content, ..
        } => fs::write(path, new_content)
            .with_context(|| format!("rewrite startup file: {}", path.display())),
    }
}

fn path_exists_no_follow(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| format!("inspect path: {}", path.display())),
    }
}

fn copy_path_no_follow(source: &Path, dest: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(source)
        .with_context(|| format!("inspect archive source: {}", source.display()))?;
    if meta.file_type().is_symlink() {
        let target =
            fs::read_link(source).with_context(|| format!("read symlink: {}", source.display()))?;
        let symlink_note = dest.with_extension("symlink-target");
        ensure_parent_dir(&symlink_note)?;
        write_new_archive_file(&symlink_note, target.to_string_lossy().as_bytes())
            .with_context(|| format!("archive symlink target: {}", symlink_note.display()))?;
        return Ok(());
    }
    if meta.is_dir() {
        create_private_dir(dest)
            .with_context(|| format!("create archive dir: {}", dest.display()))?;
        for entry in fs::read_dir(source)
            .with_context(|| format!("read archive source dir: {}", source.display()))?
        {
            let entry = entry.with_context(|| format!("read entry under: {}", source.display()))?;
            copy_path_no_follow(&entry.path(), &dest.join(entry.file_name()))?;
        }
        return Ok(());
    }

    ensure_parent_dir(dest)?;
    let mut source_file = fs::File::open(source)
        .with_context(|| format!("open archive source: {}", source.display()))?;
    let mut dest_file = open_new_archive_file(dest)
        .with_context(|| format!("create archive destination: {}", dest.display()))?;
    io::copy(&mut source_file, &mut dest_file).with_context(|| {
        format!(
            "copy archive path: {} -> {}",
            source.display(),
            dest.display()
        )
    })?;
    Ok(())
}

fn remove_path_no_follow(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)
        .with_context(|| format!("inspect removal path: {}", path.display()))?;
    if meta.file_type().is_symlink() || meta.is_file() {
        fs::remove_file(path).with_context(|| format!("remove file: {}", path.display()))?;
    } else if meta.is_dir() {
        fs::remove_dir_all(path).with_context(|| format!("remove dir: {}", path.display()))?;
    } else {
        anyhow::bail!("unsupported Hishtory cleanup path type: {}", path.display());
    }
    Ok(())
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| format!("create dir: {}", parent.display()))?;
    }
    Ok(())
}

fn create_private_archive_root(archive_dir: &Path, root: &Path) -> Result<()> {
    fs::create_dir_all(archive_dir)
        .with_context(|| format!("create archive parent: {}", archive_dir.display()))?;
    validate_archive_ancestor_permissions(archive_dir)?;
    create_private_dir(root).with_context(|| {
        format!(
            "create exclusive archive root (must not already exist): {}",
            root.display()
        )
    })
}

fn validate_archive_ancestor_permissions(archive_dir: &Path) -> Result<()> {
    let absolute = if archive_dir.is_absolute() {
        archive_dir.to_path_buf()
    } else {
        std::env::current_dir()
            .context("resolve current directory for archive parent")?
            .join(archive_dir)
    };
    let archive_dir = normalize_lexical_path(&absolute)?;

    for ancestor in archive_dir.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let metadata = fs::metadata(ancestor)
            .with_context(|| format!("inspect archive ancestor: {}", ancestor.display()))?;
        anyhow::ensure!(
            metadata.is_dir(),
            "archive ancestor must be a directory: {}",
            ancestor.display()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = metadata.permissions().mode();
            let shared_writable = mode & 0o022 != 0;
            let sticky = mode & libc::S_ISVTX as u32 != 0;
            anyhow::ensure!(
                !shared_writable || sticky,
                "archive ancestor is writable by another local user without sticky-bit protection: {}",
                ancestor.display()
            );
        }
    }
    Ok(())
}

fn create_private_dir(path: &Path) -> Result<()> {
    fs::create_dir(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn open_new_archive_file(path: &Path) -> io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

fn write_new_archive_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = open_new_archive_file(path)?;
    file.write_all(bytes)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_hishtory_dry_run_does_not_delete_files() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        fs::create_dir_all(home.join(".hishtory")).unwrap();
        fs::write(home.join(".hishtory/.hishtory.db"), "sqlite").unwrap();
        fs::write(
            home.join(".zshrc"),
            "export PATH=\"$PATH:$HOME/.hishtory\"\nsource $HOME/.hishtory/config.zsh\n",
        )
        .unwrap();

        let report = cleanup_hishtory(CleanupOptions {
            home_dir: home.to_path_buf(),
            apply: false,
            archive_dir: None,
            no_archive: false,
            backup_name: None,
        })
        .unwrap();

        assert!(!report.applied);
        assert!(home.join(".hishtory/.hishtory.db").exists());
        assert!(home.join(".zshrc").exists());
        assert!(
            report
                .actions
                .iter()
                .any(|action| matches!(action, CleanupAction::RemovePath { .. }))
        );
        assert!(report.actions.iter().any(|action| {
            matches!(
                action,
                CleanupAction::RemoveEmptyStartupFile {
                    removed_lines: 2,
                    ..
                }
            )
        }));
    }

    #[test]
    fn cleanup_hishtory_apply_requires_archive_or_explicit_no_archive() {
        let dir = tempfile::tempdir().unwrap();
        let err = cleanup_hishtory(CleanupOptions {
            home_dir: dir.path().to_path_buf(),
            apply: true,
            archive_dir: None,
            no_archive: false,
            backup_name: None,
        })
        .unwrap_err();

        assert!(format!("{err:#}").contains("--archive-dir or --no-archive"));
    }

    #[test]
    fn cleanup_hishtory_apply_archives_and_removes_hishtory_only_startup_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let archive_dir = dir.path().join("archive");
        fs::create_dir_all(home.join(".hishtory")).unwrap();
        fs::write(home.join(".hishtory/.hishtory.db"), "sqlite").unwrap();
        fs::write(
            home.join(".zshrc"),
            "# Hishtory Config\nsource /home/me/.hishtory/config.zsh\n",
        )
        .unwrap();

        let report = cleanup_hishtory(CleanupOptions {
            home_dir: home.clone(),
            apply: true,
            archive_dir: Some(archive_dir.clone()),
            no_archive: false,
            backup_name: Some("backup".to_string()),
        })
        .unwrap();

        assert!(report.applied);
        assert!(!home.join(".hishtory").exists());
        assert!(!home.join(".zshrc").exists());
        assert!(archive_dir.join("backup/.hishtory/.hishtory.db").exists());
        assert!(archive_dir.join("backup/.zshrc").exists());
        assert_eq!(report.archive_root, Some(archive_dir.join("backup")));
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_hishtory_rejects_precreated_archive_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let archive_dir = dir.path().join("archive");
        let victim = dir.path().join("victim");
        fs::create_dir_all(home.join(".hishtory")).unwrap();
        fs::create_dir_all(&archive_dir).unwrap();
        fs::write(home.join(".hishtory/.hishtory.db"), "sqlite").unwrap();
        fs::write(&victim, "preserve").unwrap();
        symlink(&victim, archive_dir.join("backup")).unwrap();

        let err = cleanup_hishtory(CleanupOptions {
            home_dir: home,
            apply: true,
            archive_dir: Some(archive_dir),
            no_archive: false,
            backup_name: Some("backup".to_string()),
        })
        .unwrap_err();

        assert!(format!("{err:#}").contains("must not already exist"));
        assert_eq!(fs::read_to_string(victim).unwrap(), "preserve");
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_hishtory_rejects_shared_writable_archive_ancestor() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let shared = dir.path().join("shared");
        let archive_dir = shared.join("archive");
        fs::create_dir_all(home.join(".hishtory")).unwrap();
        fs::create_dir_all(&shared).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o0777)).unwrap();
        fs::write(home.join(".hishtory/.hishtory.db"), "sqlite").unwrap();

        let err = cleanup_hishtory(CleanupOptions {
            home_dir: home.clone(),
            apply: true,
            archive_dir: Some(archive_dir),
            no_archive: false,
            backup_name: Some("backup".to_string()),
        })
        .unwrap_err();

        assert!(format!("{err:#}").contains("writable by another local user"));
        assert!(home.join(".hishtory/.hishtory.db").exists());
    }

    #[test]
    fn cleanup_hishtory_apply_rewrites_mixed_startup_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(
            home.join(".bashrc"),
            "export EDITOR=vim\nsource /home/me/.hishtory/config.sh\nalias ll='ls -l'\n",
        )
        .unwrap();

        let report = cleanup_hishtory(CleanupOptions {
            home_dir: home.clone(),
            apply: true,
            archive_dir: None,
            no_archive: true,
            backup_name: None,
        })
        .unwrap();

        assert!(report.applied);
        assert_eq!(
            fs::read_to_string(home.join(".bashrc")).unwrap(),
            "export EDITOR=vim\nalias ll='ls -l'\n"
        );
        assert!(report.actions.iter().any(|action| {
            matches!(
                action,
                CleanupAction::RewriteStartupFile {
                    removed_lines: 1,
                    ..
                }
            )
        }));
    }

    #[test]
    fn cleanup_hishtory_preserves_unrelated_mentions_in_startup_files() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(
            home.join(".bashrc"),
            "# hishtory was retired\nexport MIGRATION_NOTE='hishtory was retired'\necho hishtory\neval \"echo hishtory init was old\"\nsource $HOME/.hishtory/config.sh\neval \"$(hishtory init bash)\"\n",
        )
        .unwrap();

        cleanup_hishtory(CleanupOptions {
            home_dir: home.clone(),
            apply: true,
            archive_dir: None,
            no_archive: true,
            backup_name: None,
        })
        .unwrap();

        assert_eq!(
            fs::read_to_string(home.join(".bashrc")).unwrap(),
            "# hishtory was retired\nexport MIGRATION_NOTE='hishtory was retired'\necho hishtory\neval \"echo hishtory init was old\"\n"
        );
    }

    #[test]
    fn cleanup_hishtory_rejects_archive_inside_removal_tree() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let hishtory_dir = home.join(".hishtory");
        fs::create_dir_all(&hishtory_dir).unwrap();
        fs::write(hishtory_dir.join(".hishtory.db"), "sqlite").unwrap();

        let err = cleanup_hishtory(CleanupOptions {
            home_dir: home,
            apply: true,
            archive_dir: Some(hishtory_dir.join("backups")),
            no_archive: false,
            backup_name: Some("backup".to_string()),
        })
        .unwrap_err();

        assert!(format!("{err:#}").contains("archive path overlaps cleanup source"));
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_hishtory_refuses_to_rewrite_symlinked_startup_file() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let target = dir.path().join("shared-zshrc");
        fs::write(&target, "source $HOME/.hishtory/config.zsh\n").unwrap();
        symlink(&target, home.join(".zshrc")).unwrap();

        let err = cleanup_hishtory(CleanupOptions {
            home_dir: home,
            apply: true,
            archive_dir: None,
            no_archive: true,
            backup_name: None,
        })
        .unwrap_err();

        assert!(format!("{err:#}").contains("symlinked startup file"));
        assert_eq!(
            fs::read_to_string(target).unwrap(),
            "source $HOME/.hishtory/config.zsh\n"
        );
    }
}
