# Spec: ai-first-sol-guidance

## 배경

- 저장소가 AI-first `v1.10.1`을 고정하고 있어 OpenAI 지침 기준이 GPT-6.1 Sol에 머물러 있다.

## 계획 스냅샷

- 목표: 서명된 `v1.10.2`를 고정하고 GPT-6.2 Sol 지침을 추적 산출물에 반영한다.
- 범위: AI-first 선언과 생성물, 작업 마감 기록.
- 제약: profile 선택, overlay, 제품 코드와 실행 모델 설정을 보존한다.
- 검증 명령: `python3 .ai-first/check.py`, `scripts/check.sh --fast`, `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-sol-guidance`.
- 완료 기준: 출고 gate 통과, 원격 기본 브랜치와 의도한 커밋 일치, 동일 커밋 CI 성공.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-sol-guidance` | 배포된 소스에서 선언과 생성물을 갱신하고 출고를 확인한다. |

## 완료/미완료/다음 액션

- 완료: 버전 고정과 제품 동작 보존 기준을 정했다.
- 미완료: C1.
- 다음 액션: 서명된 릴리스에서 생성물을 만든 뒤 필수 검증을 실행한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-ai-first-sol-guidance`와 C1 출고 검사 결과.
