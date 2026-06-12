# Spec: dependency-patch-refresh

## 배경

- 요청 맥락: 활성 `docs/todo-*`가 없어 다음 유지보수 후보를 점검했고, 직접 의존성 patch drift가 남아 있음을 확인했다.
- 현재 문제/기회: `regex`, `rusqlite`, `uuid`는 patch 최신 버전이 존재하므로 API 변경 없는 lockfile refresh로 회귀 위험을 낮출 수 있다.

## 계획 스냅샷

- 목표: 직접 의존성 patch drift(`regex 1.12.3 -> 1.12.4`, `rusqlite 0.40.0 -> 0.40.1`, `uuid 1.23.2 -> 1.23.3`)를 lockfile 수준에서 갱신하고, 기존 audit 잔여 경계가 변하지 않았음을 확인한다.
- 범위: `Cargo.lock`, 이 작업의 todo 문서, 완료 시 `docs/LESSONS_LOG.md`와 todo closure만 포함한다. API 변경, major/minor dependency 전환, P2P 동작 변경은 제외한다.
- 검증 명령: `cargo outdated --workspace --depth 1`, `cargo audit`, `cargo fmt --all --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/run-manifest-checks.sh --mode quick --work-id dependency-patch-refresh`.
- 완료 기준: patch 후보가 최신 lockfile에 반영되고, `cargo audit`에는 기존 허용 잔여 `RUSTSEC-2024-0436 paste`만 남으며, Rust 기본 검증과 manifest quick 게이트가 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo outdated --workspace --depth 1` | 직접 의존성 patch drift 후보 확인 |
| C2 | done | codex | `cargo update -p regex -p rusqlite -p uuid` | patch 후보 lockfile 갱신 |
| C3 | done | codex | `cargo audit` | 보안 advisory 잔여가 기존 `RUSTSEC-2024-0436 paste` 경계에서 변하지 않았는지 확인 |
| C4 | done | codex | `cargo fmt --all --check && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` | Rust 기본 검증 통과 |
| C5 | todo | codex | `scripts/finalize-and-push.sh --message "build: refresh patch dependencies" --work-id dependency-patch-refresh` | patch refresh change를 표준 경로로 커밋/푸시하고 다음 closure change에서 todo 삭제 |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3, C4. `Cargo.lock`에서 `regex 1.12.4`, `rusqlite 0.40.1`, `uuid 1.23.3` 및 관련 transitive patch(`hashlink 0.12.0`, `libsqlite3-sys 0.38.1`, `regex-syntax 0.8.11`)가 반영됐다.
- 미완료: C5.
- 다음 액션: `build: refresh patch dependencies` change를 표준 finalize/push 경로로 출고한 뒤, 완료 todo를 lessons log에 내재화하고 todo workspace를 삭제한다.
- 검증 증거: `scripts/start-work.sh --work-id dependency-patch-refresh`, `cargo outdated --workspace --depth 1` (`All dependencies are up to date, yay!`), `cargo audit` (기존 허용 `RUSTSEC-2024-0436 paste` 1건), `cargo fmt --all --check`, `cargo test --workspace` (132 passed), `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/run-manifest-checks.sh --mode quick --work-id dependency-patch-refresh`.
