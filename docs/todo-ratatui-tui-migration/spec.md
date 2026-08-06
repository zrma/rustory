# Spec: ratatui-tui-migration

## 배경

- 요청 맥락: 수작업 ANSI 기반 Rustory TUI를 Ratatui로 전환하고 검증 후 daily-driver와 승인된 운영 fleet까지 배포한다.
- 현재 문제/기회: `search.rs`와 `watch_tui.rs`가 레이아웃, 화면 갱신, 선택 표시, terminal restore를 직접 구현해 사용자 표면 회귀를 테스트하기 어렵다.

## 계획 스냅샷

- 목표: watch dashboard와 inline history search의 렌더링을 Ratatui로 전환하되 기존 입력, 검색, P2P 상태 계산과 shell prompt 복원 계약은 보존한다.
- 범위: Ratatui 의존성, 공통 terminal/rendering adapter, watch/search TUI 렌더링, snapshot 성격의 buffer 테스트, release version과 출고 증거를 포함한다.
- 검증 명령: `cargo test search && cargo test watch_tui`, PTY 기반 `rr search`/watch 종료 복원 검사, `scripts/check-release-gates.sh --manifest-mode full --work-id ratatui-tui-migration`.
- 완료 기준: 기존 검색 키·삭제·가로 스크롤과 watch dashboard 정보가 유지되고, Ratatui `TestBackend` 검증·전체 release gate·동일 SHA CI·local/fleet 배포 후 health 검증이 모두 통과한다.

## 비목표

- 검색 relevance 알고리즘, P2P 동기화 상태 계산, tracker/relay protocol을 변경하지 않는다.
- Ctrl+R shell hook의 stdout command 반환 계약이나 fleet identity/secret 구성을 변경하지 않는다.
- 이번 마이그레이션을 계기로 색상 테마나 신규 탐색 기능을 추가하지 않는다.

## 결정 사항

- 전체 화면 watch는 Ratatui `Terminal`과 fullscreen viewport가 terminal diff와 restore를 소유한다.
- inline search는 기존 `/dev/tty` raw input과 prompt 보존 수명을 유지하고 Ratatui buffer/widget을 ANSI 출력으로 연결한다.
- 사용자 표면 회귀는 고정 크기 `TestBackend`와 실제 PTY 종료/복원 증거를 함께 사용한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test watch_tui` | watch dashboard를 Ratatui terminal/frame/widget으로 전환하고 기존 정보 밀도와 좁은 화면 경계를 보존한다. |
| C2 | done | codex | `cargo test search` | inline search 렌더링을 Ratatui로 전환하고 입력·삭제·가로 스크롤·prompt 복원을 보존한다. |
| C3 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id ratatui-tui-migration` | Rust, installer, P2P, publication 전체 검증을 통과한다. |
| C4 | in_progress | codex | `rr version --json && rr doctor --auto-fix && rr sync-status --json --with-tracker` | `v1.0.62` source를 출고하고 local canary와 승인된 운영 fleet을 순차 갱신해 daemon, tracker, pending, cluster health를 확인한다. |

## 완료/미완료/다음 액션

- 완료: C1, C2. watch는 Ratatui fullscreen `Terminal`/buffer diff로, search는 styled `Buffer`/`Paragraph`로 전환했고 기존 검색·dashboard 테스트가 통과했다.
- 미완료: C4.
- 다음 액션: 전체 source/release gate와 publication boundary를 통과한 뒤 출고 revision과 runtime 배포를 검증한다.
- 검증 증거: `cargo test search`, `cargo test watch_tui`, `cargo clippy --all-targets -- -D warnings`, 실제 PTY에서 search 결과·cursor restore와 두 watch 화면·alternate-screen restore 확인.
