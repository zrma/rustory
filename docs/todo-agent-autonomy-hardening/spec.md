# Spec: agent-autonomy-hardening

## 배경

- 요청 맥락: LLM 에이전트가 사람 판단이 필요한 경우에만 에스컬레이션하고, 나머지는 repo-local 게이트로 자율 수행할 수 있는지 점검했다.
- 현재 문제/기회: macOS 기본 `/bin/bash` 3.2에서 일부 문서/jj 게이트가 실패했고, rewrite 전 stale `jj` head가 branch hygiene을 막고 있었다.

## 계획 스냅샷

- 목표: 기본 macOS shell 환경에서도 agent release/push 게이트가 실행 가능하도록 보강하고, stale `jj` head를 정리해 자율 출고 경로를 복구한다.
- 범위: `scripts/check-doc-links.sh`, `scripts/check-doc-index.sh`, `scripts/check-jj-conflicts.sh`의 Bash 3.2 호환성 및 로컬 `jj` head 위생 정리.
- 검증 명령: `scripts/check-doc-links.sh`, `scripts/check-doc-index.sh`, `scripts/check-jj-conflicts.sh`, `scripts/check-branch-hygiene.sh`, `scripts/run-manifest-checks.sh --mode quick --work-id agent-autonomy-hardening`.
- 완료 기준: 위 게이트가 기본 shell 경로에서 통과하고, 완료 todo 삭제 증거가 `docs/LESSONS_LOG.md`에 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `scripts/run-manifest-checks.sh --mode quick --work-id agent-autonomy-hardening` | Bash 3.2 호환성 보강 및 stale `jj` head 정리 |

## 완료/미완료/다음 액션

- 완료: Bash 3.2 호환성 보강, stale `jj` head 정리.
- 미완료: todo 마감 커밋.
- 다음 액션: todo를 삭제하고 `docs/LESSONS_LOG.md`에 `todo-agent-autonomy-hardening` 마감 증적을 남긴다.
- 검증 증거: `scripts/check-doc-links.sh`, `scripts/check-doc-index.sh`, `scripts/check-jj-conflicts.sh`, `scripts/check-branch-hygiene.sh`.
