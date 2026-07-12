use anyhow::{Context, Result, bail};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
}

impl Shell {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "bash" => Ok(Self::Bash),
            "zsh" => Ok(Self::Zsh),
            _ => bail!("unsupported shell: {value}"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
        }
    }
}

pub const HOOK_START: &str = "# >>> rustory hook >>>";
pub const HOOK_END: &str = "# <<< rustory hook <<<";
const LEGACY_HOOK_START: &str = "# >>> rustory >>>";
const LEGACY_HOOK_END: &str = "# <<< rustory <<<";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedHookFixReport {
    pub rc_file: PathBuf,
    pub shell: Shell,
    pub status: ManagedHookFixStatus,
    pub removed_blocks: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedHookFixStatus {
    Fixed,
    Ok,
    Skipped,
}

pub fn render_hook(shell: Shell) -> String {
    match shell {
        Shell::Bash => render_bash_hook(),
        Shell::Zsh => render_zsh_hook(),
    }
}

pub fn auto_fix_existing_managed_hook_blocks(
    install_path: &Path,
) -> Result<Vec<ManagedHookFixReport>> {
    let bin_dir = install_path
        .parent()
        .with_context(|| format!("install path has no parent: {}", install_path.display()))?;
    let mut reports = Vec::new();
    for (rc_file, shell) in managed_hook_candidate_files(&[])? {
        let existing = match std::fs::read_to_string(&rc_file) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err).with_context(|| format!("read rc file: {}", rc_file.display()));
            }
        };
        if !contains_managed_hook_block(&existing) {
            continue;
        }
        let block = render_managed_source_block(shell, bin_dir);
        reports.push(update_managed_hook_block(
            &rc_file,
            shell,
            &block,
            false,
            Some(existing),
        )?);
    }
    Ok(reports)
}

pub(crate) fn remove_managed_hook_blocks_from_paths(
    rc_files: &[PathBuf],
) -> Result<Vec<ManagedHookFixReport>> {
    let mut reports = Vec::new();
    for rc_file in rc_files {
        let rc_file = rc_file.clone();
        let shell = shell_for_rc_file(&rc_file);
        let existing = match std::fs::read_to_string(&rc_file) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(err).with_context(|| format!("read rc file: {}", rc_file.display()));
            }
        };
        if !contains_managed_hook_block(&existing) {
            continue;
        }

        let (cleaned, removed_blocks) = strip_managed_hook_blocks(&existing)?;
        if cleaned == existing {
            reports.push(ManagedHookFixReport {
                rc_file,
                shell,
                status: ManagedHookFixStatus::Ok,
                removed_blocks,
            });
            continue;
        }

        atomic_write_text_preserving_symlink(&rc_file, &cleaned)
            .with_context(|| format!("write rc file: {}", rc_file.display()))?;
        reports.push(ManagedHookFixReport {
            rc_file,
            shell,
            status: ManagedHookFixStatus::Fixed,
            removed_blocks,
        });
    }
    Ok(reports)
}

fn managed_hook_candidate_files(extra_rc_files: &[PathBuf]) -> Result<Vec<(PathBuf, Shell)>> {
    let home = home_dir()?;
    let mut candidates = Vec::new();
    if let Some(shell) = default_shell() {
        candidates.push((default_rc_file_for_home(&home, shell), shell));
    }
    for (name, shell) in [(".zshrc", Shell::Zsh), (".bashrc", Shell::Bash)] {
        let path = home.join(name);
        if !candidates.iter().any(|(candidate, _)| candidate == &path) {
            candidates.push((path, shell));
        }
    }
    for path in extra_rc_files {
        if !candidates.iter().any(|(candidate, _)| candidate == path) {
            candidates.push((path.clone(), shell_for_rc_file(path)));
        }
    }
    Ok(candidates)
}

fn shell_for_rc_file(path: &Path) -> Shell {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.contains("zsh") {
        Shell::Zsh
    } else if name.contains("bash") {
        Shell::Bash
    } else {
        default_shell().unwrap_or(Shell::Bash)
    }
}

fn update_managed_hook_block(
    rc_file: &Path,
    shell: Shell,
    block: &str,
    install_if_missing: bool,
    existing: Option<String>,
) -> Result<ManagedHookFixReport> {
    let existing = match existing {
        Some(content) => content,
        None => match std::fs::read_to_string(rc_file) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(err) => {
                return Err(err).with_context(|| format!("read rc file: {}", rc_file.display()));
            }
        },
    };
    let has_block = contains_managed_hook_block(&existing);
    if !has_block && !install_if_missing {
        return Ok(ManagedHookFixReport {
            rc_file: rc_file.to_path_buf(),
            shell,
            status: ManagedHookFixStatus::Skipped,
            removed_blocks: 0,
        });
    }

    let (cleaned, removed_blocks) = strip_managed_hook_blocks(&existing)?;
    let updated = append_managed_block(&cleaned, block);
    if updated == existing {
        return Ok(ManagedHookFixReport {
            rc_file: rc_file.to_path_buf(),
            shell,
            status: ManagedHookFixStatus::Ok,
            removed_blocks,
        });
    }

    if let Some(parent) = rc_file.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create rc parent: {}", parent.display()))?;
    }
    atomic_write_text_preserving_symlink(rc_file, &updated)
        .with_context(|| format!("write rc file: {}", rc_file.display()))?;
    Ok(ManagedHookFixReport {
        rc_file: rc_file.to_path_buf(),
        shell,
        status: ManagedHookFixStatus::Fixed,
        removed_blocks,
    })
}

fn contains_managed_hook_block(content: &str) -> bool {
    find_line_marker(content, HOOK_START).is_some()
        || find_line_marker(content, LEGACY_HOOK_START).is_some()
}

fn strip_managed_hook_blocks(content: &str) -> Result<(String, usize)> {
    strip_managed_marker_blocks(
        content,
        &[(HOOK_START, HOOK_END), (LEGACY_HOOK_START, LEGACY_HOOK_END)],
    )
}

pub(crate) fn strip_managed_marker_blocks(
    content: &str,
    marker_pairs: &[(&str, &str)],
) -> Result<(String, usize)> {
    let mut rest = content;
    let mut output = String::with_capacity(content.len());
    let mut removed = 0;
    while let Some((start, marker)) = find_next_line_marker(rest, marker_pairs) {
        let Some((start_marker, end_marker)) = marker_pairs
            .iter()
            .find(|(start_marker, _)| *start_marker == marker)
            .copied()
        else {
            bail!("managed block has unmatched end marker: {marker}");
        };
        output.push_str(&rest[..start]);
        let after_start_offset = marker_line_end(rest, start, start_marker);
        let after_start = &rest[after_start_offset..];
        let Some((end, next_marker)) = find_next_line_marker(after_start, marker_pairs) else {
            bail!("managed block is missing end marker {end_marker} after {start_marker}");
        };
        if next_marker != end_marker {
            bail!(
                "managed block marker nesting is invalid: expected {end_marker}, found {next_marker}"
            );
        }
        let skip = after_start_offset + marker_line_end(after_start, end, end_marker);
        rest = &rest[skip..];
        removed += 1;
    }
    output.push_str(rest);
    Ok((output, removed))
}

fn find_next_line_marker<'a>(
    content: &str,
    marker_pairs: &'a [(&'a str, &'a str)],
) -> Option<(usize, &'a str)> {
    marker_pairs
        .iter()
        .flat_map(|(start_marker, end_marker)| [*start_marker, *end_marker])
        .filter_map(|marker| find_line_marker(content, marker).map(|offset| (offset, marker)))
        .min_by_key(|(offset, _)| *offset)
}

fn find_line_marker(content: &str, marker: &str) -> Option<usize> {
    content.match_indices(marker).find_map(|(offset, _)| {
        let starts_line = offset == 0 || content.as_bytes().get(offset - 1) == Some(&b'\n');
        let after = offset + marker.len();
        let ends_line = after == content.len()
            || content.as_bytes().get(after) == Some(&b'\n')
            || (content.as_bytes().get(after) == Some(&b'\r')
                && content.as_bytes().get(after + 1) == Some(&b'\n'));
        (starts_line && ends_line).then_some(offset)
    })
}

fn marker_line_end(content: &str, offset: usize, marker: &str) -> usize {
    let after = offset + marker.len();
    if content[after..].starts_with("\r\n") {
        after + 2
    } else if content[after..].starts_with('\n') {
        after + 1
    } else {
        after
    }
}

fn append_managed_block(existing: &str, block: &str) -> String {
    if existing.trim().is_empty() {
        return block.to_string();
    }
    format!("{}\n\n{}", existing.trim_end(), block)
}

pub(crate) fn atomic_write_text_preserving_symlink(path: &Path, content: &str) -> Result<()> {
    let write_path = match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => std::fs::canonicalize(path)
            .with_context(|| format!("resolve symlinked file: {}", path.display()))?,
        Ok(_) => path.to_path_buf(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => path.to_path_buf(),
        Err(err) => return Err(err).with_context(|| format!("stat file: {}", path.display())),
    };
    let parent = write_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("create file parent: {}", parent.display()))?;
    let existing_permissions = std::fs::metadata(&write_path)
        .ok()
        .map(|metadata| metadata.permissions());
    let file_name = write_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("rustory-rc");
    let tmp_path = parent.join(format!(".{file_name}.tmp-{}", uuid::Uuid::new_v4()));

    let result = (|| -> Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o666);
        }
        let mut file = options
            .open(&tmp_path)
            .with_context(|| format!("create temporary file: {}", tmp_path.display()))?;
        file.write_all(content.as_bytes())
            .with_context(|| format!("write temporary file: {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("sync temporary file: {}", tmp_path.display()))?;
        if let Some(permissions) = existing_permissions {
            std::fs::set_permissions(&tmp_path, permissions)
                .with_context(|| format!("preserve file permissions: {}", write_path.display()))?;
        }
        std::fs::rename(&tmp_path, &write_path).with_context(|| {
            format!(
                "atomically replace file: {} -> {}",
                tmp_path.display(),
                write_path.display()
            )
        })?;
        if let Ok(parent_dir) = std::fs::File::open(parent) {
            let _ = parent_dir.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    result
}

fn render_managed_source_block(shell: Shell, bin_dir: &Path) -> String {
    let bin_expr = shell_path_expr(bin_dir);
    format!(
        "{HOOK_START}\n\
         # Managed by rustory installer. Re-run with --install-hook to update.\n\
         export PATH=\"{bin_expr}:$PATH\"\n\
         if command -v rr >/dev/null 2>&1; then\n\
           source <(rr hook --shell {})\n\
         fi\n\
         {HOOK_END}\n",
        shell.name()
    )
}

fn shell_path_expr(path: &Path) -> String {
    let home = match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home),
        None => return shell_escape_path(path),
    };
    match path.strip_prefix(&home) {
        Ok(rel) if rel.as_os_str().is_empty() => "$HOME".to_string(),
        Ok(rel) => format!("$HOME/{}", shell_escape_path(rel)),
        Err(_) => shell_escape_path(path),
    }
}

fn shell_escape_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
}

fn default_shell() -> Option<Shell> {
    let shell_name = std::env::var_os("SHELL")
        .and_then(|value| PathBuf::from(value).file_name().map(|name| name.to_owned()))
        .and_then(|name| name.to_str().map(str::to_string));
    match shell_name.as_deref() {
        Some("bash") => Some(Shell::Bash),
        Some("zsh") => Some(Shell::Zsh),
        _ => {
            #[cfg(target_os = "macos")]
            {
                Some(Shell::Zsh)
            }
            #[cfg(not(target_os = "macos"))]
            {
                Some(Shell::Bash)
            }
        }
    }
}

fn default_rc_file_for_home(home: &Path, shell: Shell) -> PathBuf {
    match shell {
        Shell::Bash => home.join(".bashrc"),
        Shell::Zsh => home.join(".zshrc"),
    }
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME env var not set")
}

fn render_bash_hook() -> String {
    r#"# rustory (rr) bash hook
# 설치(예): source <(rr hook --shell bash)

__RUSTORY_HOOK_INSTALLED=1
export RUSTORY_HOOK_INSTALLED=1

__rustory_last_histnum="$(HISTTIMEFORMAT= builtin history 1 | sed -e 's/^ *//' -e 's/ .*//')"
__rustory_last_start_histnum=""
__rustory_last_start_ms=""
__rustory_last_start_sec=""
__rustory_in_hook=""

__rustory_hook_disabled() {
  case "${RUSTORY_HOOK_DISABLE:-}" in
    ""|0|false|False|FALSE|no|No|NO|off|Off|OFF) return 1 ;;
    *) return 0 ;;
  esac
}

__rustory_epoch_ms() {
  local ts="${EPOCHREALTIME:-}"
  [[ -z "$ts" ]] && return 1

  local sec="${ts%%.*}"
  local frac=""
  if [[ "$ts" == *.* ]]; then
    frac="${ts#*.}"
  fi
  frac="${frac}000"
  frac="${frac:0:3}"

  case "$sec$frac" in
    ""|*[!0-9]*) return 1 ;;
  esac
  printf '%s\n' "$(( sec * 1000 + 10#$frac ))"
}

__rustory_preexec() {
  __rustory_hook_disabled && return 0
  [[ -n "$__rustory_in_hook" ]] && return 0

  local histnum="${HISTCMD:-}"
  if [[ -z "$histnum" ]]; then
    return 0
  fi
  if [[ "$histnum" == "$__rustory_last_start_histnum" || "$histnum" == "$__rustory_last_histnum" ]]; then
    return 0
  fi
  __rustory_last_start_histnum="$histnum"

  if [[ -n "${EPOCHREALTIME:-}" ]]; then
    __rustory_last_start_ms="$(__rustory_epoch_ms 2>/dev/null || true)"
  else
    __rustory_last_start_ms=""
    __rustory_last_start_sec="${SECONDS:-0}"
  fi
}

trap '__rustory_preexec' DEBUG

__rustory_precmd() {
  local exit_code=$?
  __rustory_hook_disabled && return 0
  __rustory_in_hook=1

  local line
  line="$(HISTTIMEFORMAT= builtin history 1 | sed -e 's/^ *//')"

  local histnum="${line%% *}"
  local raw_cmd="${line#"$histnum"}"
  # `history`는 line number와 command 사이에 두 칸을 둔다. 그 두 칸만
  # 제거해야 사용자가 입력한 privacy opt-out 앞 공백을 보존할 수 있다.
  if [[ "$raw_cmd" != "  "* ]]; then
    __rustory_in_hook=""
    return 0
  fi
  raw_cmd="${raw_cmd#  }"
  if [[ "$raw_cmd" == " "* ]]; then
    __rustory_last_histnum="$histnum"
    __rustory_last_start_ms=""
    __rustory_last_start_sec=""
    __rustory_in_hook=""
    return 0
  fi

  local cmd="$raw_cmd"
  cmd="${cmd#"${cmd%%[![:space:]]*}"}"

  if [[ -z "$histnum" || -z "$cmd" ]]; then
    __rustory_in_hook=""
    return 0
  fi
  if [[ "$histnum" == "$__rustory_last_histnum" ]]; then
    __rustory_in_hook=""
    return 0
  fi
  __rustory_last_histnum="$histnum"

  local duration_ms=0
  if [[ -n "$__rustory_last_start_ms" && -n "${EPOCHREALTIME:-}" ]]; then
    local end_ms
    end_ms="$(__rustory_epoch_ms 2>/dev/null || true)"
    if [[ -n "$end_ms" && "$end_ms" -ge "$__rustory_last_start_ms" ]]; then
      duration_ms=$(( end_ms - __rustory_last_start_ms ))
    fi
  elif [[ -n "$__rustory_last_start_sec" ]]; then
    local end_sec="${SECONDS:-0}"
    if [[ "$end_sec" -ge "$__rustory_last_start_sec" ]]; then
      duration_ms=$(( (end_sec - __rustory_last_start_sec) * 1000 ))
    fi
  fi
  __rustory_last_start_ms=""
  __rustory_last_start_sec=""
  __rustory_in_hook=""

  ( rr record --cmd "$cmd" --cwd "$PWD" --exit-code "$exit_code" --duration-ms "$duration_ms" --shell "bash" --hostname "${HOSTNAME:-}" >/dev/null 2>&1 ) &
  disown "$!" 2>/dev/null || true
}

# PROMPT_COMMAND에 1회만 주입
case ";$PROMPT_COMMAND;" in
  *";__rustory_precmd;"*) ;;
  *) PROMPT_COMMAND="__rustory_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}" ;;
esac

__rustory_ctrl_r() {
  __rustory_hook_disabled && return 0
  local original_line original_point selected
  original_line="$READLINE_LINE"
  original_point="$READLINE_POINT"
  # RUSTORY_SEARCH_LIMIT는 rr search 내부에서 config.toml보다 우선한다.
  selected="$(rr search)" || selected=""
  if [[ -z "$selected" ]]; then
    READLINE_LINE="$original_line"
    READLINE_POINT="$original_point"
    return 0
  fi

  READLINE_LINE="$selected"
  READLINE_POINT=${#READLINE_LINE}
}

bind -x '"\C-r":__rustory_ctrl_r'
"#
    .to_string()
}

fn render_zsh_hook() -> String {
    r#"# rustory (rr) zsh hook
# 설치(예): source <(rr hook --shell zsh)

typeset -g __RUSTORY_HOOK_INSTALLED=1
export RUSTORY_HOOK_INSTALLED=1

autoload -Uz add-zsh-hook

typeset -g __rustory_last_cmd=""
typeset -g __rustory_last_start_ms=""

__rustory_hook_disabled() {
  case "${RUSTORY_HOOK_DISABLE:-}" in
    ""|0|false|False|FALSE|no|No|NO|off|Off|OFF) return 1 ;;
    *) return 0 ;;
  esac
}

__rustory_epoch_ms() {
  local ts="${EPOCHREALTIME:-}"
  [[ -z "$ts" ]] && return 1

  local sec="${ts%%.*}"
  local frac=""
  if [[ "$ts" == *.* ]]; then
    frac="${ts#*.}"
  fi
  frac="${frac}000"
  frac="${frac[1,3]}"

  case "$sec$frac" in
    ""|*[!0-9]*) return 1 ;;
  esac
  printf '%s\n' "$(( sec * 1000 + 10#$frac ))"
}

__rustory_preexec() {
  if [[ "$1" == " "* ]]; then
    __rustory_last_cmd=""
    __rustory_last_start_ms=""
    return 0
  fi

  __rustory_last_cmd="$1"
  if [[ -n "${EPOCHREALTIME:-}" ]]; then
    __rustory_last_start_ms="$(__rustory_epoch_ms 2>/dev/null || true)"
  else
    __rustory_last_start_ms=""
  fi
}

__rustory_precmd() {
  local exit_code=$?
  __rustory_hook_disabled && return 0

  local cmd="$__rustory_last_cmd"
  cmd="${cmd#"${cmd%%[![:space:]]*}"}"
  if [[ -z "$cmd" ]]; then
    return 0
  fi

  local duration_ms=0
  if [[ -n "$__rustory_last_start_ms" && -n "${EPOCHREALTIME:-}" ]]; then
    local end_ms
    end_ms="$(__rustory_epoch_ms 2>/dev/null || true)"
    if [[ -n "$end_ms" && "$end_ms" -ge "$__rustory_last_start_ms" ]]; then
      duration_ms=$(( end_ms - __rustory_last_start_ms ))
    fi
  fi

  __rustory_last_cmd=""
  __rustory_last_start_ms=""

  ( rr record --cmd "$cmd" --cwd "$PWD" --exit-code "$exit_code" --duration-ms "$duration_ms" --shell "zsh" --hostname "${HOST:-}" >/dev/null 2>&1 ) &!
}

add-zsh-hook -d preexec __rustory_preexec 2>/dev/null || true
add-zsh-hook -d precmd __rustory_precmd 2>/dev/null || true
add-zsh-hook preexec __rustory_preexec
add-zsh-hook precmd __rustory_precmd

__rustory_widget_ctrl_r() {
  __rustory_hook_disabled && return 0
  local original_buffer original_cursor selected
  original_buffer="$BUFFER"
  original_cursor="$CURSOR"
  zle -I
  # RUSTORY_SEARCH_LIMIT는 rr search 내부에서 config.toml보다 우선한다.
  selected="$(rr search)" || selected=""
  if [[ -n "$selected" ]]; then
    BUFFER="$selected"
    CURSOR=${#BUFFER}
  else
    BUFFER="$original_buffer"
    CURSOR="$original_cursor"
  fi
  zle redisplay
}

if [[ -o interactive ]] && (( $+widgets )) && command -v bindkey >/dev/null 2>&1; then
  zle -N __rustory_ctrl_r_widget __rustory_widget_ctrl_r
  bindkey '^R' __rustory_ctrl_r_widget
fi
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_hook_contains_disable_and_ctrl_r() {
        let got = render_hook(Shell::Bash);
        assert!(got.contains("RUSTORY_HOOK_DISABLE"));
        assert!(got.contains("__rustory_hook_disabled()"));
        assert!(got.contains("0|false|False|FALSE|no|No|NO|off|Off|OFF"));
        assert!(got.contains("export RUSTORY_HOOK_INSTALLED=1"));
        assert!(!got.contains("${__RUSTORY_HOOK_INSTALLED:-}"));
        assert!(got.contains("RUSTORY_SEARCH_LIMIT"));
        assert!(got.contains("selected=\"$(rr search)\""));
        assert!(!got.contains("rr search --limit"));
        assert!(got.contains("original_line=\"$READLINE_LINE\""));
        assert!(got.contains("original_point=\"$READLINE_POINT\""));
        assert!(got.contains("READLINE_LINE=\"$selected\""));
        assert!(got.contains("READLINE_POINT=${#READLINE_LINE}"));
        assert!(!got.contains("READLINE_LINE=\"${READLINE_LINE:0:$READLINE_POINT}$selected"));
        assert!(got.contains("bind -x '\"\\C-r\":__rustory_ctrl_r'"));
        assert!(got.contains("trap '__rustory_preexec' DEBUG"));
        assert!(got.contains("local raw_cmd=\"${line#\"$histnum\"}\""));
        assert!(got.contains("if [[ \"$raw_cmd\" == \" \"* ]]; then"));
        assert!(got.contains("__rustory_epoch_ms()"));
        assert!(got.contains("--duration-ms"));
        assert!(got.contains("disown \"$!\""));

        assert!(!got.contains("rr|rr\\ *)"));
    }

    #[test]
    fn zsh_hook_contains_disable_and_ctrl_r() {
        let got = render_hook(Shell::Zsh);
        assert!(got.contains("RUSTORY_HOOK_DISABLE"));
        assert!(got.contains("__rustory_hook_disabled()"));
        assert!(got.contains("0|false|False|FALSE|no|No|NO|off|Off|OFF"));
        assert!(got.contains("export RUSTORY_HOOK_INSTALLED=1"));
        assert!(!got.contains("${__RUSTORY_HOOK_INSTALLED:-}"));
        assert!(got.contains("add-zsh-hook -d preexec __rustory_preexec"));
        assert!(got.contains("add-zsh-hook -d precmd __rustory_precmd"));
        assert!(got.contains("if [[ \"$1\" == \" \"* ]]; then"));
        assert!(got.contains("RUSTORY_SEARCH_LIMIT"));
        assert!(got.contains("selected=\"$(rr search)\""));
        assert!(!got.contains("rr search --limit"));
        assert!(got.contains("original_buffer=\"$BUFFER\""));
        assert!(got.contains("original_cursor=\"$CURSOR\""));
        assert!(got.contains("BUFFER=\"$selected\""));
        assert!(got.contains("CURSOR=${#BUFFER}"));
        assert!(!got.contains("LBUFFER+=\"$selected\""));
        assert!(got.contains("bindkey '^R'"));
        assert!(got.contains("zle -I"));
        assert!(got.contains("[[ -o interactive ]]"));
        assert!(got.contains("(( $+widgets ))"));
        assert!(got.contains("zle -N __rustory_ctrl_r_widget"));
        assert!(got.contains("__rustory_epoch_ms()"));
        assert!(got.contains("frac=\"${frac[1,3]}\""));

        assert!(!got.contains("rr|rr\\ *)"));
    }

    #[test]
    fn bash_hook_skips_space_prefixed_command_and_records_normal_command() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("recorded-commands.txt");
        let hook = render_hook(Shell::Bash);
        let script = format!(
            r#"
history -c
rr() {{
  [[ "$1" == "record" ]] || return 0
  shift
  while (( $# > 0 )); do
    if [[ "$1" == "--cmd" ]]; then
      printf '%s\n' "$2" >> "$RUSTORY_TEST_RECORD_LOG"
      return 0
    fi
    shift
  done
}}
{hook}
history -s ' echo private'
__rustory_precmd
wait
history -s 'echo public'
__rustory_precmd
wait
"#
        );

        let output = std::process::Command::new("bash")
            .args(["--noprofile", "--norc", "-c", &script])
            .env("RUSTORY_TEST_RECORD_LOG", &log_path)
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "bash hook smoke failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(std::fs::read_to_string(log_path).unwrap(), "echo public\n");
    }

    #[test]
    fn install_managed_hook_block_dedupes_legacy_and_current_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        let rr = dir.path().join(".local/bin/rr");
        std::fs::write(
            &rc,
            [
                "export KEEP=1\n",
                LEGACY_HOOK_START,
                "\nsource <(rr hook --shell zsh)\n",
                LEGACY_HOOK_END,
                "\n\n",
                HOOK_START,
                "\nsource <(rr hook --shell zsh)\n",
                HOOK_END,
                "\n",
            ]
            .join(""),
        )
        .unwrap();

        let block = render_managed_source_block(Shell::Zsh, rr.parent().unwrap());
        let report = update_managed_hook_block(&rc, Shell::Zsh, &block, true, None).unwrap();
        let content = std::fs::read_to_string(&rc).unwrap();

        assert_eq!(report.status, ManagedHookFixStatus::Fixed);
        assert_eq!(report.removed_blocks, 2);
        assert_eq!(content.matches(HOOK_START).count(), 1);
        assert_eq!(content.matches(HOOK_END).count(), 1);
        assert!(!content.contains(LEGACY_HOOK_START));
        assert!(content.contains("export KEEP=1"));
        assert!(content.contains("source <(rr hook --shell zsh)"));
    }

    #[test]
    fn strip_managed_hook_blocks_removes_current_and_legacy_blocks() {
        let content = [
            "export KEEP=1\n",
            LEGACY_HOOK_START,
            "\nsource <(rr hook --shell zsh)\n",
            LEGACY_HOOK_END,
            "\n\n",
            HOOK_START,
            "\nsource <(rr hook --shell bash)\n",
            HOOK_END,
            "\nexport KEEP_TOO=1\n",
        ]
        .join("");

        let (cleaned, removed_blocks) = strip_managed_hook_blocks(&content).unwrap();

        assert_eq!(removed_blocks, 2);
        assert!(!cleaned.contains(LEGACY_HOOK_START));
        assert!(!cleaned.contains(HOOK_START));
        assert!(cleaned.contains("export KEEP=1"));
        assert!(cleaned.contains("export KEEP_TOO=1"));
    }

    #[test]
    fn strip_managed_hook_blocks_preserves_unmanaged_layout() {
        let prefix = "export KEEP=1\n\n\n";
        let managed = [
            HOOK_START,
            "\nsource <(rr hook --shell zsh)\n",
            HOOK_END,
            "\n",
        ]
        .join("");
        let suffix = "\n\nexport KEEP_TOO=1\n\n\n";
        let content = format!("{prefix}{managed}{suffix}");

        let (cleaned, removed_blocks) = strip_managed_hook_blocks(&content).unwrap();

        assert_eq!(removed_blocks, 1);
        assert_eq!(cleaned, format!("{prefix}{suffix}"));
    }

    #[test]
    fn strip_managed_hook_blocks_ignores_quoted_marker_text() {
        let content = format!("echo '{HOOK_START}'\nexport KEEP=1\necho '{HOOK_END}'\n");

        let (cleaned, removed_blocks) = strip_managed_hook_blocks(&content).unwrap();

        assert_eq!(removed_blocks, 0);
        assert_eq!(cleaned, content);
    }

    #[test]
    fn strip_managed_hook_blocks_rejects_unmatched_and_nested_markers() {
        let unmatched = format!("{HOOK_START}\nexport KEEP=1\n");
        let nested = format!("{HOOK_START}\nexport KEEP=1\n{LEGACY_HOOK_START}\n");

        assert!(strip_managed_hook_blocks(&unmatched).is_err());
        assert!(strip_managed_hook_blocks(&nested).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_rc_write_preserves_symlink_and_target_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("shared-zshrc");
        let link = dir.path().join(".zshrc");
        std::fs::write(&target, "export KEEP=1\n").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
        symlink(&target, &link).unwrap();

        atomic_write_text_preserving_symlink(&link, "export KEEP=2\n").unwrap();

        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "export KEEP=2\n");
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[test]
    fn managed_hook_candidates_include_custom_rc_file() {
        let custom = PathBuf::from("/tmp/rustory-custom-shell.rc");

        let candidates = managed_hook_candidate_files(std::slice::from_ref(&custom)).unwrap();

        assert!(candidates.iter().any(|(path, _)| path == &custom));
    }

    #[test]
    fn auto_fix_skips_rc_without_managed_hook_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join(".bashrc");
        let block = render_managed_source_block(Shell::Bash, dir.path());
        let report = update_managed_hook_block(
            &rc,
            Shell::Bash,
            &block,
            false,
            Some("alias ll='ls -alh'\n".to_string()),
        )
        .unwrap();

        assert_eq!(report.status, ManagedHookFixStatus::Skipped);
        assert!(!rc.exists());
    }
}
