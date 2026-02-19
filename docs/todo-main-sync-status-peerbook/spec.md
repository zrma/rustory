# Spec: main-sync-status-peerbook

## 배경

- 요청 맥락: `rr sync-status`는 최근 확장(`--json`, `last_seen`, `--with-tracker`) 이후 운영 진단의 기본 명령으로 사용되고 있다.
- 현재 문제/기회: 현재는 `peer_state`/`peer_push_state`가 없는 peer가 출력에서 빠져, tracker/peerbook으로 발견된 신규 peer를 `sync-status`에서 바로 확인하기 어렵다.

## 계획 스냅샷

- 목표: `sync-status` 조회 대상을 peerbook까지 확장해 "아직 동기화 상태가 없는 peer"도 0 cursor 상태로 가시화한다.
- 범위: `src/storage.rs` 쿼리/테스트와 운영 문서(`docs/p2p.md`)만 수정한다.
- 검증 명령: `cargo fmt --all --check`, `cargo test --workspace`, `scripts/run-manifest-checks.sh --mode quick --work-id main-sync-status-peerbook`.
- 완료 기준: peerbook-only peer가 `sync-status`/JSON에 나타나고, 관련 단위 테스트 및 문서 설명이 갱신된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `cargo fmt --all --check && cargo test --workspace && scripts/run-manifest-checks.sh --mode quick --work-id main-sync-status-peerbook` | `list_peer_sync_status`를 peerbook 포함 기준으로 확장하고 테스트/문서를 갱신한다. |

## 완료/미완료/다음 액션

- 완료: `src/storage.rs`의 peer sync 상태 집계 쿼리에 `peer_book`를 합쳐 peerbook-only peer도 결과에 포함되도록 수정했고, `list_peer_sync_status_merges_pull_and_push_state` 테스트를 확장했다. `docs/p2p.md`에 0 cursor 표시 규칙을 문서화했다.
- 완료: `cargo fmt --all --check`, `cargo test --workspace`, `scripts/check-todo-readiness.sh`, `scripts/check-open-questions-schema.sh --require-closed`, `scripts/run-manifest-checks.sh --mode quick --work-id main-sync-status-peerbook`를 통과했다.
- 미완료: todo 마감 커밋(LESSONS 반영 + `docs/todo-main-sync-status-peerbook/` 삭제).
- 다음 액션: 구현 커밋을 먼저 출고한 뒤 todo 마감 커밋을 이어서 진행한다.
- 검증 증거: 위 명령 실행 로그.
