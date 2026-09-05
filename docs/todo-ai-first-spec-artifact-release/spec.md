# Spec: ai-first-spec-artifact-release

## 배경

- 요청 맥락: `ai-first-spec-artifact-release` 작업을 시작하기 전에 계획/검증 기준을 고정한다.
- 현재 문제/기회: 시작 단계를 수동으로 처리하면 계획 스냅샷/게이트 누락이 발생할 수 있다.

## 계획 스냅샷

- 목표: 검증된 AI-first spec/artifact 계약을 signed v1.5.0 release pin으로 확정한다.
- 범위: 최신 main 기반의 framework source pin, generated output과 interface identity 검사를 갱신한다. 제품 코드와 기존 WIP를 보존한다.
- 검증 명령: `scripts/run-manifest-checks.sh --mode quick --work-id ai-first-spec-artifact-release`.
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고 검증 명령이 재현 가능하게 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `scripts/run-manifest-checks.sh --mode quick --work-id ai-first-spec-artifact-release` | 최종 release pin, strict native/publication gate와 remote main 및 same-SHA CI를 검증한다. |

## 완료/미완료/다음 액션

- 완료: 없음.
- 미완료: C1.
- 다음 액션: 요구사항을 확정하고 구현/검증을 진행한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-ai-first-spec-artifact-release`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-ai-first-spec-artifact-release/open-questions.md`.

## 관계와 이관

framework signed tag identity 확인이 pin 전환의 선행 조건이다. 최종 source를 포함한
출고 tree에서 strict gate를 검증한다. 검증된 결과와 todo 삭제 근거는
`docs/LESSONS_LOG.md`로 이관하며 원격 검증 완료 전에는 publication 완료를 주장하지 않는다.
