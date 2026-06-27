# Spec: next-maintenance-planning

## 배경

- 요청 맥락: 활성 todo/issue/direct dependency drift가 없을 때 다음 유지보수 후보를 검토하고, 이어갈 work-id를 `docs/todo-*`에 명시한다.
- 현재 문제/기회: Docker relay fallback acceptance는 green으로 복구됐다. Hishtory public sync 장애를 대체하려면 이제 Rustory의 다중 머신 P2P cluster migration 경로와 runbook을 검증해야 한다.

## 계획 스냅샷

- 목표: 현재 repo 상태를 근거 기반으로 정리하고, 다음 실행 후보를 Docker acceptance refresh와 Hishtory migration readiness로 고정한다.
- 범위: 후보 탐색, baseline 검증 증거 기록, 다음 work slice 정의. 코드/스크립트 변경은 범위 밖이다.
- 검증 명령: `scripts/run-manifest-checks.sh --mode quick --work-id next-maintenance-planning`, `env PATH="/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" scripts/check.sh`.
- 완료 기준: 후보 탐색 증거가 기록되고, `docker-acceptance-refresh` 완료 뒤 `hishtory-migration-readiness`로 각 머신 설치/이관/cluster sync runbook을 이어갈 수 있다.

## 후보 검토 결과

- 활성 todo: 없음에서 시작했으며 `docs/todo-next-maintenance-planning`을 새 계획 workspace로 생성했다.
- 원격 상태: `jj git fetch --remote origin` 결과 변경 없음, `main`/`origin/main`은 `297df958`에 정렬돼 있었다.
- GitHub issue: `gh issue list --state open --limit 20` 결과 `[]`.
- 직접 dependency drift: `cargo outdated --workspace --depth 1` 결과 `All dependencies are up to date, yay!`.
- 보안 audit: `cargo audit`는 기존 허용 잔여 `RUSTSEC-2024-0436 paste` warning 1건만 보고했다.
- 기본 로컬 baseline: `env PATH="/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" scripts/check.sh` 통과. `cargo fmt`, `cargo test --workspace` 132 passed, `cargo clippy`, `scripts/smoke_p2p_local.sh` 모두 통과했다.
- 완료된 실행 후보: `docker-acceptance-refresh`. 2026-06-28 수정 후 `bash scripts/acceptance_docker_macos_linux.sh`와 `scripts/check.sh --fast --acceptance`가 통과했다.
  - 1차 실패: `contrib/docker/acceptance/compose.yml`의 컨테이너 내부 `${relay_ip}` shell 변수가 Docker Compose interpolation에 의해 호스트 변수로 먼저 치환되어 linux peer가 tracker에 등록되지 않았다.
  - 2차 실패: linux-peer service가 stale Docker image를 재사용해 현재 `p2p-serve --relay` 코드가 아닌 오래된 바이너리로 실행됐다.
  - 수정 결과: 컨테이너 내부 shell 변수 escape와 linux-peer image freshness를 보장해 macOS host `p2p-sync --push`가 relay fallback으로 pull/push를 완료했다.
- 다음 제품 후보: `hishtory-migration-readiness`. Rustory가 실사용 가능한 상태가 되면 Hishtory가 설치된 각 머신에 Rustory를 병행 설치하고, 기존 shell/hishtory history를 Rustory DB로 seed한 뒤, P2P cluster sync를 켜서 자연스럽게 이관한다.

## 실사용 전환 방향

- 전환 원칙: 한 번에 Hishtory를 끄지 않고, Rustory를 병행 설치해 local append-only 기록과 P2P sync가 안정화될 때까지 dual-run 한다.
- 최소 전환 gate:
  - `scripts/check.sh` green.
  - `scripts/check.sh --acceptance` green.
  - 최소 2개 실제 머신(macOS/Linux 또는 macOS/macOS)에서 tracker/relay + `p2p-sync --watch --push` soak 통과.
  - `rr import`가 기존 shell history seed를 idempotent하게 처리한다는 증거 확보.
  - Hishtory local store/export 형식 확인 후, 필요하면 `rr import hishtory` 또는 별도 변환 스크립트 제공.
- 권장 이관 순서:
  1. central tracker/relay를 self-hosted 환경에 배포하고 relay identity key를 영속화한다.
  2. 첫 번째 기준 머신에서 `rr init`, `rr import`, `rr doctor`, `rr p2p-sync --watch --push`를 적용한다.
  3. 나머지 머신에 Rustory를 병행 설치하고 같은 swarm key/user id로 온보딩한다.
  4. `rr sync-status --with-tracker`와 DB entry count로 수렴 여부를 확인한다.
  5. 충분한 soak 이후 shell hook에서 Hishtory 기록을 비활성화하고 Rustory만 남긴다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `jj status && find docs -maxdepth 2 -type d -name 'todo-*' -print` | 활성 todo와 작업트리 상태 확인 |
| C2 | done | codex | `gh issue list --state open --limit 20 --json number,title,labels,updatedAt,url` | 열린 GitHub issue 후보 확인 |
| C3 | done | codex | `cargo outdated --workspace --depth 1 && cargo audit && env PATH="/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" scripts/check.sh` | dependency/audit/basic gate 상태 확인 |
| C4 | done | codex | `scripts/check.sh --fast --acceptance` | Docker relay fallback acceptance 실패 수정 |
| C5 | todo | codex | `rr import --help && rr sync-status --help` | Hishtory 병행 설치/마이그레이션/cluster sync runbook 정의 |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3, C4.
- 미완료: C5.
- 다음 액션: `docs/todo-hishtory-migration-readiness` 같은 별도 work-id로 설치/이관/검증 runbook과 필요한 import 기능을 정리한다.
- 검증 증거: `scripts/start-work.sh --work-id next-maintenance-planning`, `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" cargo outdated --workspace --depth 1`, `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" cargo audit`, `env PATH="/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" scripts/check.sh`, `docker compose -f contrib/docker/acceptance/compose.yml config`, `bash scripts/acceptance_docker_macos_linux.sh`, `scripts/check.sh --fast --acceptance`, `scripts/run-manifest-checks.sh --mode quick --work-id docker-acceptance-refresh`.
