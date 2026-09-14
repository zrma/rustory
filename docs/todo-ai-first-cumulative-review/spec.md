# Spec: ai-first-cumulative-review

## 배경

공통 변경 리뷰의 범위·근거·누적 finding 관리가 소비 generated 지침에도 포함되어야 한다.

## 계획 스냅샷

- 목표: signed AI-first v1.7.2를 pin하고 고정 snapshot·범위와 후발 지적 분류를 generated 지침으로 전달한다.
- 범위: 선언·lock·generated AGENTS/harness 갱신과 native 작업 기록만 변경한다. profile·overlay·제품 코드·runtime·strict 권한은 보존한다.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-cumulative-review`.
- 완료 기준: source pin 및 standalone/interface, full native gate, 원격 SHA와 동일 SHA CI가 확인되고 완료 지식이 소유 artifact로 이관된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `python3 .ai-first/check.py` | signed source pin과 generated 지침 갱신, 기존 profile·overlay 보존 |
| C2 | in_progress | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-cumulative-review` | native 검증과 완료 이관, 원격 및 CI 확인 |

## 완료/미완료/다음 액션

- 완료: C1 및 full native release gate 통과.
- 미완료: C2의 완료 이관, 원격 및 CI 검증.
- 다음 액션: 기준 revision을 보존하고 완료 지식을 LESSONS_LOG로 이관한 뒤 strict safe push와 CI를 확인한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-ai-first-cumulative-review`, `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-cumulative-review` 통과.
