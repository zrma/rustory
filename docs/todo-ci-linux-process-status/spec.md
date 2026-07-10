# Spec: ci-linux-process-status

## 배경

- 요청 맥락: `v1.0.44` 출고 전 최신 `main`의 GitHub CI/Release Gates 실패를 해소한다.
- 현재 문제/기회: macOS에서는 컴파일되지 않는 Linux 전용 `stop_systemd_user_daemon()`이 `process_status()`를 호출하지만 helper가 macOS에만 정의되어 Linux Rust 1.95 build가 `E0425`로 실패한다.

## 계획 스냅샷

- 목표: `process_status()`의 cfg를 실제 macOS/Linux 호출 범위와 일치시키고 최신 `main`의 GitHub CI/Release Gates를 green으로 복구한다.
- 범위: `src/self_update.rs` platform cfg, 회귀 교훈 문서, 가장 가까운 로컬 검증과 원격 GitHub Actions 확인.
- 검증 명령: `cargo fmt --all --check`, `cargo test self_update --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `gh run list --commit <sha>`.
- 완료 기준: macOS 로컬 검증이 통과하고 새 `main` commit의 Docs Integrity, CI, Release Gates가 모두 success다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `gh run view 29056085785 --log-failed`, `gh run view 29056085701 --log-failed` | Linux `E0425 process_status not found` 실패 근거 고정 |
| C2 | done | codex | `cargo fmt --all --check && cargo test self_update --workspace && cargo clippy --workspace --all-targets -- -D warnings` | helper cfg를 macOS/Linux 호출 범위와 일치시키고 로컬 회귀 검증 |
| C3 | todo | codex | `gh run list --commit <sha>` | 수정 commit의 Docs Integrity, CI, Release Gates 전체 success 확인 |

## 완료/미완료/다음 액션

- 완료: C1-C2. 원격 실패를 재현하고 helper cfg를 macOS/Linux로 맞췄다. macOS self-update 테스트 16개와 Clippy가 통과했고, node0 native Linux release build도 성공했다.
- 미완료: C3 원격 전체 check green 확인.
- 다음 액션: main에 push한 뒤 Docs Integrity, CI, Release Gates의 최종 conclusion을 확인한다.
- 검증 증거: `cargo fmt --all --check`, `cargo test self_update --workspace` (16 passed), `cargo clippy --workspace --all-targets -- -D warnings`, `RUSTORY_RELEASE_LINUX_REMOTE=node0 scripts/build-release-assets.sh --target x86_64-unknown-linux-gnu --dist-dir /tmp/rustory-ci-linux-process-status --linux-builder ssh`.
