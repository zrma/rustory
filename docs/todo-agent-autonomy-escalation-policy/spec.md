# Spec: agent-autonomy-escalation-policy

## 배경

- 요청 맥락: 사용자는 에이전트가 명시적 에스컬레이션 또는 사용자 판단이 필요한 경우에만 호출하고, 나머지는 자율적으로 목표 달성까지 진행하기를 원한다.
- 현재 문제/기회: 기존 문서 구조는 이미 자율 실행에 가깝지만, 최상위 agent 지침과 운영 문서에 해당 원칙이 직접 문장으로 고정되어 있지 않다.

## 계획 스냅샷

- 목표: 에스컬레이션 외 자율 진행 원칙을 agent 진입 문서와 운영/에스컬레이션 문서에 명시한다.
- 범위: `AGENTS.md`, `docs/OPERATING_MODEL.md`, `docs/ESCALATION_POLICY.md`, todo 마감 증적 문서만 수정한다.
- 검증 명령: `scripts/run-manifest-checks.sh --mode quick --work-id agent-autonomy-escalation-policy`.
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고 검증 명령이 재현 가능하게 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `scripts/run-manifest-checks.sh --mode quick --work-id agent-autonomy-escalation-policy` | 에스컬레이션 외 자율 진행 원칙을 문서에 명시하고 검증 수행 |

## 완료/미완료/다음 액션

- 완료: 작업 범위와 검증 기준을 구체화했다.
- 미완료: C1 문서 반영 및 검증.
- 다음 액션: 문서 변경 후 quick manifest 검증을 실행하고 todo 마감 증적을 남긴다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-agent-autonomy-escalation-policy`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-agent-autonomy-escalation-policy/open-questions.md`.
