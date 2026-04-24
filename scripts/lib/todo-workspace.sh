#!/usr/bin/env bash

# Shared helpers to resolve todo workspace paths from docs/REPO_MANIFEST.yaml.
# Fallback is docs/todo-* to keep backward compatibility.

TODO_WORKSPACE_GLOB="docs/todo-*"
TODO_WORKSPACE_PARENT_REL="docs"
TODO_WORKSPACE_NAME_PATTERN="todo-*"
TODO_WORKSPACE_NAME_PREFIX="todo-"
TODO_WORK_ID_PATTERN='^[a-z0-9]+(-[a-z0-9]+)*$'

todo_workspace_is_valid_work_id() {
  local work_id="$1"
  [[ "$work_id" =~ $TODO_WORK_ID_PATTERN ]]
}

todo_workspace_load_config() {
  local root="$1"
  local manifest="${2:-$root/docs/REPO_MANIFEST.yaml}"
  local raw=""
  local stars=""

  TODO_WORKSPACE_GLOB="docs/todo-*"

  if [[ -f "$manifest" ]]; then
    raw="$(
      awk '
        /^[[:space:]]*todo_workspace_glob:[[:space:]]*/ {
          line = $0
          sub(/^[^:]+:[[:space:]]*/, "", line)
          print line
          exit
        }
      ' "$manifest"
    )"
    raw="${raw%%#*}"
    raw="${raw#"${raw%%[![:space:]]*}"}"
    raw="${raw%"${raw##*[![:space:]]}"}"
    raw="${raw%\'}"
    raw="${raw#\'}"
    raw="${raw%\"}"
    raw="${raw#\"}"
    if [[ -n "$raw" ]]; then
      TODO_WORKSPACE_GLOB="$raw"
    fi
  fi

  TODO_WORKSPACE_PARENT_REL="${TODO_WORKSPACE_GLOB%/*}"
  TODO_WORKSPACE_NAME_PATTERN="${TODO_WORKSPACE_GLOB##*/}"

  if [[ -z "$TODO_WORKSPACE_PARENT_REL" || "$TODO_WORKSPACE_PARENT_REL" == "$TODO_WORKSPACE_GLOB" ]]; then
    echo "invalid todo_workspace_glob (must include parent directory): $TODO_WORKSPACE_GLOB" >&2
    return 1
  fi
  if [[ "$TODO_WORKSPACE_PARENT_REL" == *"*"* || "$TODO_WORKSPACE_PARENT_REL" == *"?"* || "$TODO_WORKSPACE_PARENT_REL" == *"["* ]]; then
    echo "invalid todo_workspace_glob (wildcards in parent directory are not supported): $TODO_WORKSPACE_GLOB" >&2
    return 1
  fi

  stars="${TODO_WORKSPACE_NAME_PATTERN//[^*]/}"
  if [[ -z "$stars" || ${#stars} -ne 1 ]]; then
    echo "invalid todo_workspace_glob (basename must contain exactly one '*'): $TODO_WORKSPACE_GLOB" >&2
    return 1
  fi

  TODO_WORKSPACE_NAME_PREFIX="${TODO_WORKSPACE_NAME_PATTERN%\*}"
  if [[ -z "$TODO_WORKSPACE_NAME_PREFIX" ]]; then
    echo "invalid todo_workspace_glob (basename prefix before '*' is required): $TODO_WORKSPACE_GLOB" >&2
    return 1
  fi
}

todo_workspace_rel_for_work_id() {
  local work_id="$1"
  printf '%s/%s%s\n' "$TODO_WORKSPACE_PARENT_REL" "$TODO_WORKSPACE_NAME_PREFIX" "$work_id"
}

todo_workspace_abs_for_work_id() {
  local root="$1"
  local work_id="$2"
  printf '%s/%s\n' "$root" "$(todo_workspace_rel_for_work_id "$work_id")"
}

todo_workspace_find_dirs() {
  local root="$1"
  local parent_abs="$root/$TODO_WORKSPACE_PARENT_REL"
  if [[ ! -d "$parent_abs" ]]; then
    return 0
  fi
  find "$parent_abs" -maxdepth 1 -mindepth 1 -type d -name "$TODO_WORKSPACE_NAME_PATTERN" | sort
}

todo_workspace_extract_work_id() {
  local workspace_path="$1"
  local workspace_name="${workspace_path##*/}"
  if [[ "$workspace_name" != "$TODO_WORKSPACE_NAME_PREFIX"* ]]; then
    return 1
  fi
  printf '%s\n' "${workspace_name#$TODO_WORKSPACE_NAME_PREFIX}"
}

# Returns:
#   0 -> prints single work-id
#   1 -> invalid naming pattern/work-id format
#   2 -> no todo workspace found
#   3 -> multiple todo workspaces found (prints rel paths, one per line)
todo_workspace_discover_work_id() {
  local root="$1"
  local work_id_pattern="${2:-$TODO_WORK_ID_PATTERN}"
  local todo_dirs=()
  local todo_dir=""
  local resolved_work_id=""

  readarray -t todo_dirs < <(todo_workspace_find_dirs "$root")

  if (( ${#todo_dirs[@]} == 1 )); then
    if ! resolved_work_id="$(todo_workspace_extract_work_id "${todo_dirs[0]}")"; then
      echo "auto-detected todo workspace has invalid naming pattern: ${todo_dirs[0]#$root/} (expected prefix: $TODO_WORKSPACE_NAME_PREFIX)" >&2
      return 1
    fi
    if [[ ! "$resolved_work_id" =~ $work_id_pattern ]]; then
      echo "auto-detected work-id is invalid: $resolved_work_id (from ${todo_dirs[0]#$root/})" >&2
      return 1
    fi
    printf '%s\n' "$resolved_work_id"
    return 0
  fi

  if (( ${#todo_dirs[@]} == 0 )); then
    return 2
  fi

  for todo_dir in "${todo_dirs[@]}"; do
    echo "${todo_dir#$root/}"
  done
  return 3
}

todo_workspace_collect_deleted_work_ids() {
  local root="$1"
  local include_head_range="${2:-auto}"
  local run_head_range=0
  local path_prefix="$TODO_WORKSPACE_PARENT_REL/$TODO_WORKSPACE_NAME_PREFIX"

  if [[ "$include_head_range" == "auto" ]]; then
    if [[ -z "$(git -C "$root" status --porcelain --untracked-files=normal)" ]]; then
      run_head_range=1
    fi
  elif [[ "$include_head_range" == "1" ]]; then
    run_head_range=1
  fi

  {
    git -C "$root" diff --name-status -- "$TODO_WORKSPACE_PARENT_REL"
    git -C "$root" diff --cached --name-status -- "$TODO_WORKSPACE_PARENT_REL"
    if (( run_head_range == 1 )) && git -C "$root" rev-parse --verify --quiet HEAD^ >/dev/null; then
      git -C "$root" diff --name-status HEAD^..HEAD -- "$TODO_WORKSPACE_PARENT_REL"
    fi
  } \
    | awk -v prefix="$path_prefix" '
      $1 == "D" {
        path = $2
        if (index(path, prefix) != 1) {
          next
        }
        rest = substr(path, length(prefix) + 1)
        split(rest, parts, "/")
        if (parts[1] != "") {
          print parts[1]
        }
      }
    ' \
    | sort -u
}

# Returns:
#   0 -> prints single closed work-id
#   2 -> no closed work-id candidate
#   3 -> multiple closed work-id candidates (prints ids, one per line)
todo_workspace_discover_closed_work_id() {
  local root="$1"
  local include_head_range="${2:-auto}"
  local closed_work_ids=()
  local closed_work_id=""

  readarray -t closed_work_ids < <(todo_workspace_collect_deleted_work_ids "$root" "$include_head_range")

  if (( ${#closed_work_ids[@]} == 1 )); then
    printf '%s\n' "${closed_work_ids[0]}"
    return 0
  fi

  if (( ${#closed_work_ids[@]} == 0 )); then
    return 2
  fi

  for closed_work_id in "${closed_work_ids[@]}"; do
    echo "$closed_work_id"
  done
  return 3
}
