# Spec: main-sync-status

## 배경

- 요청 맥락: 메인 기능 구현 흐름으로 전환하면서 peer별 동기화 상태를 즉시 확인할 수 있는 진단 명령이 필요했다.
- 현재 문제/기회: 기존에는 `p2p-sync` 실행 로그에 의존해야 해서 pull/push cursor와 pending push를 한 번에 파악하기 어려웠다.

## 계획 스냅샷

- 목표: `main-sync-status` 작업을 단일 기준(spec)으로 관리하고 안전하게 구현한다.
- 범위: 현재 요청에 포함된 코드/문서/스크립트 변경만 수행한다.
- 검증 명령: `cargo fmt --all --check`, `cargo test --workspace`, `scripts/run-manifest-checks.sh --mode quick --work-id main-sync-status`.
- 완료 기준: `rr sync-status` 명령이 로컬 ingest head + peer별 `pull_cursor/push_cursor/pending_push`를 출력하고, 관련 테스트/문서가 갱신된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `cargo fmt --all --check && cargo test --workspace && scripts/run-manifest-checks.sh --mode quick --work-id main-sync-status` | `rr sync-status` 구현/검증 완료 후 다음 메인 기능 착수 전 마감 정리 |

## 완료/미완료/다음 액션

- 완료: `src/storage.rs`에 sync 상태 조회 API(`latest_ingest_seq`, `list_peer_sync_status`, `count_pending_push_entries`)를 추가했고, `src/cli.rs`에 `sync-status` 서브커맨드를 연결했다. 이후 후속 work-id(`main-sync-status-json`)에서 JSON 출력과 `last_seen_unix` 확장까지 반영했다.
- 미완료: 초기 `main-sync-status` work-id 자체 마감(todo 삭제 + 교훈 로그 반영) 정리.
- 다음 액션: `main-sync-status-json` 마감 시점에 `main-sync-status`와 함께 closure 커밋으로 정리한다.
- 검증 증거: `cargo fmt --all --check`, `cargo test --workspace`, `scripts/check-todo-readiness.sh docs/todo-main-sync-status`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-main-sync-status/open-questions.md`, `scripts/run-manifest-checks.sh --mode quick --work-id main-sync-status`.
