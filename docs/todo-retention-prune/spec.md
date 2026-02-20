# Spec: retention-prune

## 배경

- 요청 맥락: `docs/mvp.md`에 남아 있는 "삭제/정리 후속" 항목을 실제 CLI 기능으로 제공해 운영자가 로컬 히스토리 용량을 직접 관리할 수 있게 한다.
- 현재 문제/기회: 현재는 히스토리가 append-only로만 증가해 장기간 사용 시 DB 크기와 검색 비용이 계속 증가한다.

## 계획 스냅샷

- 목표: `rr prune --older-than-days <n> [--dry-run]` 명령을 추가해 cutoff 이전 엔트리 수/삭제 결과를 일관되게 출력한다.
- 범위: `src/cli.rs`, `src/storage.rs` 구현/테스트와 사용자 문서(`docs/mvp.md`, `docs/quickstart.md`)를 갱신한다.
- 검증 명령: `cargo fmt --all --check`, `cargo test --workspace`, `scripts/run-manifest-checks.sh --mode quick --work-id retention-prune`.
- 완료 기준: prune 명령 파싱/실행/출력 및 storage 단위 테스트가 통과하고 문서의 후속 항목이 최신 상태로 반영된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `cargo fmt --all --check && cargo test --workspace && scripts/run-manifest-checks.sh --mode quick --work-id retention-prune` | `rr prune --older-than-days <n> [--dry-run]` 커맨드와 storage 정리 로직/테스트/문서를 구현한다. |

## 완료/미완료/다음 액션

- 완료: `src/storage.rs`에 `prune_entries_older_than(cutoff_unix, dry_run)`를 추가해 후보 카운트와 실제 삭제를 분리했다.
- 완료: `src/cli.rs`에 `rr prune --older-than-days <n> [--dry-run]`를 추가하고 cutoff 계산/입력 검증(`--older-than-days >= 1`)을 반영했다.
- 완료: `src/storage.rs`, `src/cli.rs` 테스트를 확장하고 `docs/mvp.md`, `docs/quickstart.md` 사용법을 갱신했다.
- 미완료: todo 마감 커밋(C1 상태 `done` 전환 + `docs/todo-retention-prune` 삭제) 처리.
- 다음 액션: 기능 변경을 먼저 출고한 뒤 todo 마감 커밋에서 C1을 `done`으로 닫고 워크스페이스를 삭제한다.
- 검증 증거: `scripts/start-work.sh --work-id retention-prune`, `cargo fmt --all --check`, `cargo test --workspace`, `scripts/run-manifest-checks.sh --mode quick --work-id retention-prune`.
