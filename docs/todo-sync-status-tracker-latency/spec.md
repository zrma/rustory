# Spec: sync-status-tracker-latency

## 배경

- 요청 맥락: `rr sync-status --with-tracker`가 tracker의 reachable/error만 보여 주고 있어, 실제 체감 상태(응답 지연)를 판단하기 어렵다.
- 현재 문제/기회: tracker가 살아 있어도 지연이 큰 상태를 조기에 감지하기 어렵기 때문에, 운영 진단용 필드(`latency_ms`)를 추가하면 즉시성 높은 판단이 가능해진다.

## 계획 스냅샷

- 목표: `sync-status --with-tracker` 결과에 tracker별 `latency_ms`를 추가해 텍스트/JSON 모두에서 확인 가능하게 만든다.
- 범위: `src/cli.rs`의 tracker ping 및 status report, 관련 단위 테스트, `docs/p2p.md`의 스키마 설명만 수정한다.
- 검증 명령: `cargo fmt --all --check`, `cargo test --workspace`, `scripts/run-manifest-checks.sh --mode quick --work-id sync-status-tracker-latency`.
- 완료 기준: tracker reachable 시 `latency_ms`가 채워지고, unreachable 시 `latency_ms=null`(또는 텍스트 `-`)로 출력되며 테스트/문서가 동기화된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `cargo fmt --all --check && cargo test --workspace && scripts/run-manifest-checks.sh --mode quick --work-id sync-status-tracker-latency` | tracker status에 `latency_ms`를 추가하고 CLI/JSON/테스트/문서를 갱신한다. |

## 완료/미완료/다음 액션

- 완료: `src/cli.rs`의 tracker ping 반환값을 지연(`u64`) 포함 형태로 확장하고, `SyncStatusTrackerReport`에 `latency_ms`를 추가해 텍스트/JSON 출력에 반영했다.
- 완료: `src/cli.rs` 테스트(`tracker_status_report_marks_unreachable_on_ping_error`, `tracker_status_report_includes_latency_on_success`)를 보강했고 `docs/p2p.md`의 `tracker_status[]` 스키마를 `latency_ms|null` 포함 형태로 갱신했다.
- 완료: `cargo fmt --all`, `cargo test --workspace`, `scripts/run-manifest-checks.sh --mode quick --work-id sync-status-tracker-latency`를 통과했다.
- 미완료: todo 마감 커밋(LESSONS 반영 + `docs/todo-sync-status-tracker-latency/` 삭제).
- 다음 액션: 구현 커밋을 먼저 출고한 뒤 todo 마감 커밋을 이어서 진행한다.
- 검증 증거: 위 명령의 통과 로그.
