# Spec: relay-hardening-release

## 배경

relay 연결 제한과 상태 집계의 로컬 검증이 끝났으며 사용자에게 검증된 수정본을 출고한다.
프로토콜과 기존 circuit 옵션은 보존한다. 운영 메모리 안정성은 제품 단위 검증과 별도다.

## 계획 스냅샷

- 목표: relay 보완을 v1.0.65 source와 검증된 release assets로 출고한다.
- 범위: patch version, full native gate, source publication, daily-driver release assets와 identity 검증.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id relay-hardening-release`, `scripts/release-version.sh --profile daily-driver --gate none`.
- 완료 기준: 원격 source/CI와 release asset의 version/revision/checksum이 일치하고 남은 지식을 소유 문서에 이관한다.

## 제약

공개 기록에는 개인 배포 정보나 raw 실행 로그를 남기지 않는다. 연결 제한은 프로세스의
전체 메모리 상한을 보장하지 않으며, 원인 미확정 OOM의 해결로 표현하지 않는다.
현재 wire protocol과 tracker 동작 변경은 제외한다. 운영 배포는 별도 소유 경로에서 검증한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id relay-hardening-release` | 버전 갱신과 full native/publication gate |
| C2 | todo | codex | `scripts/release-version.sh --profile daily-driver --gate none` | source/CI 및 release assets identity 검증 |
| C3 | todo | codex | `scripts/check-todo-closure.sh` | 소유 문서/교훈 이관과 packet 마감 |

## 완료/미완료/다음 액션

- 완료: relay 보완의 `scripts/check.sh`, quota/timeout/회복/circuit 검증.
- 미완료: C1, C2, C3.
- 다음 액션: v1.0.65를 준비하고 full gate를 실행한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-relay-hardening-release`.
