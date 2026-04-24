#!/usr/bin/env bash
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "$ROOT" ]]; then
  exit 0
fi

if [[ ! -d "$ROOT/.jj" ]]; then
  exit 0
fi

if [[ "${ALLOW_GIT_COMMIT_IN_JJ_ROOT:-0}" == "1" ]]; then
  nonempty_at="$(
    cd "$ROOT"
    jj log -r '@ & ~empty()' --no-graph \
      -T 'change_id.short() ++ " " ++ commit_id.short() ++ " " ++ description.first_line() ++ "\n"' || true
  )"

  if [[ -n "$nonempty_at" && "${ALLOW_GIT_COMMIT_WITH_NONEMPTY_AT:-0}" != "1" ]]; then
    cat >&2 <<'EOF'
[FAIL] ALLOW_GIT_COMMIT_IN_JJ_ROOT=1 예외 모드에서도 working-copy(@)가 non-empty면 git commit을 차단합니다.
       - 현재 변경은 jj 커밋/정리 후 재시도하세요.
       - 점검: jj st
       - 정리: jj describe -m "<type>: <summary>" 또는 jj abandon <change-id>
       - 예외 override(매우 제한): ALLOW_GIT_COMMIT_WITH_NONEMPTY_AT=1
EOF
    while IFS= read -r head; do
      [[ -z "$head" ]] && continue
      echo "       - $head" >&2
    done <<< "$nonempty_at"
    exit 1
  fi

  orphan_heads="$(
    cd "$ROOT"
    jj log -r 'heads(all()) & ~bookmarks() & ~empty() & ~@' --no-graph \
      -T 'change_id.short() ++ " " ++ commit_id.short() ++ " " ++ description.first_line() ++ "\n"' || true
  )"

  if [[ -n "$orphan_heads" && "${ALLOW_GIT_COMMIT_WITH_ORPHAN_HEADS:-0}" != "1" ]]; then
    cat >&2 <<'EOF'
[FAIL] ALLOW_GIT_COMMIT_IN_JJ_ROOT=1 예외 모드에서도 고아 non-empty jj head가 남아 있으면 git commit을 차단합니다.
       - 우선 정리: jj log -r "heads(all()) & ~bookmarks() & ~empty()"
       - 정리 방법: jj abandon <change-id> 또는 bookmark 지정
       - 예외 override(매우 제한): ALLOW_GIT_COMMIT_WITH_ORPHAN_HEADS=1
EOF
    while IFS= read -r head; do
      [[ -z "$head" ]] && continue
      echo "       - $head" >&2
    done <<< "$orphan_heads"
    exit 1
  fi
  exit 0
fi

cat >&2 <<'EOF'
[FAIL] root 저장소는 jj 우선 정책입니다. git commit을 차단합니다.
       - 권장: jj describe -m "<type>: <summary>"
       - 푸시: jj git push --remote origin --bookmark <bookmark>
       - 예외(정말 필요할 때만): ALLOW_GIT_COMMIT_IN_JJ_ROOT=1 git commit ...
EOF
exit 1
