# Spec: init-force-invalid-config

## 배경

- 요청 맥락: `rr doctor`가 invalid config를 진단하도록 만든 뒤, 같은 복구 경로에서 `rr init --force` 동작을 확인했다.
- 현재 문제/기회: `~/.config/rustory/config.toml`이 깨진 상태에서는 `rr init --force`도 `config::load_default()` 단계에서 종료되어, 사용자가 강제 초기화로 설정 파일을 복구할 수 없다.

## 계획 스냅샷

- 목표: `rr init --force`가 invalid config를 default/env 기준으로 무시하고 새 config를 써서 복구할 수 있게 한다.
- 범위: `src/cli.rs`의 config load 허용 조건, 관련 테스트, quickstart 문서의 복구 안내만 수정한다.
- 검증 명령: `cargo test init --workspace`, invalid config smoke(`HOME=$(mktemp -d) ... target/debug/rr init --force ...`), `scripts/run-manifest-checks.sh --mode quick --work-id init-force-invalid-config`.
- 완료 기준: invalid config 상태에서 `rr init --force`는 0으로 종료하고 새 config를 쓰며, `rr init` without `--force`는 기존처럼 parse error를 반환한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `cargo test init --workspace` | `rr init --force`에 한해 invalid config load error를 default config로 복구하는 경로를 구현한다. |
| C2 | todo | codex | `HOME=$(mktemp -d) ... target/debug/rr init --force ...` | 실제 CLI smoke로 invalid config overwrite 성공과 non-force 실패를 확인한다. |
| C3 | todo | codex | `scripts/run-manifest-checks.sh --mode quick --work-id init-force-invalid-config` | 관련 문서와 todo 상태를 갱신하고 repo quick 게이트를 통과시킨다. |

## 완료/미완료/다음 액션

- 완료: 없음.
- 미완료: C1, C2, C3.
- 다음 액션: config load error 허용 조건을 `Doctor`와 `Init --force`로 분리하고 단위 테스트/CLI smoke를 추가한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-init-force-invalid-config`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-init-force-invalid-config/open-questions.md`.
