# Spec: reader-focused-docs

## 배경

- 공개 README와 문서 인덱스에서 제품 사용자보다 유지보수·agent 운영 안내가 앞선다.
- README 정책과 검사도 개인 운영 인덱스 및 상세 Agent Navigation을 강제해 같은 중복을 재생산한다.

## 계획 스냅샷

- 목표: 사용자가 제품 목적, 빠른 시작과 제품 문서를 먼저 찾고 기여자·agent는 작업 지침으로 이동한다.
- 범위: README, 문서 인덱스, README 역할 정책과 그 정책을 검사하는 스크립트.
- 검증 명령: `scripts/run-manifest-checks.sh --mode quick --work-id reader-focused-docs`.
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고 검증 명령이 재현 가능하게 남는다.

## 제약과 제외 범위

- CLI 옵션·동작은 코드와 help가 소유한다. 최소 온보딩 예시, 개인정보와 grid identity 경계는 유지한다.
- 기존 HANDOFF/EXECUTION_LOOP/CHANGE_CONTROL의 실행·권한·검증 계약은 유지한다.
- runtime, 제품 기능, framework pin, release version과 실제 fleet 배포는 변경하지 않는다.
- 완료 판단과 문서 역할의 이유는 README_OPERATING_POLICY와 LESSONS_LOG로 이관한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `scripts/check-readme-policy.sh` | README와 문서 인덱스에서 제품 안내를 우선하고 작업 경로는 링크로 연결 |
| C2 | todo | codex | `scripts/check-script-smoke.sh` | 역할 정책과 README 검사를 같은 계약으로 조정하고 필수 탐색 경로 보존 |
| C3 | todo | codex | `scripts/run-manifest-checks.sh --mode quick --work-id reader-focused-docs` | 문서 무결성·공개 경계·native 검증과 원래 acceptance 대조 후 지식 이관 |

## 완료/미완료/다음 액션

- 완료: 없음.
- 미완료: C1, C2, C3.
- 다음 액션: 요구사항을 확정하고 구현/검증을 진행한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-reader-focused-docs`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-reader-focused-docs/open-questions.md`.
