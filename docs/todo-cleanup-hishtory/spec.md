# Spec: cleanup-hishtory

## 배경

- 요청 맥락: Hishtory import/hook handoff는 DB를 보존하는 전략이 맞지만, 몇 주 안정화 후에는 DB 포함 Hishtory 찌꺼기를 안전하게 정리할 명시적 경로가 필요하다.
- 현재 문제/기회: installer/import 단계에서 바로 삭제하면 fallback을 잃고, 수동 삭제만 안내하면 startup file과 DB/backup 처리 기준이 머신마다 흔들린다.

## 계획 스냅샷

- 목표: `rr cleanup-hishtory`를 dry-run 기본값으로 추가하고, 실제 삭제는 archive 또는 명시적 no-archive 선택이 있어야만 수행하게 한다.
- 범위: user HOME 아래 Hishtory 디렉터리/바이너리/startup hook 라인 정리, 문서화, 단위 테스트.
- 비범위: system-wide profile, package manager uninstall, Rustory import/installer 경로의 자동 삭제.
- 검증 명령: `cargo fmt --all --check`, `cargo test cleanup_hishtory --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/run-manifest-checks.sh --mode quick --work-id cleanup-hishtory`.
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고 검증 명령이 재현 가능하게 남는다. 완료 후 todo는 `docs/LESSONS_LOG.md` 참조와 함께 삭제한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test cleanup_hishtory --workspace` | dry-run 기본값, archive/no-archive guard, HOME cleanup 대상 탐지/삭제 구현 |
| C2 | done | codex | `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` | 기존 CLI/Hishtory import 동작과 회귀 없음 확인 |
| C3 | in_progress | codex | `scripts/run-manifest-checks.sh --mode quick --work-id cleanup-hishtory` | migration runbook과 todo closeout 기준 갱신 |

## 완료/미완료/다음 액션

- 완료: C1, C2.
- 미완료: C3 closeout.
- 다음 액션: 구현 커밋 후 `docs/LESSONS_LOG.md`에 `todo-cleanup-hishtory` 참조를 남기고 todo workspace를 삭제한다.
- 검증 증거: `cargo fmt --all --check`, `cargo test cleanup_hishtory --workspace` (5 passed), `cargo test --workspace` (197 passed, sandbox loopback 제한 때문에 권한 상승 재실행), `cargo clippy --workspace --all-targets -- -D warnings`, `target/debug/rr cleanup-hishtory --home /private/tmp/rustory-cleanup-smoke-missing`, `target/debug/rr cleanup-hishtory --apply --home /private/tmp/rustory-cleanup-smoke-missing` (expected guard failure), `scripts/run-manifest-checks.sh --mode quick --work-id cleanup-hishtory`.
