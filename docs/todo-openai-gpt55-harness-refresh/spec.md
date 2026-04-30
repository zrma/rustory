# Spec: openai-gpt55-harness-refresh

## 배경

- 요청 맥락: `openai-gpt55-harness-refresh` 작업을 시작하기 전에 계획/검증 기준을 고정한다.
- 현재 문제/기회: 시작 단계를 수동으로 처리하면 계획 스냅샷/게이트 누락이 발생할 수 있다.

## 계획 스냅샷

- 목표: `openai-gpt55-harness-refresh` 작업을 단일 기준(spec)으로 관리하고 안전하게 구현한다.
- 범위: 현재 요청에 포함된 코드/문서/스크립트 변경만 수행한다.
- 검증 명령: `scripts/run-manifest-checks.sh --mode quick --work-id openai-gpt55-harness-refresh`.
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고 검증 명령이 재현 가능하게 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `scripts/run-manifest-checks.sh --mode quick --work-id openai-gpt55-harness-refresh` | GPT-5.5 하네스 기준 문서화와 검증 수행 |

## 완료/미완료/다음 액션

- 완료: 공식 OpenAI developer docs 기준으로 GPT-5.5 하네스 변경 범위를 좁혔다.
- 미완료: C1 최종 검증과 todo 삭제 마감.
- 다음 액션: quick/full 게이트를 통과시킨 뒤 todo를 삭제하고 `finalize-and-push`로 마감한다.
- 검증 증거: `scripts/start-work.sh --work-id openai-gpt55-harness-refresh`, `scripts/check-todo-readiness.sh docs/todo-openai-gpt55-harness-refresh`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-openai-gpt55-harness-refresh/open-questions.md`.
