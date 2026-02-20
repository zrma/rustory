# Spec: doctor-json-output

## 배경

- 현재 `rr doctor`는 텍스트 출력만 제공해서 자동 점검 스크립트/머신 파싱 연동이 어렵다.
- 최근 doctor 출력 항목(async upload/auto prune 포함)이 늘어나면서, 구조화 출력의 필요성이 커졌다.

## 계획 스냅샷

- 목표: `rr doctor --json`을 추가해 동일 진단 정보를 JSON으로 출력한다.
- 범위:
  - `src/cli.rs` doctor 서브커맨드에 `--json` 플래그 추가
  - doctor JSON 리포트 구조체/빌더 추가
  - 텍스트 모드는 기존 동작을 유지
  - 사용자 문서(`docs/p2p.md`, `docs/quickstart.md`)에 새 옵션 반영
- 검증 명령:
  - `scripts/run-manifest-checks.sh --mode quick --work-id doctor-json-output`
  - `cargo fmt --all --check`
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
- 완료 기준:
  - `rr doctor --json`이 유효한 JSON을 출력한다.
  - JSON에 기본 설정/키/트래커/자동화 상태가 포함된다.
  - C1..C3가 `done`이고 검증 증거가 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test --workspace` | doctor CLI에 `--json` 옵션과 JSON 출력 경로 구현 |
| C2 | done | codex | `cargo test --workspace` | doctor JSON 출력 단위 테스트 추가 |
| C3 | in_progress | codex | `scripts/run-manifest-checks.sh --mode quick --work-id doctor-json-output` | 문서 반영 및 게이트 통과 |

## 완료/미완료/다음 액션

- 완료: C1, C2.
- 미완료: C3(마감 커밋에서 todo 정리 예정).
- 다음 액션: 기능 커밋을 먼저 완료한 뒤, 마감 커밋에서 `todo-doctor-json-output` 식별자를 `LESSONS_ARCHIVE`에 남기고 todo를 삭제한다.
- 검증 증거:
  - `cargo fmt --all --check`
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `scripts/run-manifest-checks.sh --mode quick --work-id doctor-json-output`
  - `target/debug/rr doctor --json`
