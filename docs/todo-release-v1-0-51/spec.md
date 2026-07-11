# Spec: release-v1-0-51

## 배경

- 요청 맥락: relay listener 재시도 tight loop를 백오프로 수정한 최종 patch release를 배포한다.
- 현재 문제/기회: Mac canary에서 실제 stderr 증가율을 재검증해야 전체 fleet 완료로 닫을 수 있다.

## 계획 스냅샷

- 목표: `v1.0.51`을 게시하고 Mac, 내부 5개 노드, 외부 x86에 배포한 뒤 runtime log와 cluster 상태를 검증한다.
- 범위: package version, source/tag/assets, worker-first rollout, Mac log growth, fleet와 cluster 상태.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-51`.
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고 검증 명령이 재현 가능하게 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `rg 'version = "1.0.51"' Cargo.toml Cargo.lock` | package와 lock version 일치 |
| C2 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-51` | 전체 출고 게이트 통과 |
| C3 | todo | codex | `gh run list --branch main` | 원격 CI 3종 성공 |
| C4 | todo | codex | `scripts/check-linux-glibc-baseline.sh dist/release-v1.0.51/rr-x86_64-unknown-linux-gnu 2.17` | 공개 자산과 GLIBC 검증 |
| C5 | todo | codex | `rr sync-status --json --with-tracker` | fleet, Mac runtime log, cluster 검증 |

## 완료/미완료/다음 액션

- 완료: C1.
- 미완료: C2-C5.
- 다음 액션: full gate, CI, 공개 자산, canary와 fleet 순차 검증.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-release-v1-0-51`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-release-v1-0-51/open-questions.md`.
