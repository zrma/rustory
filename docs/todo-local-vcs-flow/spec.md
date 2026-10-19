# Spec: Local VCS task flow

## 배경

선형 이력과 잔여물 검사를 실제 로컬 agent 시작·통합·마감에 연결한다. 제품 runtime과
기존 WIP·고유 history·release는 유지하고 공통 검사 때문에 추가한 CI binding만 제거한다.

## 계획 스냅샷

- 목표: 시작 기록을 native start/finalize와 연결하고 사본 회수 후 finish로 마감한다.
- 범위: framework pin/generated artifact, native helper adapter와 운영 계약. 자동 삭제와 서버 설정은 제외한다.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id local-vcs-flow`.
- 완료 기준: native gate, 원격 SHA 및 동일 SHA CI, 기존 release와 history 보존, task copy 회수.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id local-vcs-flow` | 공통 로컬 workflow 도입과 native 검증 |
| C2 | todo | codex | `python3 .ai-first/vcs.py finish` | 출고 확인, 지식 이관과 사본 회수 |

## 완료/미완료/다음 액션

- 완료: 시작 기록과 작업 사본 격리, native adapter 구현.
- 미완료: 검증과 출고 및 마감.
- 다음 액션: full gate 뒤 결과를 CHANGE_CONTROL에 이관하고 마감한다.
- 검증 증거: `scripts/check-release-gates.sh --manifest-mode full --work-id local-vcs-flow`, `docs/CHANGE_CONTROL.md`.
