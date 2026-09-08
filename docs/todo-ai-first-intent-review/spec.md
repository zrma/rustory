# Spec: ai-first-intent-review

## 배경

- 현재 작업 템플릿은 계획과 검증 스키마를 제공하지만 해결할 문제, 관찰 가능한 결과와 영향 판단이 초기 기본 문구에 묻힐 수 있다.
- AI-first v1.6.0의 intent-aware review 계약을 기존 Rustory 작업 흐름과 연결한다.

## 계획 스냅샷

- 목표: 새 작업의 실제 문제와 기대 결과를 spec에 구체화하고 원래 의도를 diff 및 evidence와 비교할 수 있게 한다.
- 범위: framework release pin과 생성 파일, native source assertion, start-work 템플릿과 실행 지침을 갱신한다.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-intent-review`.
- 완료 기준: standalone/interface, 기존 native smoke와 full gate 통과 후 계약·판단 근거를 소유 문서와 교훈 로그로 이관하고 todo를 정리한다.

## 영향과 경계

- 영향: 유지보수자가 새 작업을 시작하고 결과를 리뷰하는 문서 경로에 적용한다.
- 제약: 기존 readiness 필드, 질문 스키마, 초기화 성공 및 기존 파일 보존 동작을 유지한다. schema PASS는 실제 요구사항 확정을 대신하지 않는다.
- 제외 범위: 제품 코드, runtime model, 제품 release와 운영 배포는 변경하지 않는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `scripts/check-agent-harness-interface.sh` | 정식 framework pin과 생성 계약을 일치시킨다 |
| C2 | todo | codex | `scripts/check-script-smoke.sh --work-id ai-first-intent-review` | 새 intent 안내와 기존 초기화·보존 계약을 함께 검증한다 |
| C3 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-intent-review` | full gate 검증 후 owning artifact로 지식을 이관한다 |

## 완료/미완료/다음 액션

- 완료: 작업 범위와 수용 기준 수립.
- 미완료: C1, C2, C3.
- 다음 액션: readiness 확인 후 bounded adoption을 구현한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-ai-first-intent-review`.
