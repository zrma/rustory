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
| C1 | todo | codex | `env PATH="/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" scripts/check.sh --fast` | `scripts/check.sh`가 `cargo`를 직접 전제하지 않고 rustup cargo fallback을 사용하게 한다. |
| C2 | todo | codex | `env PATH="/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" scripts/run-manifest-checks.sh --mode full --repo-key rustory --work-id toolchain-path-fallback` | manifest full mode의 `cargo fmt/test/clippy`도 동일한 fallback 환경에서 통과하게 한다. |
| C3 | todo | codex | `scripts/run-manifest-checks.sh --mode quick --work-id toolchain-path-fallback` | todo/readiness/script smoke와 문서 게이트가 기존 정책과 호환되는지 확인한다. |

## 완료/미완료/다음 액션

- 완료: 계획 workspace 생성 및 초기 readiness/quick manifest 게이트 통과.
- 미완료: C1, C2, C3.
- 다음 액션: repo scripts에 cargo PATH fallback을 추가하고 재현 환경에서 검증한다.
- 검증 증거: `scripts/start-work.sh --work-id toolchain-path-fallback`; 실패 재현 `scripts/check.sh --fast` -> `scripts/check.sh: line 55: cargo: command not found`.
