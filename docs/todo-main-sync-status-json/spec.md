# Spec: main-sync-status-json

## 배경

- 요청 맥락: `rr sync-status` 1차 구현 이후 자동화 파이프라인/외부 도구 연계를 위해 머신 파싱 가능한 출력이 필요하다.
- 현재 문제/기회: 현재 텍스트 출력은 사람이 보기에는 충분하지만 스크립트 연계(모니터링, 알림, 대시보드)에는 추가 파싱 비용이 든다.

## 계획 스냅샷

- 목표: `rr sync-status --json` 출력과 기본 필터(`--peer`) 동작을 안정적으로 제공한다.
- 범위: `src/cli.rs`, `src/storage.rs`, 관련 문서(`docs/p2p.md`) 및 테스트만 수정한다.
- 검증 명령: `cargo fmt --all --check`, `cargo test --workspace`, `scripts/run-manifest-checks.sh --mode quick --work-id main-sync-status-json`.
- 완료 기준: JSON 출력 스키마가 고정되고, 파싱/필터 동작 테스트가 추가되며, 문서 예시가 갱신된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `cargo fmt --all --check && cargo test --workspace && scripts/run-manifest-checks.sh --mode quick --work-id main-sync-status-json` | `rr sync-status --json` 구현/테스트/문서 반영 및 후속 확장(JSON 필드/필터) 인계 정리 |

## 완료/미완료/다음 액션

- 완료: `src/cli.rs`에 `rr sync-status --json` 플래그를 추가하고, `SyncStatusReport` 빌더를 도입해 텍스트/JSON 출력 경로를 공통화했다. 파싱 테스트(`--json`)와 리포트 단위 테스트(필터 + pending_push 계산 + JSON 직렬화)를 추가했고 `docs/p2p.md` 사용 예시/스키마를 갱신했다. 후속 확장으로 `last_seen_unix`(peerbook 기반)를 JSON/텍스트 출력에 반영했고, `src/storage.rs`에 peerbook `last_seen` 조회 API 및 관련 테스트를 추가했다.
- 미완료: tracker live 상태(`reachable`, `last_error`) 같은 네트워크 런타임 지표를 `sync-status`와 결합할지 결정 필요.
- 다음 액션: `sync-status` 런타임 상태 결합 범위를 별도 work-id로 분리해(옵션 플래그/타임아웃 정책 포함) 요구사항을 확정한다.
- 검증 증거: `cargo fmt --all --check`, `cargo test --workspace`, `scripts/check-todo-readiness.sh docs/todo-main-sync-status-json`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-main-sync-status-json/open-questions.md`, `scripts/run-manifest-checks.sh --mode quick --work-id main-sync-status-json`.
