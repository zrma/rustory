# Spec: p2p-log-noise

## 배경

- 요청 맥락: 외부망 Linux 컨테이너에서 public relay circuit은 성립하고 `rr mesh --watch`도 `ok`지만, daemon 로그에 retryable `dial timeout after 12s`가 반복적으로 `warn`으로 남아 운영 장애처럼 보인다.
- 현재 문제/기회: per-peer P2P dial timeout은 relay/NAT 환경에서 같은 tick 또는 다음 tick의 inbound relay circuit/sync summary로 우회될 수 있다. 이 계열은 데이터 무결성/인증/DB 오류와 구분해 warning noise를 줄여야 한다.

## 계획 스냅샷

- 목표: retryable P2P timeout은 `info`로 낮추고, non-retryable 오류는 기존처럼 `warn`으로 유지한다.
- 범위: `p2p-sync` watch/per-peer 로그 레벨 분류와 해당 회귀 테스트. sync 성공 기준, retry 정책, transport 동작은 변경하지 않는다.
- 검증 명령: `cargo test p2p_request_failure_log_level --workspace` 및 `scripts/check.sh --fast`.
- 완료 기준: `dial timeout after ...`가 retryable/info로 분류되고, non-retryable 오류는 warn으로 남으며, fast gate가 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test p2p_request_failure_log_level --workspace` | retryable timeout과 non-retryable 오류의 로그 레벨 회귀 테스트 추가 |
| C2 | done | codex | `scripts/check.sh --fast` | p2p-sync watch/per-peer 로그 레벨 정리 후 fast gate 통과 |
| C3 | todo | codex | 외부망 daemon log 확인 | 릴리즈/배포 후 외부망 로그에서 retryable `dial timeout after ...`가 `info:`로 낮아지는지 확인 |

## 완료/미완료/다음 액션

- 완료: C1, C2.
- 미완료: C3.
- 다음 액션: 릴리즈하면 daemon 재시작 후 외부망 로그에서 retryable `dial timeout after ...`가 `info:`로 낮아지는지 확인한다.
- 검증 증거: `cargo fmt --all --check`, `cargo test p2p_request_failure_log_level --workspace` (1 passed), `cargo test is_retryable_p2p_request_error --workspace` (1 passed), `scripts/check.sh --fast` (250 passed + dev build).
