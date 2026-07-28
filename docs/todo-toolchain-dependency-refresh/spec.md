# Spec: toolchain-dependency-refresh

## 배경

- 요청 맥락: Rust toolchain과 호환 의존성 유지보수를 검증한 뒤 `main`에 출고한다.
- 현재 문제/기회: Rust 1.97.1과 호환 dependency update는 로컬 검증을 통과했지만 아직 원격에 반영되지 않았다.
- 게이트 발견: 출고 준비 중 두 core 운영 문서의 `Last Verified`가 31일이 되어 strict freshness gate가 차단되었고, 현재 harness/manifest/스크립트와의 정합성을 재검토했다.

## 계획 스냅샷

- 목표: toolchain/dependency refresh와 운영 문서 freshness를 strict gate로 검증하고 원격 `main` 및 동일 SHA CI까지 반영한다.
- 범위: `rust-toolchain.toml`, Rust action을 고정하는 CI workflow, `Cargo.lock`, 재검토한 core 운영 문서 두 개, 현재 todo 상태만 변경한다.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id toolchain-dependency-refresh`, machine-local publication guard, 원격 SHA 및 GitHub Actions 확인.
- 완료 기준: 모든 로컬/publication gate가 통과하고 `origin/main`과 필수 GitHub Actions가 같은 출고 SHA에서 성공한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `rustc --version` | local 및 CI toolchain을 Rust 1.97.1로 맞춘다. |
| C2 | done | codex | `cargo update --dry-run --locked` | 호환 범위 내 dependency update 잔여가 없도록 `Cargo.lock`을 갱신한다. |
| C3 | done | codex | `scripts/check.sh --fast` | format, Rust tests, clippy, installer tests를 통과한다. |
| C4 | done | codex | `cargo test --no-default-features --workspace && scripts/smoke_p2p_local.sh && cargo audit` | feature/P2P 회귀와 vulnerability 상태를 확인한다. |
| C5 | done | codex | `scripts/check-doc-last-verified.sh` | core 운영 문서 두 개를 현재 harness/manifest/스크립트와 재검토하고 freshness를 갱신한다. |
| C6 | todo | codex | `git ls-remote --heads origin main && gh run list --commit <published-sha>` | 원격 `main`과 동일 SHA CI 성공을 확인하고 todo를 마감한다. |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3, C4, C5.
- 미완료: C6.
- 다음 액션: full/publication gate 통과 후 첫 change를 push하고 동일 SHA CI를 확인한다.
- 검증 증거: `scripts/check.sh --fast`, no-default/import feature checks, `scripts/smoke_p2p_local.sh`, `cargo update --dry-run --locked`, `cargo audit`, `scripts/check-doc-last-verified.sh`.
