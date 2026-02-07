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
  return 0
fi
__RUSTORY_HOOK_INSTALLED=1

__rustory_last_histnum=""

__rustory_precmd() {
  local exit_code=$?
  [[ -n "${RUSTORY_HOOK_DISABLE:-}" ]] && return 0

  local line
  line="$(HISTTIMEFORMAT= builtin history 1 | sed -e 's/^ *//')"

  local histnum="${line%% *}"
  local cmd="${line#* }"
  cmd="${cmd#"${cmd%%[![:space:]]*}"}"

  if [[ -z "$histnum" || -z "$cmd" ]]; then
    return 0
  fi
  if [[ "$histnum" == "$__rustory_last_histnum" ]]; then
    return 0
  fi
  __rustory_last_histnum="$histnum"

  # rr 자체는 기록하지 않는다.
  case "$cmd" in
    rr|rr\ *) return 0 ;;
  esac

  ( rr record --cmd "$cmd" --cwd "$PWD" --exit-code "$exit_code" --shell "bash" >/dev/null 2>&1 ) &
}

# PROMPT_COMMAND에 1회만 주입
case ";$PROMPT_COMMAND;" in
  *";__rustory_precmd;"*) ;;
  *) PROMPT_COMMAND="__rustory_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}" ;;
esac

__rustory_ctrl_r() {
  local limit="${RUSTORY_SEARCH_LIMIT:-100000}"
  local selected
  selected="$(rr search --limit "$limit")" || return 0
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
  return 0
fi
typeset -g __RUSTORY_HOOK_INSTALLED=1

autoload -Uz add-zsh-hook

typeset -g __rustory_last_cmd=""
typeset -g __rustory_last_start_us=""

__rustory_preexec() {
  __rustory_last_cmd="$1"
  if [[ -n "${EPOCHREALTIME:-}" ]]; then
    __rustory_last_start_us="${EPOCHREALTIME/./}"
  else
    __rustory_last_start_us=""
  fi
}

__rustory_precmd() {
  local exit_code=$?
  [[ -n "${RUSTORY_HOOK_DISABLE:-}" ]] && return 0

  local cmd="$__rustory_last_cmd"
  cmd="${cmd#"${cmd%%[![:space:]]*}"}"
  if [[ -z "$cmd" ]]; then
    return 0
  fi

  # rr 자체는 기록하지 않는다.
  case "$cmd" in
  rr|rr\ *)
    __rustory_last_cmd=""
    __rustory_last_start_us=""
    return 0
  ;;
  esac

  local duration_ms=0
  if [[ -n "$__rustory_last_start_us" && -n "${EPOCHREALTIME:-}" ]]; then
    local end_us="${EPOCHREALTIME/./}"
    if [[ "$end_us" -ge "$__rustory_last_start_us" ]]; then
      duration_ms=$(( (end_us - __rustory_last_start_us) / 1000 ))
    fi
  fi

  __rustory_last_cmd=""
  __rustory_last_start_us=""

  ( rr record --cmd "$cmd" --cwd "$PWD" --exit-code "$exit_code" --duration-ms "$duration_ms" --shell "zsh" --hostname "${HOST:-}" >/dev/null 2>&1 ) &!
}

add-zsh-hook preexec __rustory_preexec
add-zsh-hook precmd __rustory_precmd

__rustory_widget_ctrl_r() {
  local limit="${RUSTORY_SEARCH_LIMIT:-100000}"
  local selected
  selected="$(rr search --limit "$limit")" || return 0
  if [[ -n "$selected" ]]; then
    LBUFFER+="$selected"
  fi
  zle redisplay
}

zle -N __rustory_ctrl_r_widget __rustory_widget_ctrl_r
bindkey '^R' __rustory_ctrl_r_widget
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
        assert!(got.contains("RUSTORY_SEARCH_LIMIT"));
        assert!(got.contains("bind -x '\"\\C-r\":__rustory_ctrl_r'"));

        // ensure we skip both `rr` and `rr ...`
        assert!(got.contains("case \"$cmd\" in"));
        assert!(got.contains("rr|rr\\ *)"));
    }

    #[test]
    fn zsh_hook_contains_disable_and_ctrl_r_and_rr_filter() {
        let got = render_hook(Shell::Zsh);
        assert!(got.contains("RUSTORY_HOOK_DISABLE"));
        assert!(got.contains("RUSTORY_SEARCH_LIMIT"));
        assert!(got.contains("bindkey '^R'"));

        // ensure we skip both `rr` and `rr ...`
        assert!(got.contains("case \"$cmd\" in"));
        assert!(got.contains("rr|rr\\ *)"));
    }
}
