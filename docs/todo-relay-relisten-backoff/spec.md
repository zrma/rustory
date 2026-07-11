# Spec: relay-relisten-backoff

## 배경

- 요청 맥락: `v1.0.50` Mac canary에서 도달 불가능한 relay 주소가 즉시 close/re-listen을 반복해 stderr가 45초에 약 10.9MB 증가했다.
- 현재 문제/기회: relay listener close 경로에 cooldown이 없어 DNS 또는 네트워크 장애가 CPU와 디스크를 소모하는 tight loop로 확대된다.

## 계획 스냅샷

- 목표: relay re-listen을 5초부터 최대 60초까지 지수 백오프하고 reservation 성공 시 초기화한다.
- 범위: P2P server relay listener lifecycle, backoff 단위 테스트, Mac canary 로그 증가 검증.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id relay-relisten-backoff`.
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고 검증 명령이 재현 가능하게 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test relay_relisten_schedule_backs_off_and_caps_without_duplicate_timer --workspace` | 5, 10, 20, 40, 60초 백오프와 중복 timer 방지 |
| C2 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id relay-relisten-backoff` | 전체 회귀 게이트 통과 |
| C3 | todo | codex | `rr update --version v1.0.51` | Mac canary에서 tight loop와 stderr 폭증이 멈춤 |

## 완료/미완료/다음 액션

- 완료: C1.
- 미완료: C2-C3.
- 다음 액션: 전체 gate 후 `v1.0.51` canary에서 실제 로그 증가율을 확인한다.
- 검증 증거: `cargo test relay_relisten_schedule_backs_off_and_caps_without_duplicate_timer --workspace`.
