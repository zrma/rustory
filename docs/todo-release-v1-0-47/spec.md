# Spec: release-v1-0-47

## 배경

- 요청 맥락: P0 privacy boundary 보완을 `v1.0.47`로 공개하고 local MacBook 및 k8s 5개 노드에 배포한다.
- 현재 문제/기회: source push, public release asset, updater 검증, worker-first fleet rollout을 하나의 증거 체인으로 닫아야 실제 사용자 동작과 배포 상태를 보장할 수 있다.

## 계획 스냅샷

- 목표: `v1.0.47` daily-driver release를 게시하고 local MacBook과 `node0..3`, `sample-node`에 배포해 version/daemon/tracker/sync 상태를 검증한다.
- 범위: `Cargo.toml`/`Cargo.lock` version bump, public `main`/GitHub Release, macOS arm64 및 Linux x86_64 asset/checksum/GLIBC baseline, local 및 5-node `rr update`, cluster health, release lessons closure.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-47`, `scripts/release-version.sh --version v1.0.47 --profile daily-driver --gate none --work-id release-v1-0-47`, `rr version`, `rr sync-status --json --with-tracker`.
- 완료 기준: C1-C6가 모두 `done`이고 remote main/tag/release target이 일치하며, public asset checksum과 Linux `GLIBC_2.17` baseline이 확인되고 local/5-node가 `1.0.47`로 수렴한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `rg -n '^version = "1.0.47"' Cargo.toml Cargo.lock` | package와 lockfile version을 `1.0.47`로 갱신 |
| C2 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-47` | full source/release gate 통과 |
| C3 | in_progress | codex | `git ls-remote --heads origin main && gh run list --commit <sha>` | release change를 public `main`에 push하고 remote/CI 확인 |
| C4 | todo | codex | `gh release view v1.0.47 --json tagName,targetCommitish,assets` | daily-driver assets/checksum 게시 및 Linux GLIBC baseline 확인 |
| C5 | todo | codex | `rr version && rr sync-status --json --with-tracker` | local MacBook 및 k8s 5개 노드 worker-first update/daemon/sync 검증 |
| C6 | todo | codex | `scripts/check-todo-closure.sh && jj status` | cluster health/remaining risk를 확인하고 lessons log로 todo 마감 |

## 완료/미완료/다음 액션

- 완료: C1-C2. version bump와 full source/release gate를 완료했다.
- 미완료: C3-C6.
- 다음 액션: release change를 push하고 public asset 게시 후 fleet rollout을 진행한다.
- 검증 증거: `v1.0.47` full gate에서 309 tests, clippy `-D warnings`, installer tests, local P2P smoke가 통과했다. P0 구현 stack은 public boundary `mode=all`과 동일 full gate를 통과해 `origin/main`에 반영됐다.
