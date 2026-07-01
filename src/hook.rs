use anyhow::{Context, Result, bail};
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
    for (rc_file, shell) in managed_hook_candidate_files()? {
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

fn managed_hook_candidate_files() -> Result<Vec<(PathBuf, Shell)>> {
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
    Ok(candidates)
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

    let (cleaned, removed_blocks) = strip_managed_hook_blocks(&existing);
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
    std::fs::write(rc_file, updated)
        .with_context(|| format!("write rc file: {}", rc_file.display()))?;
    Ok(ManagedHookFixReport {
        rc_file: rc_file.to_path_buf(),
        shell,
        status: ManagedHookFixStatus::Fixed,
        removed_blocks,
    })
}

fn contains_managed_hook_block(content: &str) -> bool {
    content.contains(HOOK_START) || content.contains(LEGACY_HOOK_START)
}

fn strip_managed_hook_blocks(content: &str) -> (String, usize) {
    let mut rest = content;
    let mut output = String::with_capacity(content.len());
    let mut removed = 0;
    while let Some((start, start_marker, end_marker)) = find_next_managed_block_start(rest) {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + start_marker.len()..];
        let Some(end) = after_start.find(end_marker) else {
            output.push_str(&rest[start..]);
            return (output, removed);
        };
        let mut skip = start + start_marker.len() + end + end_marker.len();
        if rest[skip..].starts_with('\n') {
            skip += 1;
        }
        rest = &rest[skip..];
        removed += 1;
    }
    output.push_str(rest);
    (trim_repeated_blank_lines(&output), removed)
}

fn find_next_managed_block_start(content: &str) -> Option<(usize, &'static str, &'static str)> {
    [(HOOK_START, HOOK_END), (LEGACY_HOOK_START, LEGACY_HOOK_END)]
        .into_iter()
        .filter_map(|(start_marker, end_marker)| {
            content
                .find(start_marker)
                .map(|offset| (offset, start_marker, end_marker))
        })
        .min_by_key(|(offset, _, _)| *offset)
}

fn append_managed_block(existing: &str, block: &str) -> String {
    if existing.trim().is_empty() {
        return block.to_string();
    }
    format!("{}\n\n{}", existing.trim_end(), block)
}

fn trim_repeated_blank_lines(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut blank = false;
    for line in content.lines() {
        let is_blank = line.trim().is_empty();
        if is_blank && blank {
            continue;
        }
        result.push_str(line);
        result.push('\n');
        blank = is_blank;
    }
    result.trim_matches('\n').to_string()
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

if [[ -n "${__RUSTORY_HOOK_INSTALLED:-}" ]]; then
  export RUSTORY_HOOK_INSTALLED=1
  return 0
fi
__RUSTORY_HOOK_INSTALLED=1
export RUSTORY_HOOK_INSTALLED=1

__rustory_last_histnum=""
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
  local cmd="${line#* }"
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
  local selected
  # RUSTORY_SEARCH_LIMIT는 rr search 내부에서 config.toml보다 우선한다.
  selected="$(rr search)" || return 0
  [[ -z "$selected" ]] && return 0

  READLINE_LINE="${READLINE_LINE:0:$READLINE_POINT}$selected${READLINE_LINE:$READLINE_POINT}"
  READLINE_POINT=$(( READLINE_POINT + ${#selected} ))
}

bind -x '"\C-r":__rustory_ctrl_r'
"#
    .to_string()
}

fn render_zsh_hook() -> String {
    r#"# rustory (rr) zsh hook
# 설치(예): source <(rr hook --shell zsh)

if [[ -n "${__RUSTORY_HOOK_INSTALLED:-}" ]]; then
  export RUSTORY_HOOK_INSTALLED=1
  return 0
fi
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

add-zsh-hook preexec __rustory_preexec
add-zsh-hook precmd __rustory_precmd

__rustory_widget_ctrl_r() {
  __rustory_hook_disabled && return 0
  zle -I
  local selected
  # RUSTORY_SEARCH_LIMIT는 rr search 내부에서 config.toml보다 우선한다.
  selected="$(rr search)" || return 0
  if [[ -n "$selected" ]]; then
    LBUFFER+="$selected"
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
        assert!(got.contains("RUSTORY_SEARCH_LIMIT"));
        assert!(got.contains("selected=\"$(rr search)\""));
        assert!(!got.contains("rr search --limit"));
        assert!(got.contains("bind -x '\"\\C-r\":__rustory_ctrl_r'"));
        assert!(got.contains("trap '__rustory_preexec' DEBUG"));
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
        assert!(got.contains("RUSTORY_SEARCH_LIMIT"));
        assert!(got.contains("selected=\"$(rr search)\""));
        assert!(!got.contains("rr search --limit"));
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
