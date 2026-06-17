# Spec: time-patch-refresh

## 배경

- 요청 맥락: 활성 todo/issue/PR이 없는 상태에서 다음 자율 유지보수 후보를 검토했다.
- 현재 문제/기회: `cargo outdated --workspace --depth 1`에서 direct dependency `time`이 `0.3.47 -> 0.3.49`로 drift된 것을 확인했다.

## 계획 스냅샷

- 목표: `time` direct dependency patch drift를 lockfile 기준으로 갱신하고 기존 보안/테스트 게이트를 유지한다.
- 범위: `time`, `time-core`, `time-macros`의 Rust 1.95.0 호환 patch lockfile refresh와 해당 검증 증적 기록만 수행한다.
- 검증 명령: `cargo outdated --workspace --depth 1`, `cargo audit`, `cargo fmt --all --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/run-manifest-checks.sh --mode quick --work-id time-patch-refresh`, `scripts/smoke_p2p_local.sh`.
- 완료 기준: direct dependency drift가 사라지고, 기존 허용 audit warning 외 신규 보안 실패가 없으며, Rust 기본 검증/manifest quick/P2P smoke가 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `cargo outdated --workspace --depth 1` | `time` direct dependency patch drift 확인 및 lockfile refresh 적용 |
| C2 | todo | codex | `cargo audit` | 보안 advisory 상태가 기존 허용 `paste` warning 외 신규 실패 없이 유지되는지 확인 |
| C3 | todo | codex | `cargo fmt --all --check` + `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` | Rust 기본 검증 통과 |
| C4 | todo | codex | `scripts/run-manifest-checks.sh --mode quick --work-id time-patch-refresh` + `scripts/smoke_p2p_local.sh` | repo manifest quick 게이트와 P2P smoke 통과 |

## 완료/미완료/다음 액션

- 완료: 초기 todo readiness와 open-questions 닫힘 상태 확인.
- 미완료: C1, C2, C3, C4.
- 다음 액션: `cargo update -p time`을 적용하고 지정 검증 명령을 실행한다.
- 검증 증거: `scripts/start-work.sh --work-id time-patch-refresh`, `cargo update -p time --dry-run`.
