# Spec: doctor-autofix-watch-layout

## 배경

- 요청 맥락: daily driver 운영 중 `rr doctor`가 권한/기본 secret filter 문제를 알려주지만 사용자가 각 머신에서 수동으로 고치기 번거롭다.
- 추가 증상: `rr sync-status --watch` 화면이 단순 cursor 표라서 전체 grid를 읽기 어렵고, peer 이름/큰 cursor/rate 값이 길어지면 열이 밀린다.
- 현재 문제/기회: 안전하게 자동 수정 가능한 hygiene 항목은 `rr doctor --auto-fix`로 처리하고, watch TUI는 우선 로컬 관점의 torrent-like grid view로 바꾼다. true global active flow는 원격 telemetry가 필요하므로 별도 후속 범위로 둔다.

## 계획 스냅샷

- 목표: `rr doctor --auto-fix`로 안전한 로컬 권한/기본 config hygiene을 자동 보정하고, `sync-status --watch`를 local grid 관제 화면으로 개선한다.
- 범위: doctor CLI flag, safe auto-fix report, config/key/db 권한 보정, 기본 `record_ignore_regex` 보정, watch 출력 폭 제한/축약, local view label/flow 표현, 문서 갱신.
- 결정: `doctor --auto-fix`는 relay/token/config parse error처럼 운영 판단이 필요한 값을 자동 변경하지 않는다. `sync-status --watch`는 현재 데이터 모델이 정확히 제공하는 local-known flow map으로 개선하고, peer 간 global active transfer telemetry는 후속 범위로 둔다.
- 검증 명령: `cargo test doctor_auto_fix`, `cargo test sync_status_watch`, `cargo fmt --all --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고, 신규 테스트와 기존 빠른 검증이 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `cargo test doctor_auto_fix` | `rr doctor --auto-fix`가 config/db/key 권한과 누락된 기본 `record_ignore_regex`를 안전하게 보정한다. |
| C2 | todo | codex | `cargo test sync_status_watch` | `sync-status --watch`가 긴 peer/device/cursor/rate 값에서도 열 폭을 넘기지 않고 local-to-peer push / peer-to-local pull 관점을 torrent-like grid로 표시한다. |
| C3 | todo | codex | `cargo fmt --all --check && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` | 구현/문서가 repo 검증 기준을 통과한다. |

## 완료/미완료/다음 액션

- 완료: 없음.
- 미완료: C1-C3.
- 다음 액션: CLI와 렌더러를 구현하고 focused test를 추가한다.
- 검증 증거: planned `cargo test doctor_auto_fix`, planned `cargo test sync_status_watch`, planned `cargo fmt --all --check`, planned `cargo test --workspace`, planned `cargo clippy --workspace --all-targets -- -D warnings`.
