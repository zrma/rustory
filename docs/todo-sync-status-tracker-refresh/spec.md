# Spec: sync-status-tracker-refresh

## 배경

- 요청 맥락: `samplex-x86_64`가 tracker에는 새로 announce되고 있는데 `samplex-arm64`의 `rr sync-status --json --with-tracker`/`rr mesh --watch --with-tracker`에서는 stale peer처럼 보였다.
- 현재 문제/기회: 기존 `--with-tracker`는 tracker ping 상태만 붙이고 tracker peer list의 최신 `last_seen`/device metadata를 report에 병합하지 않아 로컬 `peer_book` 캐시가 오래되면 watch UI가 잘못된 stale 판정을 보여줄 수 있다.

## 계획 스냅샷

- 목표: `--with-tracker` status/watch 경로에서 tracker peer list를 읽어 최신 peer metadata를 report에 반영한다.
- 범위: `sync-status` report 생성과 CLI watch 호출부만 수정한다. P2P sync protocol, relay dialing, tracker announce TTL은 변경하지 않는다.
- 검증 명령: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/check.sh --fast`.
- 완료 기준: stale 표시는 tracker list의 최신 `last_seen`을 우선 반영하고, 기존 로컬 캐시가 더 최신이면 로컬 값을 유지한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `cargo test sync_status_report` | tracker peer list의 최신 metadata가 `sync-status` report에 반영되도록 구현 |
| C2 | in_progress | codex | `cargo test --workspace` | 기존 sync/search/P2P 동작 회귀 확인 |
| C3 | in_progress | codex | `cargo clippy --workspace --all-targets -- -D warnings` | lint와 dead-code 회귀 확인 |

## 완료/미완료/다음 액션

- 완료: `sync-status` report 생성 시 tracker peer list를 읽어 최신 `last_seen`/device metadata를 병합하도록 구현했다.
- 미완료: 릴리즈/배포 뒤 실제 `samplex-arm64`에서 stale 표시가 사라지는지 확인하고 closeout할 필요가 있다.
- 다음 액션: `finalize-and-push` 후 필요 시 버전 릴리즈/노드 업데이트를 진행한다.
- 검증 증거: `cargo test sync_status_report`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/check.sh --fast`.
