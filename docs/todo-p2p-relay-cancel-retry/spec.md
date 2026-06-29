# Spec: p2p-relay-cancel-retry

## 배경

- 요청 맥락: private-node-01 신규 설치 후 tracker discovery는 성공했지만 `rr p2p-sync --push`가 relay circuit dial에서
  `Response from behaviour was canceled: oneshot canceled`로 실패했다.
- 현재 문제/기회: 기존 grid의 relay circuit은 계속 accepted 되고 있으므로, 이 실패는 설치/토큰 문제가 아니라
  relay client dial의 일시 실패 처리와 신규 디바이스 온보딩 안내 부족으로 좁혀진다.

## 계획 스냅샷

- 목표: relay behaviour cancellation을 retryable transient dial failure로 처리하고, one-line install이 daemon을
  자동 시작하지 않는다는 운영 경계를 문서화한다.
- 범위: P2P retry 분류, 회귀 테스트, 배포 문서, 1.0.6 version bump.
- 검증 명령: `cargo fmt --all --check`, `cargo test p2p --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.
- 완료 기준: v1.0.6 후보가 로컬 검증을 통과하고, 사용자가 private-node-01에 직접 설치하지 않은 상태로 update 가능한 릴리즈 경로를 준비한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test p2p --workspace` | `oneshot canceled` relay dial failure를 retryable로 분류하고 회귀 테스트 추가 |
| C2 | done | codex | `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` | v1.0.6 version bump와 문서 업데이트 검증 |
| C3 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id p2p-relay-cancel-retry` | 출고 게이트/릴리즈 asset/원격 push 완료 |

## 완료/미완료/다음 액션

- 완료: C1, C2.
- 미완료: C3.
- 다음 액션: full release gate를 실행하고 v1.0.6 릴리즈 가능 여부를 확정한다.
- 검증 증거: `cargo fmt --all --check` 통과, `cargo test p2p --workspace` 33 passed, `cargo clippy --workspace --all-targets -- -D warnings` 통과.
