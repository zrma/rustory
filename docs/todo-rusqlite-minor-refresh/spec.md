# Spec: rusqlite-minor-refresh

## 배경

- 요청 맥락: dependency refresh 후 `cargo outdated --workspace --depth 1`에서 직접 의존성 잔여 후보를 재확인했다.
- 현재 문제/기회: `rusqlite 0.39.0 -> 0.40.0`은 남은 직접 의존성 중 유일한 semver-minor 갱신 후보이며, storage API 사용부가 기존 테스트로 넓게 덮여 있다.

## 계획 스냅샷

- 목표: `rusqlite` 직접 의존성을 0.40 계열로 갱신하고 SQLite storage 동작이 기존 테스트/스모크에서 유지되는지 확인한다.
- 범위: `Cargo.toml`, `Cargo.lock`, todo/교훈 로그만 수정한다. `rand` 0.10 및 `ureq` 3.x major/API 변경은 이번 범위에서 제외한다.
- 검증 명령: `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" cargo test storage --workspace`, `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" scripts/check.sh --fast`, `scripts/run-manifest-checks.sh --mode quick --work-id rusqlite-minor-refresh`.
- 완료 기준: `rusqlite 0.40.0`이 lockfile에 반영되고, storage 테스트와 Rust 기본 빠른 게이트가 통과하며, 완료 todo가 삭제 가능해진다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" cargo update -p rusqlite` | `Cargo.toml`의 `rusqlite` 요구 버전을 0.40 계열로 갱신하고 lockfile을 갱신한다. |
| C2 | done | codex | `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" cargo test storage --workspace` | SQLite storage 회귀 테스트를 먼저 통과시킨다. |
| C3 | done | codex | `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" scripts/check.sh --fast` | fmt/test/clippy 빠른 표준 게이트를 통과시킨다. |
| C4 | done | codex | `scripts/run-manifest-checks.sh --mode quick --work-id rusqlite-minor-refresh` | todo readiness와 문서/스크립트 quick 게이트를 통과시킨다. |
| C5 | todo | codex | `scripts/check-todo-closure.sh` | 완료 todo 식별자를 `docs/LESSONS_LOG.md`에 남기고 `docs/todo-rusqlite-minor-refresh`를 삭제한다. |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3, C4. `rusqlite 0.40.0`과 `libsqlite3-sys 0.38.0`이 반영됐고 storage/빠른 표준 게이트가 통과했다.
- 미완료: C5.
- 다음 액션: `docs/LESSONS_LOG.md`에 `todo-rusqlite-minor-refresh` 마감 증거를 남기고 todo 디렉터리를 삭제한다.
- 검증 증거: `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" cargo update -p rusqlite`, `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" cargo test storage --workspace`, `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" scripts/check.sh --fast`, `scripts/run-manifest-checks.sh --mode quick --work-id rusqlite-minor-refresh`, `cargo outdated --workspace --depth 1`.
