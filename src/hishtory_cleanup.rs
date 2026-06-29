use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
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
                let root = archive_dir.join(
                    opts.backup_name
                        .unwrap_or_else(|| format!("hishtory-backup-{}", unix_now())),
                );
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
    line.to_ascii_lowercase().contains("hishtory")
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
        fs::write(&symlink_note, target.to_string_lossy().as_ref())
            .with_context(|| format!("archive symlink target: {}", symlink_note.display()))?;
        return Ok(());
    }
    if meta.is_dir() {
        fs::create_dir_all(dest)
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
    fs::copy(source, dest).with_context(|| {
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
}
