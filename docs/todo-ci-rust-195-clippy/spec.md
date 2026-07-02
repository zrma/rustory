# Spec: ci-rust-195-clippy

## 배경

- 요청 맥락: GitHub Actions `CI`와 `Release Gates`가 Rust/Clippy 1.95에서 `src/self_update.rs`의 `clippy::collapsible_if`를 `-D warnings`로 실패시키고 있다.
- 현재 문제/기회: 로컬 검증은 통과했지만 CI toolchain의 최신 Clippy lint가 더 엄격해서 main이 반복적으로 빨간 상태가 됐다.

## 계획 스냅샷

- 목표: Rust 1.95 Clippy 기준에서도 CI가 통과하도록 `self_update`의 경고를 제거한다.
- 범위: `src/self_update.rs`의 Clippy-only 구조 수정과 작업 증적 문서만 포함한다.
- 검증 명령: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test self_update`, `scripts/check-release-gates.sh --manifest-mode full --dry-run --work-id ci-rust-195-clippy`.
- 완료 기준: 로컬 clippy/test/gate가 통과하고, push 후 GitHub Actions 최신 main run의 `CI`/`Release Gates`가 green으로 회복된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `cargo clippy --workspace --all-targets -- -D warnings` | Rust 1.95 `collapsible_if` 경고 제거 |
| C2 | in_progress | codex | `cargo test self_update` | self-update daemon restart 경로 회귀 확인 |
| C3 | in_progress | codex | `scripts/check-release-gates.sh --manifest-mode full --dry-run --work-id ci-rust-195-clippy` | release gate 경로에서 CI 재발 방지 확인 |

## 완료/미완료/다음 액션

- 완료: CI 실패 원인을 Rust 1.95 Clippy의 `collapsible_if` 경고로 확인하고, `src/self_update.rs`의 중첩 조건을 collapsed form으로 수정했다. 로컬 `cargo fmt --all --check`, `cargo test self_update`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/check-release-gates.sh --manifest-mode full --dry-run --work-id ci-rust-195-clippy`는 통과했다.
- 미완료: GitHub Actions 최신 main run에서 `CI`/`Release Gates` green 회복 확인.
- 다음 액션: fix commit을 push한 뒤 원격 CI 결과를 확인하고, green이면 todo를 closeout한다.
- 검증 증거: `gh run view 28580738506 --repo zrma/rustory --log-failed`, `gh run view 28580738504 --repo zrma/rustory --log-failed`, `cargo fmt --all --check`, `cargo test self_update`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/check-release-gates.sh --manifest-mode full --dry-run --work-id ci-rust-195-clippy`.
