# Spec: history-dedupe

## 배경

- 요청 맥락: Hishtory import와 multi-device sync 이후 같은 날짜/호스트/경로/명령/exit code 조합이 반복된 row를 로컬에서 정리할 수 있어야 한다.
- 현재 문제/기회: `rr delete`는 entry id 또는 regex를 직접 지정해야 하므로 같은 명령 반복 row를 안전하게 줄이는 maintenance UX가 부족하다.
- 동기화 경계: Rustory는 현재 delete tombstone sync를 제공하지 않으므로 중복 정리는 local-only 작업으로 둔다.

## 계획 스냅샷

- 목표: `rr dedupe` 명령을 추가해 동일 UTC 날짜 + hostname + CWD + command + exit code 중복 row를 local-only로 정리한다.
- 범위:
  - 기본 실행은 dry-run이며 실제 삭제는 `--apply`가 있을 때만 수행한다.
  - 기본 scope는 현재 device id이다. `--all-devices`는 local DB 안의 모든 device id를 대상으로 한다.
  - 기본 keep 정책은 newest이고 `--keep oldest`를 제공한다.
  - `--older-than-days`로 최근 row를 제외할 수 있다.
  - peer push cursor가 있으면 아직 가장 느린 peer가 받지 못한 row는 삭제 후보에서 제외한다.
- 범위 밖:
  - grid-wide tombstone/delete propagation.
  - record hook 자동 dedupe.
  - Hishtory DB 원본 수정.
- 검증 명령:
  - `cargo test dedupe --workspace`
  - `cargo fmt --all --check`
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `scripts/run-manifest-checks.sh --mode quick --work-id history-dedupe`
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고 검증 명령이 재현 가능하게 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test dedupe --workspace` | storage dedupe 집계/삭제/keep/scope/push-floor 회귀 테스트 |
| C2 | done | codex | `rr dedupe --help` | CLI surface와 safety flags 문서화 |
| C3 | todo | codex | `scripts/run-manifest-checks.sh --mode quick --work-id history-dedupe` | 출고/closeout 전 repo gate 통과 및 문서 정합성 확인 |

## 완료/미완료/다음 액션

- 완료: C1, C2.
- 미완료: C3.
- 다음 액션: 구현 commit을 먼저 푸시하고, 별도 closeout에서 C3를 닫은 뒤 todo workspace를 삭제한다.
- 검증 증거: todo readiness, open-questions schema, `cargo test dedupe --workspace`, `rr dedupe --help`, temp DB smoke, full Rust test/clippy, quick manifest gate.
