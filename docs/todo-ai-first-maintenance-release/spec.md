# Spec: ai-first-maintenance-release

## 배경

- 요청 맥락: AI-first renderer 유지보수 patch의 signed source를 소비 pin에 반영한다.
- 현재 문제/기회: 기존 pin은 v1.7.2이므로 v1.7.3 source와 generated 정합성을 갱신한다.

## 계획 스냅샷

- 목표: signed v1.7.3 pin과 generated artifact를 갱신하고 native 출고 검증을 통과한다.
- 범위: .ai-first.toml, .ai-first.lock, AGENTS.md, docs/agent-harness.md와 작업 종료 기록.
- 영향: repository의 agent bootstrap과 framework source identity.
- 제약: 기존 profile·overlay·native gate·제품 동작·active work를 보존한다.
- 제외: 제품 version/release, runtime 변경과 검증 생략.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-maintenance-release`.
- 완료 기준: standalone/interface·full native gate, 공개 경계, remote equality와 동일 SHA CI 확인.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-maintenance-release` | signed source 적용, 보호 파일 보존, native 검증과 결과 이관 |

## 완료/미완료/다음 액션

- 완료: native 작업 초기화, pin/generated 갱신, 보호 파일 보존과 full native gate.
- 미완료: C1.
- 다음 액션: 결과를 이관하고 packet을 정리한 뒤 strict safe-push와 동일 SHA CI를 확인한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-ai-first-maintenance-release`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-ai-first-maintenance-release/open-questions.md`.
