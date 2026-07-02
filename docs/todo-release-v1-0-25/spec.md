# Spec: release-v1-0-25

## 배경

- 요청 맥락: `rr dedupe`가 `origin/main`에 반영됐고 CI가 성공했으므로 daily-driver release와 fleet 배포가 필요하다.
- 현재 문제/기회: `v1.0.24` GitHub Release는 이미 이전 커밋으로 발행되어 있어, 현재 `main`을 배포하려면 새 patch release가 필요하다.

## 계획 스냅샷

- 목표: package version을 `1.0.25`로 올리고, `v1.0.25` GitHub Release를 만든 뒤 로컬 맥북과 k8s 5개 노드에 배포한다.
- 범위:
  - `Cargo.toml` / `Cargo.lock` 버전 bump.
  - release gate와 GitHub Release asset 검증.
  - `rr update` 기반 로컬 맥북 및 k8s 노드 배포.
- 범위 밖:
  - 새 기능 추가.
  - 운영 tracker/relay/k8s manifest 변경.
- 검증 명령:
  - `cargo fmt --all --check`
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-25`
  - `scripts/release-version.sh --version v1.0.25 --profile daily-driver --gate full --work-id release-v1-0-25`
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고 release/deploy/CI 증거가 재현 가능하게 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test --workspace` | package version bump 후 기본 Rust 회귀 검증 |
| C2 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-25` | full release gate 통과 |
| C3 | todo | codex | `scripts/release-version.sh --version v1.0.25 --profile daily-driver --gate full --work-id release-v1-0-25` | GitHub Release `v1.0.25` 발행 |
| C4 | todo | codex | `rr version` | 로컬 맥북 및 k8s 5개 노드 배포 확인 |

## 완료/미완료/다음 액션

- 완료: C1, C2.
- 미완료: C3, C4.
- 다음 액션: 커밋/푸시, release 발행, fleet 배포를 순서대로 진행한다.
- 검증 증거: `cargo fmt --all --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-25`.
