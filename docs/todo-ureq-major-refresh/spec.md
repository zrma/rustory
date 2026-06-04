# Spec: ureq-major-refresh

## 배경

- 요청 맥락: `cargo outdated --workspace --depth 1` 기준 `ureq 2.12.1 -> 3.3.0` major refresh 후보가 남아 있다.
- 현재 문제/기회: `ureq`는 tracker/transport/HTTP retry 경로에서 직접 사용되며 3.x에서 agent/error/response API가 바뀔 수 있으므로 단독 마일스톤으로 검증한다.

## 계획 스냅샷

- 목표: `ureq` 직접 의존성을 3.x로 올리고 기존 HTTP sync/tracker/retry 동작을 보존한다.
- 범위: `ureq` 의존성 선언/lockfile, `ureq` API 호출부, 관련 검증 문서만 수정한다. `rand` major refresh는 별도 작업으로 남긴다.
- 검증 명령: `cargo fmt --all --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/smoke_p2p_local.sh`, `scripts/run-manifest-checks.sh --mode full --repo-key rustory --work-id ureq-major-refresh`.
- 완료 기준: `ureq`가 `cargo outdated --workspace --depth 1` 직접 후보에서 사라지고, HTTP 관련 unit/integration 및 full release gates가 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `rg -n "\bureq\b|ureq::|AgentBuilder|Error::Status|Error::Transport" src Cargo.toml` | 기존 `ureq` 직접 사용면 식별 |
| C2 | done | codex | `cargo update -p ureq --precise 3.3.0` | `Cargo.toml`/`Cargo.lock`를 `ureq` 3.x로 갱신 |
| C3 | done | codex | `cargo test --workspace` | `ureq` 3.x API 변경에 맞춰 tracker/transport/retry 호출부 보정 |
| C4 | done | codex | `cargo outdated --workspace --depth 1` | `ureq` direct outdated 해소 및 `rand` 별도 잔존 확인 |
| C5 | todo | codex | `scripts/finalize-and-push.sh --message "build: update ureq" --work-id ureq-major-refresh` | 작업 커밋 출고 후 별도 closure 커밋에서 todo 삭제 |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3, C4. `ureq` direct dependency를 3.3.0으로 갱신했고 tracker/transport/retry 호출부를 3.x API에 맞췄다.
- 미완료: C5. `rand 0.8.6 -> 0.10.1` direct major 후보는 별도 작업으로 남긴다.
- 다음 액션: `build: update ureq` 작업 커밋을 출고한 뒤, closure 커밋에서 `docs/todo-ureq-major-refresh`를 삭제한다.
- 검증 증거: `rg -n "\bureq\b|ureq::|AgentBuilder|Error::Status|Error::Transport" src Cargo.toml`, `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" cargo update -p ureq --precise 3.3.0`, `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" cargo test --workspace` (132 passed), `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" cargo clippy --workspace --all-targets -- -D warnings`, `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" scripts/smoke_p2p_local.sh`, `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" scripts/run-manifest-checks.sh --mode full --repo-key rustory --work-id ureq-major-refresh`, `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" cargo outdated --workspace --depth 1`.
