# Spec: retention-keep-recent-policy

## 배경

- 요청 맥락: `docs/mvp.md`의 "고급 보관 정책은 후속" 항목을 소규모 기능으로 선반영한다.
- 현재 문제/기회: 현재 보관 정책은 `older-than-days` 단일 기준이라, 오래된 데이터 대량 정리 시 최근 작업 히스토리까지 과도하게 제거될 수 있다.

## 계획 스냅샷

- 목표: `rr prune`와 auto-prune에 "최신 N개 보존(keep-recent)" 정책을 추가해 보관 정책 안전성을 높인다.
- 범위: `src/cli.rs`(옵션/환경변수 파싱 + 호출), `src/storage.rs`(삭제 쿼리 확장), 관련 테스트, 사용자 문서(`docs/hook.md`, `docs/quickstart.md`, `docs/mvp.md`)를 갱신한다.
- 검증 명령: `cargo fmt --all --check`, `cargo test --workspace`, `scripts/run-manifest-checks.sh --mode quick --work-id retention-keep-recent-policy`.
- 완료 기준: `rr prune --older-than-days <d> --keep-recent <n>`이 최신 `n`개를 보존하며 동작하고, `RUSTORY_AUTO_PRUNE_KEEP_RECENT`도 동일 정책으로 적용된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `cargo fmt --all --check && cargo test --workspace && scripts/run-manifest-checks.sh --mode quick --work-id retention-keep-recent-policy` | `keep-recent` 정책(수동 prune + auto-prune env)을 구현하고 테스트/문서/검증을 완료한다. |

## 완료/미완료/다음 액션

- 완료: `src/storage.rs`에 `keep_recent` 보존 기준(최신 N개 보존)을 포함한 prune 쿼리를 구현하고 단위 테스트(`prune_entries_older_than_respects_keep_recent`)를 추가했다.
- 완료: `src/cli.rs`에 `rr prune --keep-recent` 옵션과 auto-prune env(`RUSTORY_AUTO_PRUNE_KEEP_RECENT`)를 추가하고 출력/파싱 테스트를 갱신했다.
- 완료: 사용자 문서(`docs/hook.md`, `docs/quickstart.md`, `docs/mvp.md`)에 keep-recent 정책 사용법을 반영했다.
- 미완료: C1.
- 다음 액션: feature 출고(`finalize-and-push`) 후 todo 마감 커밋에서 `LESSONS_ARCHIVE` 토큰 기록 + todo 삭제를 수행한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-retention-keep-recent-policy`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-retention-keep-recent-policy/open-questions.md`, `cargo fmt --all --check`, `cargo test --workspace`(84 passed), `scripts/run-manifest-checks.sh --mode quick --work-id retention-keep-recent-policy`.
