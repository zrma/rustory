# Spec: ai-first-review-precision

## 배경

AI-first 1.8.2는 기존 Sol compatibility 보완을 유지하고 원래 요구에서 테스트
기대값을 정하는 기준과 같은 원인에 대한 교정 범위를 기존 지침에 명확히 한다.
저장소 사용자가 읽는 두 generated entrypoint에 이 계약을 전달한다.

## 계획 스냅샷

- 목표: signed v1.8.2 source pin과 generated 지침을 정합하게 갱신한다.
- 범위: `.ai-first.toml`, `.ai-first.lock`, `AGENTS.md`, `docs/agent-harness.md`와 native 완료 지식 이관.
- 제약: 기존 Sol baseline, profile, overlay, native gate, active work, 제품 동작과 로컬 이력을 보존한다.
- 제외: 제품 runtime release, live 변경, 새 capability와 실제 agent 행동·속도·비용 효과 평가는 포함하지 않는다.
- 검증 명령: `scripts/check.sh --fast`; `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-review-precision`.
- 완료 기준: source/render/standalone/interface와 native publication gate를 통과하고 원격 동일 SHA CI를 확인한다. 선택적·환경 조건의 skip은 구별한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `scripts/check.sh --fast` | release pin과 generated 지침 및 보존 범위 검증 |
| C2 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-review-precision` | 완료 지식을 이관하고 packet을 정리한 뒤 publication 검증 |

## 완료/미완료/다음 액션

- 완료: signed source와 generated pin의 정합성 확인.
- 미완료: native gate, 결과 이관과 원격 publication/CI.
- 다음 액션: 연결된 native gate를 실행하고 결과·한계를 LESSONS_LOG에 이관한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-ai-first-review-precision`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-ai-first-review-precision/open-questions.md`.
