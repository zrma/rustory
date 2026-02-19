# Spec: sync-status-peer-age

## 배경

- 요청 맥락: `rr sync-status`는 `last_seen_unix`만 보여 주기 때문에 peer가 얼마나 오래 stale 상태인지 즉시 판단하기 어렵다.
- 현재 문제/기회: 운영자가 epoch 값을 수동 계산하지 않아도 되도록 `last_seen_age_sec`를 직접 제공하면 상태 판독 속도를 높일 수 있다.

## 계획 스냅샷

- 목표: `sync-status` peer report에 `last_seen_age_sec`를 추가해 텍스트/JSON에서 stale 경과 시간을 바로 확인할 수 있게 한다.
- 범위: `src/cli.rs`의 sync status report/출력/테스트와 `docs/p2p.md` 스키마만 수정한다.
- 검증 명령: `cargo fmt --all --check`, `cargo test --workspace`, `scripts/run-manifest-checks.sh --mode quick --work-id sync-status-peer-age`.
- 완료 기준: peer report에 `last_seen_age_sec|null`이 포함되고, 텍스트 출력/JSON 스키마/단위 테스트가 동기화된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `cargo fmt --all --check && cargo test --workspace && scripts/run-manifest-checks.sh --mode quick --work-id sync-status-peer-age` | `last_seen_age_sec` 계산/출력/직렬화 및 테스트/문서를 갱신한다. |

## 완료/미완료/다음 액션

- 완료: `src/cli.rs`에 `last_seen_age_sec` 계산 헬퍼와 peer report 필드를 추가했고 텍스트 출력에 `last_seen_age_sec` 컬럼을 반영했다.
- 완료: `sync_status_report_includes_pending_push_and_filter` 테스트를 확장하고 `compute_last_seen_age_sec_handles_past_and_future` 단위 테스트를 추가했다.
- 완료: `docs/p2p.md`의 `sync-status` 설명/JSON 스키마에 `last_seen_age_sec|null`을 반영했다.
- 미완료: 전체 검증 실행 및 커밋/푸시.
- 다음 액션: 검증 명령을 실행해 회귀를 확인한 뒤 구현 커밋을 출고한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-sync-status-peer-age`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-sync-status-peer-age/open-questions.md`.
