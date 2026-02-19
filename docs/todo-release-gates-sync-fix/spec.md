# Spec: release-gates-sync-fix

## 배경

- 요청 맥락: `release-gates-sync-fix` 작업을 시작하기 전에 계획/검증 기준을 고정한다.
- 현재 문제/기회: 시작 단계를 수동으로 처리하면 계획 스냅샷/게이트 누락이 발생할 수 있다.

## 계획 스냅샷

- 목표: `release-gates-sync-fix` 작업을 단일 기준(spec)으로 관리하고 안전하게 구현한다.
- 범위: 현재 요청에 포함된 코드/문서/스크립트 변경만 수행한다.
- 검증 명령: `scripts/run-manifest-checks.sh --mode quick --work-id release-gates-sync-fix`.
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고 검증 명령이 재현 가능하게 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `scripts/run-manifest-checks.sh --mode quick --work-id release-gates-sync-fix` | 요청 구현과 검증 수행 |

## 완료/미완료/다음 액션

- 완료: 없음.
- 미완료: C1.
- 다음 액션: 요구사항을 확정하고 구현/검증을 진행한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-release-gates-sync-fix`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-release-gates-sync-fix/open-questions.md`.
