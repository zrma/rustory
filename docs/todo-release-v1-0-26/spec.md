# Spec: release-v1-0-26

## 배경

- 요청 맥락: `rr mesh --watch` stable ordering 변경이 `main`에는 반영됐지만, 공개된 최신 GitHub Release `v1.0.25`는 이전 커밋을 가리킨다.
- 현재 문제/기회: daily-driver 머신이 `rr update`로 받을 수 있게 새 patch release와 fleet 배포 증거가 필요하다.

## 계획 스냅샷

- 목표: `1.0.26`으로 version bump 후 `v1.0.26` GitHub Release asset을 발행하고, local MacBook과 k8s 5개 노드에 배포한다.
- 범위: version metadata, release todo, release asset 발행, fleet `rr update`/daemon restart/상태 확인.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-26`, `scripts/release-version.sh --profile daily-driver --gate full --work-id release-v1-0-26`, fleet `rr version`/`rr sync-status --json --with-tracker`.
- 완료 기준: `origin/main`이 version bump commit을 가리키고, `v1.0.26` release asset이 게시되며, local MacBook + `sample-node`, `node0`, `node1`, `node2`, `node3`가 `rr version = 1.0.26`으로 확인된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `PATH=/Users/user/.rustup/toolchains/1.95.0-aarch64-apple-darwin/bin:$PATH scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-26` | version bump와 release gate 검증 |
| C2 | todo | codex | `PATH=/Users/user/.rustup/toolchains/1.95.0-aarch64-apple-darwin/bin:$PATH scripts/release-version.sh --profile daily-driver --gate full --work-id release-v1-0-26` | `v1.0.26` GitHub Release asset 발행 |
| C3 | todo | codex | `rr version`; `rr sync-status --json --with-tracker` | local MacBook + k8s 5개 노드 배포 및 상태 확인 |

## 완료/미완료/다음 액션

- 완료: C1. `PATH=/Users/user/.rustup/toolchains/1.95.0-aarch64-apple-darwin/bin:$PATH scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-26` 통과.
- 미완료: C2, C3.
- 다음 액션: version bump commit/push 후 release asset을 발행한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-release-v1-0-26`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-release-v1-0-26/open-questions.md`, `PATH=/Users/user/.rustup/toolchains/1.95.0-aarch64-apple-darwin/bin:$PATH scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-26`.
