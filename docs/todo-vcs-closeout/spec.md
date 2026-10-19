# Spec: VCS closeout adoption

## 배경

선형 통합과 임시 작업 사본 회수의 공통 검사 적용이 필요하다. 영향을 받는 경로는 agent bootstrap, standalone gate와 CI이며 제품 runtime과 배포는 제외한다.

## 계획 스냅샷

- 목표: pin한 공통 VCS 검사를 native gate와 CI에서 소비하고 작업 종료 상태를 검증한다.
- 범위: framework pin, generated artifact, CI full history와 실제 후보 binding, 종료 문서.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id vcs-closeout`.
- 완료 기준: native gate와 동일 SHA CI 성공, WIP와 기존 release 보존, task-owned 사본 회수.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id vcs-closeout` | 공통 검사 적용과 native 검증 |
| C2 | todo | codex | `python3 .ai-first/vcs.py --closeout` | 원격 검증과 owning artifact 이관, 사본 회수 |

## 완료/미완료/다음 액션

- 완료: signed source pin, generated standalone과 full native release gate.
- 미완료: 원격 CI와 local closeout.
- 다음 액션: 종료 계약을 이관하고 동일 SHA CI와 task-owned 사본 회수를 검증한다. 결과 계약은 `docs/CHANGE_CONTROL.md`와 generated harness로 이관한다.
