# Spec: release-v1-0-46

## 배경

- 요청 맥락: peer `rr` 버전 표시 변경을 충분히 재검토한 뒤 문제가 없으면 `v1.0.46` release를 발행하고 로컬 MacBook과 k8s 5개 노드에 배포한다.
- 현재 문제/기회: 운영 fleet의 업데이트 대상을 쉽게 식별하려면 source 변경뿐 아니라 release asset, updater, daemon restart, 실제 fleet 표시까지 같은 작업에서 검증해야 한다.

## 계획 스냅샷

- 목표: peer-version 변경을 안전하게 `v1.0.46`으로 출고하고 MacBook 및 k8s 5개 노드의 실행 버전과 sync/cluster health를 검증한다.
- 범위: 현재 peer-version change 검수, `main` push, 1.0.46 version bump, daily-driver GitHub Release, 로컬 MacBook, `node0..3` worker-first, `sample-node` control-plane-last 배포, 최종 문서화와 원격 상태 확인.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-46`, `scripts/release-version.sh --version v1.0.46 --profile daily-driver --gate full --work-id release-v1-0-46`, 노드별 `rr version`/`rr doctor`/`rr sync-status --json --with-tracker`, cluster healthcheck.
- 완료 기준: C1-C8이 모두 `done`이고 GitHub Release asset/checksum/GLIBC gate, 로컬 및 5개 노드 `1.0.46`, daemon/tracker/sync 상태, Kubernetes Ready/ArgoCD health, 최종 `main@origin`을 증거로 남긴다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-46` | peer-version 변경 재검토와 strict/full 회귀 검증 |
| C2 | in_progress | codex | `git ls-remote --heads origin main` | feature change를 attribution 포함 `main`에 push하고 SHA 확인 |
| C3 | todo | codex | `rr version` | Cargo version 1.0.46 bump를 검증하고 `main`에 push |
| C4 | todo | codex | `gh release view v1.0.46 --repo zrma/rustory` | daily-driver asset/checksum/GLIBC baseline을 검증하고 GitHub Release 게시 |
| C5 | todo | codex | `rr version && rr doctor && rr sync-status --json --with-tracker` | 로컬 MacBook을 v1.0.46으로 업데이트하고 user-facing 상태 확인 |
| C6 | todo | codex | `ssh ts-miniN '~/.local/bin/rr version'` | `node0..3` worker 4개를 순차 업데이트하고 각 노드 상태 확인 |
| C7 | todo | codex | `ssh ts-sample-node '~/.local/bin/rr version'` | control-plane 노드를 마지막에 업데이트하고 전체 cluster health 확인 |
| C8 | todo | codex | `gh run list --repo zrma/rustory` | 배포 증거 문서화, todo 삭제 마감, 최종 `main` push와 원격 CI 확인 |

## 완료/미완료/다음 액션

- 완료: C1. peer version 전달/저장/표시 경로를 재검토했고, 실사용 점검에서 `hostname=unknown`이 중복 호스트로 오탐되던 결함을 발견해 OS hostname fallback과 unknown sentinel 제외 처리를 추가했다.
- 미완료: C2-C8.
- 다음 액션: 검증된 feature change를 attribution 포함 `main`에 push한다.
- 검증 증거: full release gate 통과(워크스페이스 304 tests, clippy, installer, local P2P smoke), 새 debug binary의 `sync-status --json --with-tracker`에서 peer version과 빈 warnings 확인, `mesh --watch` Flow Lanes에서 `[1.0.45]` badge 확인, 배포 전 Kubernetes 5개 노드 Ready 및 ArgoCD 19개 앱 Synced/Healthy 확인.
