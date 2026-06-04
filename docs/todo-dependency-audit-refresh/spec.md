# Spec: dependency-audit-refresh

## 배경

- 요청 맥락: 활성 `docs/todo-*`가 없어 다음 유지보수 후보를 검토하던 중 dependency audit 경로와 lockfile 갱신 상태를 점검했다.
- 현재 문제/기회: `cargo-audit 0.21.2`는 최신 advisory DB의 CVSS 4.0 항목을 파싱하지 못했고, 도구 갱신 후 현재 `Cargo.lock`에는 `paste 1.0.15` unmaintained warning 1건이 남는다. `cargo update --dry-run`은 Rust 1.95.0 호환 lockfile patch/minor 갱신 가능성을 보여준다.

## 계획 스냅샷

- 목표: Rust 1.95.0 기준으로 `Cargo.lock`을 최신 호환 버전으로 갱신하고, 남는 advisory warning이 직접 수정 가능한지/구조적 잔여인지 재현 가능한 증거로 남긴다.
- 범위: `Cargo.lock`, dependency audit 결과, 해당 결정을 추적할 todo/교훈 로그만 수정한다. libp2p transport 업그레이드, Cargo.toml major 변경, P2P 프로토콜 변경은 제외한다.
- 검증 명령: `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" scripts/check.sh --fast`, `/Users/user/.cargo/bin/cargo-audit audit`, `scripts/run-manifest-checks.sh --mode quick --work-id dependency-audit-refresh`.
- 완료 기준: lockfile 갱신 후 Rust 기본 검증이 통과하고, `paste 1.0.15` warning의 transitive 경로가 `libp2p-tcp -> if-watch -> netlink-*` 구조적 잔여로 기록되며, 완료 todo가 삭제 가능해진다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `/Users/user/.cargo/bin/cargo-audit audit` | 최신 `cargo-audit`로 현재 advisory 상태와 `paste` transitive 경로를 확인한다. |
| C2 | done | codex | `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" cargo update` | Rust 1.95.0 호환 lockfile patch/minor 갱신을 적용한다. |
| C3 | done | codex | `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" scripts/check.sh --fast` | lockfile 갱신 후 fmt/test/clippy 빠른 게이트를 통과시킨다. |
| C4 | done | codex | `scripts/run-manifest-checks.sh --mode quick --work-id dependency-audit-refresh` | todo readiness와 문서/스크립트 quick 게이트를 통과시킨다. |
| C5 | todo | codex | `scripts/check-todo-closure.sh` | 완료 todo 식별자를 `docs/LESSONS_LOG.md`에 남기고 `docs/todo-dependency-audit-refresh`를 삭제한다. |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3, C4. `Cargo.lock`을 Rust 1.95.0 호환 최신 patch/minor로 갱신했고 빠른 표준 게이트가 통과했다.
- 미완료: C5.
- 다음 액션: `docs/LESSONS_LOG.md`에 `todo-dependency-audit-refresh` 마감 증거를 남기고 todo 디렉터리를 삭제한다.
- 검증 증거: `/Users/user/.cargo/bin/cargo-audit audit`, `cargo tree --target all -i paste`, `cargo update`, `PATH="$(dirname "$(/opt/homebrew/bin/rustup which cargo)"):$PATH" scripts/check.sh --fast`, `scripts/run-manifest-checks.sh --mode quick --work-id dependency-audit-refresh`.
