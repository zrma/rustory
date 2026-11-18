# Spec: ai-first-update-verification

## 배경

- 현재 AI-first 고정 버전은 v1.10.2다. 갱신 검증 절차가 보강된 서명 릴리스로 고정 버전을 올린다.

## 계획 스냅샷

- 목표: v1.10.3 source tag를 검증한 뒤 선언과 추적 생성물을 갱신한다.
- 범위: framework pin, 생성물과 native 작업 마감 기록.
- 제약: profile, overlay, 제품 코드와 실행 설정을 보존한다.
- 검증 명령: `python3 .ai-first/check.py`, `scripts/check.sh --fast`, `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-update-verification`.
- 완료 기준: 필수 출고 gate, 원격 main SHA 일치와 해당 SHA의 CI 성공.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-update-verification` | 서명 릴리스로 pin과 생성물을 갱신하고 원격 반영을 검증한다. |

## 완료/미완료/다음 액션

- 완료: framework 고정과 제품 변경 범위를 확정했다.
- 미완료: C1.
- 다음 액션: 고정한 원본에서 생성물을 갱신하고 출고 gate를 실행한다.
- 검증 증거: readiness와 C1 검증 결과.
