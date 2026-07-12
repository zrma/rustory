# Spec: CI script smoke portability

## 계획 스냅샷

- 목표: macOS 로컬 검증에서는 통과하지만 Linux Docs Integrity에서 실패하는 release fixture 의존성 누락을 제거한다.
- 범위: `scripts/check-script-smoke.sh`의 임시 release tree 구성과 Linux CI 회귀 검증.
- 검증 명령: `scripts/check-script-smoke.sh && scripts/check-release-gates.sh --manifest-mode full --work-id ci-script-smoke-portability`
- 완료 기준: fixture가 Linux GLIBC 검증 helper를 포함하고 로컬 full gate와 원격 Docs Integrity가 모두 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `scripts/check-script-smoke.sh` | Linux release fixture helper 누락 보완 |
| C2 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id ci-script-smoke-portability` | 로컬 전체 회귀 검증 |
| C3 | todo | codex | `gh run list --commit <sha>` | 원격 CI 3종 재검증 |

## 완료/미완료/다음 액션

- 완료: 원격 Docs Integrity 실패 로그에서 Linux 전용 helper 누락 원인을 확인했다.
- 미완료: C1-C3 구현과 검증.
- 다음 액션: release fixture가 사용하는 GLIBC helper를 fixture tree에 포함하고 전체 gate를 재실행한다.
- 검증 증거: Docs Integrity run `29178881957`의 `check-linux-glibc-baseline.sh: No such file or directory`.
