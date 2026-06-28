# Spec: daily-driver-readiness

## 배경

- 요청 맥락: Hishtory public sync가 503으로 실사용 신뢰를 잃어 Rustory를 daily driver로 전환해야 한다.
- 현재 문제/기회: Docker relay-only acceptance는 green이지만, 실사용 전환에서는 tracker/token/relay 설정 오류를 daemon 시작 전에 더 빨리 잡아야 한다.

## 계획 스냅샷

- 목표: Rustory를 direct-only가 아닌 tracker + relay 기반 daily-driver 경로로 안전하게 전환할 수 있게 한다.
- 범위: daemon 전환 전 preflight guard, 관련 문서, Docker relay acceptance 재검증, 다음 multi-machine soak 항목 추적.
- 검증 명령: `cargo fmt --all --check`, `cargo test daemon_ --workspace`, `scripts/check.sh --fast --acceptance`.
- 완료 기준: daemon preflight guard가 코드/문서/테스트에 반영되고, relay circuit을 실제 사용한 acceptance가 통과하며, 남은 실사용 soak 항목이 명시된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test daemon_ --workspace` | `rr daemon --preflight`로 configured tracker ping을 자식 프로세스 시작 전에 검증한다. |
| C2 | done | codex | `scripts/check.sh --fast --acceptance` | Docker macOS/Linux acceptance와 two-peer relay-only acceptance를 재실행해 relay circuit 사용과 DB 수렴을 확인한다. |
| C3 | in_progress | codex | `rr daemon --preflight` | 로컬 MacBook과 `node0`에서 실제 tracker/token/relay 설정을 preflight로 확인하고 canary sync 증거를 남긴다. |
| C4 | todo | codex | `rr sync-status --json --with-tracker` | 24시간 soak 또는 사용자가 승인한 축약 soak에서 반복 실패/timeout 폭증이 없는지 확인한다. |

## 완료/미완료/다음 액션

- 완료: daemon preflight guard 구현, 문서 반영, Docker relay acceptance 재검증.
- 미완료: 실제 MacBook + `node0` preflight/canary sync와 24시간 또는 축약 soak 증거.
- 다음 액션: 현재 배포된 tracker/relay 주소와 token으로 양쪽 머신에서 `rr daemon --preflight`, canary record, `rr sync-status --json --with-tracker`를 수행한다.
- 검증 증거: `cargo fmt --all --check`, `cargo test daemon_ --workspace`, `scripts/check.sh --fast --acceptance`.
