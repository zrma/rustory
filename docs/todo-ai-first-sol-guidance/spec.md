# Spec: ai-first-sol-guidance

## 배경

- 요청 맥락: 공식 문서 재검토를 마친 AI-first 1.8.1 지침을 release pin으로 반영한다.
- 현재 문제/기회: cache, effort update, Ultrafast와 logprobs migration 안내를 현재 generated 지침에 전달해야 한다.

## 계획 스냅샷

- 목표: 서명된 v1.8.1 source에 고정하고 기존 제품 동작과 native lifecycle을 유지한다.
- 범위: framework 선언/lock/generated artifact, 검토 기한이 지난 운영 문서의 재검토와 결과 이관.
- 영향: AI 실행 지침과 source provenance. 제품 runtime, API 통합, P2P 동작과 optional profile 선택은 변경하지 않는다.
- 제약: overlay·기존 active work·제품 코드와 검증/출고 gate를 보존한다. history rewrite와 live mutation은 제외한다.
- 검증 명령: `scripts/check.sh --fast`, `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-sol-guidance`.
- 완료 기준: source/standalone/interface/native gate와 same-SHA CI를 확인하고 결과를 LESSONS_LOG로 이관한 뒤 자체 packet을 제거한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-sol-guidance` | release pin, 보존 입력, native 검증 및 결과 이관 |

## 완료/미완료/다음 액션

- 완료: native 시작 packet 생성, 공식 source와 운영 문서 검토.
- 미완료: C1.
- 다음 액션: YAML이 준비된 검증 환경에서 native 시작 검사를 마친 뒤 source를 합성하고 검증한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-ai-first-sol-guidance`, `scripts/check-open-questions-schema.sh --require-closed`.
