# Spec: toolchain-path-fallback

## 배경

- 요청 맥락: 활성 todo/open issue/direct dependency drift가 없는 상태에서 다음 유지보수 후보를 검토했다.
- 현재 문제/기회: 이 머신의 기본 셸 PATH에는 `cargo`가 없지만 `/opt/homebrew/bin/rustup`은 존재한다. 표준 로컬 검증 명령인 `scripts/check.sh --fast`가 `cargo: command not found`로 실패해, repo 스크립트가 rustup 기반 cargo 위치를 스스로 보정하지 못한다.

## 계획 스냅샷

- 목표: 표준 로컬 검증 스크립트가 `cargo`를 PATH에서 못 찾을 때도 `rustup which cargo`로 현재 toolchain cargo를 찾아 실행할 수 있게 한다.
- 범위: repo-owned shell scripts의 toolchain PATH 보정과 관련 문서/검증 증거만 다룬다. Rust 코드, dependency version, CI workflow 변경은 제외한다.
- 검증 명령: `env PATH="/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" scripts/check.sh --fast`; `env PATH="/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" scripts/run-manifest-checks.sh --mode full --repo-key rustory --work-id toolchain-path-fallback`; `scripts/run-manifest-checks.sh --mode quick --work-id toolchain-path-fallback`.
- 완료 기준: `cargo`가 없는 기본 PATH 재현 환경에서 `scripts/check.sh --fast`와 full manifest local checks가 rustup cargo fallback으로 통과하고, 기존 quick gate가 유지된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `env PATH="/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" scripts/check.sh --fast` | `scripts/check.sh`가 `cargo`를 직접 전제하지 않고 rustup cargo fallback을 사용하게 한다. |
| C2 | done | codex | `env PATH="/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" scripts/run-manifest-checks.sh --mode full --repo-key rustory --work-id toolchain-path-fallback` | manifest full mode의 `cargo fmt/test/clippy`도 동일한 fallback 환경에서 통과하게 한다. |
| C3 | done | codex | `scripts/run-manifest-checks.sh --mode quick --work-id toolchain-path-fallback` | todo/readiness/script smoke와 문서 게이트가 기존 정책과 호환되는지 확인한다. |
| C4 | todo | codex | `scripts/check-todo-closure.sh` | 구현 커밋 후 lessons 기록과 todo workspace 삭제로 마감한다. |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3. 공용 `scripts/lib/rust-toolchain.sh`를 추가하고 `scripts/check.sh`, `scripts/run-manifest-checks.sh`, `scripts/smoke_p2p_local.sh`, `scripts/acceptance_docker_macos_linux.sh`에서 cargo fallback을 사용한다. `scripts/check-script-smoke.sh`는 lib와 주요 non-check scripts의 syntax smoke를 함께 확인한다.
- 미완료: C4. lessons 기록과 todo workspace 삭제는 구현 커밋 이후 별도 마감 단위로 진행한다.
- 다음 액션: 구현 단위를 커밋/푸시한 뒤 C4 todo closure를 진행한다.
- 검증 증거: 실패 재현 `scripts/check.sh --fast` -> `scripts/check.sh: line 55: cargo: command not found`; 수정 후 `env PATH="/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" scripts/check.sh --fast`; `env PATH="/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" scripts/run-manifest-checks.sh --mode full --repo-key rustory --work-id toolchain-path-fallback`; `scripts/run-manifest-checks.sh --mode quick --work-id toolchain-path-fallback`.
