# Spec: cli-flag-help-descriptions

## 배경

- 요청 맥락: 활성 todo가 없어 다음 온보딩 마일스톤을 검토했고, 직전 `cli-help-descriptions` 작업 이후 하위 명령 플래그 help가 비어 있는 상태를 확인했다.
- 현재 문제/기회: `rr doctor --help`, `rr p2p-sync --help`, `rr record --help` 등에서 주요 플래그 이름만 보이고 설명이 없어 첫 사용자가 어떤 값을 넣어야 하는지 help 한 화면에서 판단하기 어렵다.

## 계획 스냅샷

- 목표: CLI 하위 명령의 사용자 입력 플래그에 한 줄 설명을 추가하고, 설명 누락이 재발하지 않도록 help 테스트를 보강한다.
- 범위: `src/cli.rs`의 clap 플래그 metadata와 help 회귀 테스트, 완료 마감 로그만 수정한다.
- 검증 명령: `cargo test help --workspace`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/check-release-gates.sh --manifest-mode full --dry-run --work-id cli-flag-help-descriptions`.
- 완료 기준: 주요 하위 명령 help에서 플래그 설명이 출력되고, 전체 테스트/클리피/출고 게이트가 통과하며 todo가 삭제되고 `todo-cli-flag-help-descriptions` 식별자가 교훈 로그에 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test help --workspace` | CLI 플래그 help 설명 및 회귀 테스트 추가 |
| C2 | done | codex | `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` | 전체 Rust 검증 통과 |
| C3 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --dry-run --work-id cli-flag-help-descriptions` | 출고 게이트 통과 및 todo 마감 준비 |

## 완료/미완료/다음 액션

- 완료: C1-C3. CLI 플래그 help metadata와 회귀 테스트 추가, 전체 Rust 검증 및 release gate dry-run 통과.
- 미완료: 없음.
- 다음 액션: `docs/LESSONS_LOG.md`에 마감 증적을 남긴 뒤 todo를 삭제하고 finalize/push를 실행한다.
- 검증 증거: `scripts/start-work.sh --work-id cli-flag-help-descriptions`, `cargo test help --workspace`, `cargo build`, help smoke(`target/debug/rr doctor --help`, `target/debug/rr p2p-sync --help`, `target/debug/rr record --help`, `target/debug/rr init --help`, `target/debug/rr import --help`), `cargo fmt --all --check`, `cargo test --workspace`(114 passed), `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/check-release-gates.sh --manifest-mode full --dry-run --work-id cli-flag-help-descriptions`.
