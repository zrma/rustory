use anyhow::{Result, bail};

#[derive(Clone, Copy, Debug)]
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
}

pub fn render_hook(shell: Shell) -> String {
    match shell {
        Shell::Bash => render_bash_hook(),
        Shell::Zsh => render_zsh_hook(),
    }
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

  # rr 자체는 기록하지 않는다.
  case "$cmd" in
    rr|rr\ *) __rustory_in_hook=""; return 0 ;;
  esac

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

  # rr 자체는 기록하지 않는다.
  case "$cmd" in
  rr|rr\ *)
    __rustory_last_cmd=""
    __rustory_last_start_ms=""
    return 0
  ;;
  esac

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
    fn bash_hook_contains_disable_and_ctrl_r_and_rr_filter() {
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

        // ensure we skip both `rr` and `rr ...`
        assert!(got.contains("case \"$cmd\" in"));
        assert!(got.contains("rr|rr\\ *)"));
    }

    #[test]
    fn zsh_hook_contains_disable_and_ctrl_r_and_rr_filter() {
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

        // ensure we skip both `rr` and `rr ...`
        assert!(got.contains("case \"$cmd\" in"));
        assert!(got.contains("rr|rr\\ *)"));
    }
}
