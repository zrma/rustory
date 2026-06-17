# Spec: next-maintenance-planning

## 배경

- 요청 맥락: 활성 todo/issue/direct dependency drift가 없을 때 다음 유지보수 후보를 검토하고, 이어갈 work-id를 `docs/todo-*`에 명시한다.
- 현재 문제/기회: 로컬 기본 게이트는 green이지만 Docker relay fallback acceptance는 현재 Docker daemon 부재로 재검증하지 못했다.

## 계획 스냅샷

- 목표: 현재 repo 상태를 근거 기반으로 정리하고, 다음 실행 후보를 Docker acceptance refresh로 고정한다.
- 범위: read-only 후보 탐색, baseline 검증 증거 기록, 다음 work slice 정의. 코드/스크립트 변경은 범위 밖이다.
- 검증 명령: `scripts/run-manifest-checks.sh --mode quick --work-id next-maintenance-planning`, `env PATH="/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" scripts/check.sh`.
- 완료 기준: 후보 탐색 증거가 기록되고, Docker daemon이 사용 가능해지면 `scripts/check.sh --acceptance` 또는 `bash scripts/acceptance_docker_macos_linux.sh`로 다음 slice를 실행할 수 있다.

## 후보 검토 결과

- 활성 todo: 없음에서 시작했으며 `docs/todo-next-maintenance-planning`을 새 계획 workspace로 생성했다.
- 원격 상태: `jj git fetch --remote origin` 결과 변경 없음, `main`/`origin/main`은 `297df958`에 정렬돼 있었다.
- GitHub issue: `gh issue list --state open --limit 20` 결과 `[]`.
- 직접 dependency drift: `cargo outdated --workspace --depth 1` 결과 `All dependencies are up to date, yay!`.
- 보안 audit: `cargo audit`는 기존 허용 잔여 `RUSTSEC-2024-0436 paste` warning 1건만 보고했다.
- 기본 로컬 baseline: `env PATH="/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" scripts/check.sh` 통과. `cargo fmt`, `cargo test --workspace` 132 passed, `cargo clippy`, `scripts/smoke_p2p_local.sh` 모두 통과했다.
- 다음 실행 후보: `docker-acceptance-refresh`. 현재 `docker ps`는 `Cannot connect to the Docker daemon at unix:///Users/user/.docker/run/docker.sock`로 실패하므로 Docker daemon 실행 후 진행한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `jj status && find docs -maxdepth 2 -type d -name 'todo-*' -print` | 활성 todo와 작업트리 상태 확인 |
| C2 | done | codex | `gh issue list --state open --limit 20 --json number,title,labels,updatedAt,url` | 열린 GitHub issue 후보 확인 |
| C3 | done | codex | `cargo outdated --workspace --depth 1 && cargo audit && env PATH="/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" scripts/check.sh` | dependency/audit/basic gate 상태 확인 |
| C4 | todo | codex | `scripts/check.sh --acceptance` | Docker daemon이 사용 가능할 때 relay fallback acceptance refresh 실행 |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3.
- 미완료: C4.
- 다음 액션: Docker daemon을 실행할 수 있는 상태에서 `scripts/check.sh --acceptance`를 돌리고, 실패하면 해당 실패를 `docs/todo-docker-acceptance-refresh` 같은 별도 work-id로 구현/검증/마감한다.
- 검증 증거: `scripts/start-work.sh --work-id next-maintenance-planning`, `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" cargo outdated --workspace --depth 1`, `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" cargo audit`, `env PATH="/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" scripts/check.sh`, `docker ps`.
