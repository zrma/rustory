# Spec: cli-help-descriptions

## 배경

- 요청 맥락: 활성 todo가 없어 다음 MVP 온보딩/진단 흐름을 검토하던 중 `rr --help`를 확인했다.
- 현재 문제/기회: command 목록이 이름만 보이고 설명이 비어 있어 `init`, `doctor`, `p2p-sync`, `hook` 같은 첫 사용 경로를 help만으로 구분하기 어렵다.

## 계획 스냅샷

- 목표: `rr --help`와 주요 subcommand help가 각 명령의 역할을 한 줄로 드러내게 한다.
- 범위: `src/cli.rs`의 clap help metadata와 관련 테스트, 마감 lessons 기록만 수정한다.
- 검증 명령: `cargo test help --workspace`, `target/debug/rr --help`, `target/debug/rr init --help`, `scripts/run-manifest-checks.sh --mode quick --work-id cli-help-descriptions`.
- 완료 기준: main help command 목록에 설명이 출력되고, `init`/`doctor`/`p2p-sync`/`hook`의 목적이 help 출력과 테스트로 확인된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `cargo test help --workspace` | clap metadata와 단위 테스트로 주요 command 설명이 help에 노출되는지 고정한다. |
| C2 | todo | codex | `target/debug/rr --help && target/debug/rr init --help` | 실제 CLI help 출력에서 빈 설명이 사라졌는지 확인한다. |
| C3 | todo | codex | `scripts/run-manifest-checks.sh --mode quick --work-id cli-help-descriptions` | lessons log와 todo closure를 정리하고 repo quick gate를 통과시킨다. |

## 완료/미완료/다음 액션

- 완료: 없음.
- 미완료: C1, C2, C3.
- 다음 액션: Command enum에 subcommand별 help 설명을 추가하고 help rendering 테스트를 작성한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-cli-help-descriptions`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-cli-help-descriptions/open-questions.md`.
