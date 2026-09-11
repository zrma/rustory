# Spec: ai-first-startup-routing

## 배경

- 현재 문제: 초기 문서 순서가 요청 종류와 무관한 전체 읽기·검사 절차로 해석될 수 있다.
- 원하는 결과: AI-first v1.7.1을 pin하고 Rustory의 시작 안내를 요청별 경로에 맞춰, 설명·조사의 불필요한 절차와 변경·출고의 필수 검증을 구분한다.

## 계획 스냅샷

- 목표: 요청별 탐색 지침을 생성 결과와 native 안내에 일관되게 적용한다.
- 범위: framework 선언·lock·generated 문서, HANDOFF와 EXECUTION_LOOP의 시작 조건, 완료 지식 이관.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-startup-routing`.
- 완료 기준: C1~C3의 원래 조건과 실제 diff·검증을 대조하고 원격 pin 및 동일 SHA CI까지 확인한다.

## 영향과 제약

- 영향: 유지보수 agent의 초기 탐색·검증 라우팅. CLI·P2P·installer 제품 동작은 변경하지 않는다.
- 유지: 기존 profile·overlay, native readiness·strict safe-push gate, 권한·공개 경계와 기존 작업.
- 제외: 새 runtime·위임·hook·계측 기능, 앱 release·fleet deployment, 실제 token·시간 절감 주장.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `python3 .ai-first/check.py` | signed v1.7.1 source pin과 generated artifact 정합성 |
| C2 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-startup-routing` | native 검증과 설명·조사 및 변경·출고 라우팅의 의미 대조 |
| C3 | todo | codex | `scripts/check-lessons-log-enforcement.sh --worktree` | 원래 기준과 검증 한계를 LESSONS_LOG 및 소유 문서로 이관하고 packet 정리 |

## 완료/미완료/다음 액션

- 완료: native 작업 초기화.
- 미완료: C1, C2, C3.
- 다음 액션: release pin과 요청별 문서 경로를 반영하고 full native gate를 검증한다.
- 검증 증거: `scripts/start-work.sh --work-id ai-first-startup-routing`.
