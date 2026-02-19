# Lessons Log

- Audience: Rustory 유지보수자, LLM 에이전트
- Owner: Rustory
- Last Verified: 2026-02-19

반복 가능한 실수 방지 규칙을 누적하는 로그다. 작성 규칙은 `docs/IMPROVEMENT_LOOP.md`를 따른다.

참고: 표에 남는 `docs/todo-*` 경로는 당시 작업 증적 식별자이며, 작업 종료 후 디렉터리가 삭제되어 현재는 존재하지 않을 수 있다.
현재 실행 경로는 `Applied Change`/`Verification`에 적힌 현행 문서·스크립트 경로를 우선한다.

## Recent Entries (max 50)

| Date | Trigger | Lesson | Applied Change | Verification |
| --- | --- | --- | --- | --- |
| 2026-02-19 | 참조 저장소 대비 문서/스크립트 parity 동기화 중 경로 구조가 바뀌면서(`scripts/lib/todo-workspace.sh`) `.gitignore`의 `lib/` 규칙으로 helper 파일이 추적되지 않아 실행 환경 불일치 위험이 확인됨 (`todo-diff-sync-parity`) | 공통 helper를 서브경로로 분리할 때는 ignore 규칙과 추적 상태를 함께 갱신하고, 구 경로는 shim으로 유지해 운영 스크립트의 경로 전환을 단계적으로 수행해야 배포/협업 환경에서 깨짐을 막을 수 있다. | `scripts/*`의 todo helper source 경로를 `scripts/lib/todo-workspace.sh`로 통일하고, `scripts/todo-workspace.sh`를 호환 shim으로 전환, `.gitignore` 예외(`!scripts/lib/todo-workspace.sh`) 추가, 스모크 fixture 경로(`scripts/lib`) 보정 및 공통 문구/게이트 리팩터링 동기화 | `scripts/check-script-smoke.sh --work-id diff-sync-parity` + `scripts/run-manifest-checks.sh --mode quick --work-id diff-sync-parity` + `scripts/check-todo-closure.sh` |
| 2026-02-19 | 참조 저장소와 운영 문서를 동기화하는 과정에서 `rustory`는 `AGENTS/HANDOFF/EXECUTION_LOOP/CHANGE_CONTROL` 간 규칙 중복이 누적되어 업데이트 비용이 커졌고 `todo-doc-autonomy-parity-refresh` 마감 증적도 필요했음 | 자율 운영 문서는 라우팅 문서와 규칙 소유 문서를 분리해 단일 기준으로 유지해야 drift를 줄일 수 있으며, 완료된 todo(`todo-doc-autonomy-parity-refresh`)는 교훈 로그 식별자를 남기고 즉시 정리해야 게이트 재실패를 방지할 수 있다. | `AGENTS.md`, `docs/{HANDOFF,EXECUTION_LOOP,CHANGE_CONTROL,IMPROVEMENT_LOOP,README,REPO_MANIFEST.yaml}`를 단일 소유 원칙 중심으로 정리하고 문서/manifest 게이트를 재검증 | `scripts/check-doc-links.sh` + `scripts/check-doc-index.sh` + `scripts/check-readme-policy.sh` + `scripts/check-doc-last-verified.sh` + `scripts/check-manifest-entrypoints.sh` + `scripts/run-manifest-checks.sh --mode quick --work-id doc-autonomy-parity-refresh` |
| 2026-02-19 | docs/REPO_MANIFEST.yaml 및 docs/EXECUTION_LOOP.md 기준 점검에서 `rustory`는 문서 인덱스/README 구조/push range 교훈 로그 강제가 일부 누락되어 있었고, 완료된 `docs/todo-llm-agent-autonomy-parity/`가 잔존해 release gate를 불안정하게 만들었음 | 자율 운영 저장소는 `문서 인덱스 -> README 정책 -> push range 교훈 로그`를 스크립트/훅으로 연결하고, 완료된 todo(`todo-llm-agent-autonomy-parity`)는 즉시 정리해야 drift와 게이트 재실패를 줄일 수 있다. | `scripts/check-{doc-index,readme-policy,lessons-log-range}.sh` 추가, `scripts/check-lessons-log-enforcement.sh --worktree` 확장, `scripts/{check-push-gates,check-release-gates,jj-git-push-safe,check-script-smoke}.sh` 및 `lefthook.yml` 연동, `README`/운영 문서/manifest 동기화, `docs/todo-llm-agent-autonomy-parity/` 정리 | `scripts/check-doc-index.sh` + `scripts/check-readme-policy.sh` + `scripts/check-release-gates.sh --manifest-mode full --work-id llm-agent-autonomy-parity` + `cargo fmt --all --check` + `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` |
| 2026-02-18 | release gate 실행 중 `check-prod-preflight.sh`가 macOS 기본 bash(3.x)에서 empty array `nounset`로 실패 | 운영 스크립트는 bash4 전용 문법(`readarray`/배열 empty 확장)에 의존하지 말고, bash3 호환 루프로 작성해야 에이전트 자동 실행이 환경 차이로 중단되지 않는다. | `scripts/check-prod-preflight.sh`를 배열 누적 방식에서 `while read` 순회 방식으로 변경하고, 동일 시점에 게이트 계열 스크립트의 bash3 호환성을 점검/보정 | `scripts/check-release-gates.sh --manifest-mode full --work-id llm-agent-autonomy-parity` |
| 2026-02-18 | 참조 운영 모델과 비교했을 때 `rustory`에는 문서 책임 분리/게이트 자동화/출고 절차의 실행형 경로가 부족했음 | 에이전트 자율 운영은 문서 선언만으로 충분하지 않고, `start-work -> readiness -> release-gates -> finalize`의 실행형 루프가 저장소에 내장되어야 안정적으로 동작한다. | 운영 문서 프레임(`HANDOFF`, `EXECUTION_LOOP`, `CHANGE_CONTROL`, `OPERATING_MODEL` 등)과 게이트 스크립트(`scripts/check-*`, `scripts/start-work.sh`, `scripts/finalize-and-push.sh`) 및 CI 연동을 추가 | `scripts/check-release-gates.sh --manifest-mode full` + `cargo fmt --all --check` + `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` |
