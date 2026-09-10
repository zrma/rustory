# Spec: ai-first-shared-interface

## 배경

- framework 공통 version/source/profile assertion을 native checker에 복제해 매번 갱신해야 한다.
- standalone checker 위임으로 반복 pin 수정을 제거하고 native 제품·release 정책을 유지한다.

## 계획 스냅샷

- 목표: v1.7.0 immutable release를 pin하고 공통 interface 검사를 standalone에 위임한다.
- 영향: repository agent bootstrap과 검증 경로. Rustory runtime 및 공개 API는 변경하지 않는다.
- 범위: 선언/lock/generated output, native interface 검사와 이관된 운영 교훈.
- 제약: source_kind=release 정책, 제품 identity/daemon assertions, publication 호출과 기존 profile 선택을 유지한다.
- 제외: verification/work-coordination 자동 설치, 앱 릴리스와 실제 장치 배포.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-shared-interface`.
- 완료 기준: C1..C3 통과 후 실제 검증 결과를 운영 문서에 이관하고 native safe push와 동일 SHA CI를 확인한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `python3 .ai-first/check.py` | release pin과 generated 정합성, profile 선택 보존 |
| C2 | todo | codex | `scripts/check-agent-harness-interface.sh` | 공통 위임 및 release 정책/제품 검사 보존 |
| C3 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-shared-interface` | full native gate, 완료 지식 이관과 publication 검증 |

## 완료/미완료/다음 액션

- 완료: native 시작 게이트와 범위 정립.
- 미완료: C1..C3.
- 다음 액션: pinned source로 render하고 native gate를 실행한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-ai-first-shared-interface`.
