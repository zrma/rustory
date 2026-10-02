# Spec: ai-first-sol-adoption

## 배경

- 요청 맥락: OpenAI agent guidance baseline을 GPT-6.1 Sol로 갱신한다.
- 현재 문제/기회: 기존 provider profile은 GPT-6 Astra를 baseline으로 안내한다.

## 계획 스냅샷

- 목표: AI-first 1.8.0의 고정 release source와 생성된 Sol 지침을 적용한다.
- 범위: 선언/lock/generated artifact와 native lifecycle 기록. 제품 runtime은 유지한다.
- 검증 명령: `scripts/check.sh --fast`, `scripts/run-manifest-checks.sh --mode quick --work-id ai-first-sol-adoption`.
- 완료 기준: C1 검증, source/overlay 보존과 owning artifact 이관 후 local change로 닫는다.

## 의도와 제약

- 영향: agent bootstrap/harness와 framework source pin.
- 제약: 기존 overlay, native gate, 제품 동작과 진행 중 작업을 보존한다.
- 제외: application runtime model/API/P2P 동작 변경, Sol 행동 eval과 미승인 publication.
- local tag와 source는 고정하고 원격 발행 여부를 local 성공과 구별한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `scripts/check.sh --fast` | release pin과 Sol guidance 적용 및 local 검증 |

## 완료/미완료/다음 액션

- 완료: source 합성과 standalone 검사.
- 미완료: C1 native 검사와 결과 이관.
- 다음 액션: fast native gate 및 quick manifest를 확인하고 완료 결과를 이관한다.
- 검증 증거: `python3 .ai-first/check.py`, `scripts/check-todo-readiness.sh docs/todo-ai-first-sol-adoption`.
