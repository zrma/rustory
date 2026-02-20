# Spec: doctor-key-error-resilience

## 배경

- 요청 맥락: `rr doctor --json` 후속 안정화 작업으로 진단 커맨드의 실패 내성을 높인다.
- 현재 문제/기회: 손상된 key 파일(예: invalid swarm.key)이 있으면 `rr doctor`/`rr doctor --json`이 즉시 실패해, 문제 상태를 출력하지 못한다.

## 계획 스냅샷

- 목표: 키/설정 로딩 오류가 있어도 `doctor` 명령이 종료하지 않고 진단 리포트를 출력하도록 개선한다.
- 범위:
  - `src/cli.rs`의 doctor 리포트/출력 로직 보완
  - 관련 단위 테스트 추가
  - 사용자 문서(`docs/p2p.md`)의 동작 설명 보완
- 검증 명령:
  - `cargo fmt --all --check`
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `scripts/run-manifest-checks.sh --mode quick --work-id doctor-key-error-resilience`
- 완료 기준:
  - 깨진 key 파일 환경에서도 `rr doctor --json`이 exit 0으로 JSON 출력
  - 텍스트 출력에서도 key 섹션이 `invalid` 상태를 노출
  - 새 테스트가 회귀를 차단하고 전체 검증이 통과

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test --workspace doctor_` | doctor가 key/설정 오류를 리포트로 수집하도록 코드/테스트 구현 |
| C2 | done | codex | `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` | 스타일/정적 검증 통과 |
| C3 | in_progress | codex | `scripts/finalize-and-push.sh --message "fix: keep doctor running on key parse errors" --work-id doctor-key-error-resilience` | 워크플로/문서 게이트 재검증 후 출고 및 todo 마감 커밋 준비 |

## 완료/미완료/다음 액션

- 완료: C1, C2.
- 미완료: C3.
- 다음 액션: `scripts/finalize-and-push.sh --message "fix: keep doctor running on key parse errors" --work-id doctor-key-error-resilience` 실행 후, `todo-doctor-key-error-resilience`를 `LESSONS_ARCHIVE` 참조와 함께 삭제하는 마감 커밋을 수행한다.
- 검증 증거:
  - `cargo test --workspace doctor_`
  - `cargo fmt --all --check`
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `scripts/run-manifest-checks.sh --mode quick --work-id doctor-key-error-resilience`
  - `RUSTORY_SWARM_KEY_PATH=<invalid-file> target/debug/rr doctor --json` (exit 0, `swarm_key.load_error` 출력 확인)
