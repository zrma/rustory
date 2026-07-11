# Spec: release-v1-0-49

## 배경

- 요청 맥락: optional Atuin importer와 SQLite 입력 경계 보완을 검증된 patch release로 배포한다.
- 현재 문제/기회: 공개 asset, GLIBC baseline, updater 선검증, fleet와 cluster 상태를 하나의 출고 증거로 닫아야 한다.

## 계획 스냅샷

- 목표: `v1.0.49`를 macOS arm64/Linux x86_64로 게시하고 Mac canary, k8s 5개 노드, 외부 x86 피어에 안전하게 배포한다.
- 범위: package version, release source/tag/assets, updater rollout, daemon/tracker/cluster 후속 검증.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-49`.
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고 검증 명령이 재현 가능하게 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `rg 'version = "1.0.49"' Cargo.toml Cargo.lock` | package/lock version 일치 |
| C2 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-49` | 전체 출고 게이트 통과 |
| C3 | todo | codex | `gh run list --branch main` | release source 원격 CI 3종 성공 |
| C4 | todo | codex | `scripts/check-linux-glibc-baseline.sh dist/release-v1.0.49/rr-x86_64-unknown-linux-gnu 2.17` | 공개 자산 checksum 및 Linux GLIBC baseline 검증 |
| C5 | todo | codex | `rr version && rr sync-status --json` | Mac과 5개 내부 노드 및 외부 x86 rollout 검증 |
| C6 | todo | codex | `kubectl get nodes && kubectl get pods -A` | Kubernetes 5 Ready, ArgoCD, Pod 상태 검증 |

## 완료/미완료/다음 액션

- 완료: C1.
- 미완료: C2-C6.
- 다음 액션: 전체 게이트와 원격 CI를 통과한 source revision을 release/tag로 게시하고 순차 배포한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-release-v1-0-49`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-release-v1-0-49/open-questions.md`.
