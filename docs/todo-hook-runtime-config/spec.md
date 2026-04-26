# Spec: hook-runtime-config

## 배경

- 요청 맥락: 활성 todo가 없어 다음 MVP 온보딩/운영 표면을 검토하던 중 hook runtime 옵션이 env 전용으로 남아 있음을 확인했다.
- 현재 문제/기회: `RUSTORY_ASYNC_UPLOAD=*`, `RUSTORY_AUTO_PRUNE=*` 계열은 실사용 시 셸 세션마다 export해야 하므로, `rr init`로 만든 `config.toml`에 지속 설정으로 남길 수 있어야 한다.

## 계획 스냅샷

- 목표: hook 기록 후 비동기 업로드/자동 prune runtime 옵션을 env 우선, config fallback으로 해석하게 한다.
- 범위: `src/config.rs`, `src/cli.rs`, `docs/quickstart.md`, `docs/hook.md`, `docs/todo-hook-runtime-config/*`, `docs/LESSONS_LOG.md`.
- 검증 명령: `cargo test runtime --workspace`, `cargo test config --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/run-manifest-checks.sh --mode quick --work-id hook-runtime-config`.
- 완료 기준: config 기반 hook runtime 옵션이 doctor/record 경로에서 반영되고, env override와 잘못된 0값 검증이 테스트/문서로 고정된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test runtime --workspace` | async upload/auto prune resolver가 env 우선, config fallback, 기본값을 일관되게 해석하게 한다. |
| C2 | done | codex | `cargo test config --workspace` | `FileConfig`와 `rr init` config 템플릿에 hook runtime 옵션을 포함한다. |
| C3 | done | codex | `HOME="$(mktemp -d)" target/debug/rr doctor --json` smoke + `scripts/run-manifest-checks.sh --mode quick --work-id hook-runtime-config` | quickstart/hook 문서에 config 기반 지속 설정과 env override 관계를 반영한다. |
| C4 | todo | codex | `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` | 전체 회귀 검증 후 todo 마감 증적을 남기고 삭제한다. |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3. `src/config.rs`에 hook runtime config 필드를 추가했고, `src/cli.rs` resolver를 env > config > default 순서로 전환했으며, quickstart/hook 문서에 지속 설정을 반영했다.
- 미완료: C4.
- 다음 액션: 전체 검증은 통과했으므로 구현 커밋을 먼저 푸시한 뒤, 별도 마감 커밋에서 C4를 완료 처리하고 todo 디렉터리를 삭제한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-hook-runtime-config`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-hook-runtime-config/open-questions.md`, `cargo test runtime --workspace`, `cargo test config --workspace`, config doctor JSON smoke(`async_upload.enabled=true`, `auto_prune.enabled=true`), `cargo fmt --all --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/run-manifest-checks.sh --mode quick --work-id hook-runtime-config`.
