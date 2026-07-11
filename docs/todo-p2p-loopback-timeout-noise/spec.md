# Spec: p2p-loopback-timeout-noise

## 배경

- 요청 맥락: `v1.0.49` 배포 후 macOS daemon stderr가 loopback 후보 dial timeout으로 계속 증가하는 것을 발견했다.
- 현재 문제/기회: 기존 routine classifier는 loopback connection refused와 protocol negotiation만 숨기고 macOS의 `Operation timed out (os error 60)`은 warning으로 남긴다.

## 계획 스냅샷

- 목표: loopback direct dial timeout을 기본 로그에서 routine noise로 분류하되 공인 주소 timeout과 실제 relay 실패 warning은 보존한다.
- 범위: outgoing connection event classifier, 실제 macOS 오류 문자열 회귀 테스트, 출고 검증.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id p2p-loopback-timeout-noise`.
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고 검증 명령이 재현 가능하게 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test loopback_direct_dial_failure_is_log_noise --workspace` | macOS loopback timeout을 routine noise로 분류 |
| C2 | done | codex | `cargo test loopback_protocol_negotiation_failure_is_log_noise --workspace` | 기존 negotiation 분류 회귀 없음 |
| C3 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id p2p-loopback-timeout-noise` | 전체 회귀 게이트 통과 |

## 완료/미완료/다음 액션

- 완료: C1-C2.
- 미완료: C3.
- 다음 액션: 전체 게이트 통과 후 patch release로 fleet에 재배포한다.
- 검증 증거: `cargo test loopback_direct_dial_failure_is_log_noise --workspace`, `cargo test loopback_protocol_negotiation_failure_is_log_noise --workspace`.
