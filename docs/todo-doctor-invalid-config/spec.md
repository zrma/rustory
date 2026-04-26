# Spec: doctor-invalid-config

## 배경

- 요청 맥락: 활성 todo가 없어 다음 MVP 온보딩/진단 마일스톤을 검토했다.
- 현재 문제/기회: `rr doctor`는 설정 파일 자체가 잘못된 TOML이면 `config::load_default()` 단계에서 종료되어, 사용자가 어떤 파일을 고쳐야 하는지 doctor 보고서로 확인할 수 없다.

## 계획 스냅샷

- 목표: `rr doctor`가 invalid config 상태에서도 실행되어 텍스트/JSON 보고서에 config parse error를 드러내게 한다.
- 범위: `src/cli.rs`의 doctor 실행 경로, 관련 단위 테스트, quickstart/p2p의 doctor 설명만 수정한다.
- 검증 명령: `cargo test doctor --workspace`, invalid config smoke(`HOME=$(mktemp -d) ... target/debug/rr doctor --json`), `scripts/run-manifest-checks.sh --mode quick --work-id doctor-invalid-config`.
- 완료 기준: invalid config에서도 `rr doctor`가 0으로 종료하고 config error를 출력하며, 관련 테스트와 todo readiness/quick manifest 게이트가 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `cargo test doctor --workspace` | `rr doctor`가 config parse error를 구조화해 보고하도록 구현하고 회귀 테스트를 추가한다. |
| C2 | todo | codex | `HOME=$(mktemp -d) ... target/debug/rr doctor --json` | 실제 CLI smoke로 invalid config에서도 doctor가 종료 코드 0과 JSON error 필드를 반환함을 확인한다. |
| C3 | todo | codex | `scripts/run-manifest-checks.sh --mode quick --work-id doctor-invalid-config` | 관련 문서와 todo 상태를 갱신하고 repo quick 게이트를 통과시킨다. |

## 완료/미완료/다음 액션

- 완료: 없음.
- 미완료: C1, C2, C3.
- 다음 액션: `src/cli.rs`의 config load/doctor 보고 경로를 수정하고 단위 테스트부터 추가한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-doctor-invalid-config`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-doctor-invalid-config/open-questions.md`.
