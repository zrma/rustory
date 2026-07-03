# Spec: release-v1-0-39

## 배경

- 요청 맥락: `v1.0.38` 이후 `rr sync-status --watch`/`rr mesh --watch`의 `to_send` 표기가 `2d`, `1r+9d`처럼 장황하거나 의미가 애매해 daily-driver UI에서 해석 비용이 생겼다.
- 현재 문제/기회: 직전 `main` 변경(`fix: clarify sync watch pending labels`)은 row/delete backlog를 `R`/`D` 접두사와 footer help로 명확히 했으므로, trace 가능한 patch release로 배포해야 한다.

## 계획 스냅샷

- 목표: `1.0.39` 버전으로 Cargo metadata를 올리고, GitHub Release `v1.0.39` daily-driver asset을 발행한 뒤 local MacBook과 k8s 5개 노드에 배포한다.
- 범위: release version bump, 릴리즈 게이트, GitHub Release asset 게시, fleet `rr update` 배포/확인.
- 제외: sync 프로토콜, DB schema, tracker/relay Kubernetes 리소스 변경.
- 검증 명령:
  - `cargo fmt --all --check`
  - `cargo test watch_tui --workspace`
  - `scripts/check.sh --fast`
  - `scripts/run-manifest-checks.sh --mode quick --work-id release-v1-0-39`
  - `scripts/release-version.sh --version v1.0.39 --profile daily-driver --gate quick --work-id release-v1-0-39`
- 완료 기준: `v1.0.39` release가 최신 release로 게시되고, local MacBook 및 `sample-node`, `node0`, `node1`, `node2`, `node3`에서 `rr version`이 `1.0.39`를 보고한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `rg -n 'version = "1.0.39"' Cargo.toml Cargo.lock` | `Cargo.toml`/`Cargo.lock` 버전 bump |
| C2 | done | codex | `cargo fmt --all --check && cargo test watch_tui --workspace && scripts/check.sh --fast` | release 전 회귀 검증 |
| C3 | done | codex | `scripts/run-manifest-checks.sh --mode quick --work-id release-v1-0-39` | repo manifest quick gate |
| C4 | todo | codex | `scripts/release-version.sh --version v1.0.39 --profile daily-driver --gate quick --work-id release-v1-0-39` | GitHub Release `v1.0.39` 발행 |
| C5 | todo | codex | `rr version` on local/k8s fleet | local MacBook + k8s 5개 노드 배포 확인 |

## 완료/미완료/다음 액션

- 완료: C1-C3. Version bump, focused TUI test, fast regression check, manifest quick gate passed.
- 미완료: C4-C5.
- 다음 액션: commit/push, release publish, fleet deploy를 순서대로 진행한다.
- 검증 증거: `cargo fmt --all --check`, `cargo test watch_tui --workspace`, `scripts/check.sh --fast`, `scripts/run-manifest-checks.sh --mode quick --work-id release-v1-0-39`, `scripts/check-todo-readiness.sh docs/todo-release-v1-0-39`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-release-v1-0-39/open-questions.md`.
