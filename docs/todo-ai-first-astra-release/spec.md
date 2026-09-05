# Spec: ai-first-astra-release

## 배경

- AI-first 1.4.0 source가 signed annotated release로 발행되어 소비 pin을 확정한다.

## 계획 스냅샷

- 목표: Astra profile의 release pin과 출고 검증을 준비한다.
- 범위: 선언, lock, interface assertion과 이 작업의 lifecycle 기록.
- 검증 명령: `scripts/check-agent-harness-interface.sh`, `scripts/check-release-gates.sh --manifest-mode full`.
- 완료 기준: 동일 framework source의 release pin과 native/publication gate를 확인한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `scripts/check-release-gates.sh --manifest-mode full` | release pin 및 출고 준비 |

## 완료/미완료/다음 액션

- 완료: immutable commit 기반 local adoption과 native 검증.
- 미완료: C1.
- 다음 액션: release pin을 합성하고 local packet을 정식 기록으로 이관한 뒤 전체 출고 gate를 실행한다.
- 검증 증거: `scripts/check-agent-harness-interface.sh`, `scripts/check-release-gates.sh --manifest-mode full`.
