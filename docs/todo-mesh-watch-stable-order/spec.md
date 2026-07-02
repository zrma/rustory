# Spec: mesh-watch-stable-order

## 배경

- 요청 맥락: `rr mesh --watch`에서 몇 초 tick마다 peer 상태가 바뀌면서 `Mesh Topology`의 노드 위치와 `Flow Lanes` row 순서가 뒤섞여 보인다.
- 현재 문제/기회: mesh dashboard는 grid mental map을 유지해야 하므로 watch tick 사이의 peer 위치 안정성이 중요하다. 상태 기반 triage는 `rr sync-status --watch`가 맡고, mesh 화면은 stable peer name order를 기본값으로 둔다.

## 계획 스냅샷

- 목표: `rr mesh --watch`의 `Mesh Topology`와 `Flow Lanes` peer ordering을 device/peer display name 오름차순으로 고정한다.
- 범위: watch TUI 정렬 정책 분리, 회귀 테스트, P2P 문서 갱신.
- 범위 밖: `rr sync-status --watch`의 attention/state 정렬 변경, global peer-to-peer daemon telemetry, release/deploy 자동화 변경.
- 검증 명령:
  - `cargo test watch_tui --workspace`
  - `cargo test --all-targets`
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `scripts/run-manifest-checks.sh --mode quick --work-id mesh-watch-stable-order`
- 완료 기준: mesh watch는 이름순 stable ordering을 쓰고, sync-status watch는 기존 attention ordering을 유지한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test watch_tui --workspace` | watch TUI ordering 회귀 테스트 추가 |
| C2 | in_progress | codex | `scripts/finalize-and-push.sh --message "fix: stabilize mesh watch ordering" --work-id mesh-watch-stable-order` | 구현 커밋 push 후 별도 closeout으로 todo 삭제 |

## 완료/미완료/다음 액션

- 완료: C1.
- 미완료: C2.
- 다음 액션: 구현 커밋을 push한 뒤, 별도 docs closeout 커밋으로 C2 완료와 todo workspace 삭제를 처리한다.
- 검증 증거: `cargo test watch_tui --workspace`, `cargo test --all-targets`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/run-manifest-checks.sh --mode quick --work-id mesh-watch-stable-order`.
