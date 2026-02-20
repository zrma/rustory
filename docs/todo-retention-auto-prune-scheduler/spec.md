# Spec: retention-auto-prune-scheduler

## 배경

- 요청 맥락: `docs/mvp.md`의 "자동 보관 정책/스케줄링은 후속" 항목을 실제 동작으로 연결한다.
- 현재 문제/기회: 현재는 `rr prune --older-than-days <n>` 수동 실행만 가능해 장기간 사용 시 로컬 DB 정리가 누락되기 쉽다.

## 계획 스냅샷

- 목표: `rr record` 경로에서 opt-in 자동 prune 스케줄링(간격 제한)을 제공해 수동 정리 누락을 줄인다.
- 범위: `src/cli.rs`의 record 경로(auto prune 트리거/레이트리밋/env 파싱), 관련 테스트, 사용자 문서(`docs/hook.md`, `docs/quickstart.md`, `docs/mvp.md`)를 갱신한다.
- 검증 명령: `cargo fmt --all --check`, `cargo test --workspace`, `scripts/run-manifest-checks.sh --mode quick --work-id retention-auto-prune-scheduler`.
- 완료 기준: `RUSTORY_AUTO_PRUNE=1` 설정 시 `rr record`가 주기 제한(`RUSTORY_AUTO_PRUNE_INTERVAL_SEC`)에 따라 자동 prune을 실행하고, 실패해도 record 자체는 성공하며 문서가 최신화된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `cargo fmt --all --check && cargo test --workspace && scripts/run-manifest-checks.sh --mode quick --work-id retention-auto-prune-scheduler` | record 경로 자동 prune 스케줄링(env 기반 enable + interval marker)과 테스트/문서를 구현하고 출고 게이트를 통과시킨다. |

## 완료/미완료/다음 액션

- 완료: `src/cli.rs`에 record 성공 후 opt-in 자동 prune 트리거와 간격 제한(marker 기반), 관련 env 파싱(`RUSTORY_AUTO_PRUNE*`)을 추가했다.
- 완료: interval marker 유틸 일반화 및 관련 테스트를 갱신했고 문서(`docs/hook.md`, `docs/quickstart.md`, `docs/mvp.md`)를 동기화했다.
- 미완료: C1 출고(최종 게이트 통과 후 describe/bookmark/push).
- 다음 액션: `scripts/finalize-and-push.sh --message "feat: add auto prune scheduler" --work-id retention-auto-prune-scheduler`를 통과시켜 코드 변경을 출고한다.
- 검증 증거: `cargo fmt --all --check` 통과, `cargo test --workspace` 통과(83 passed), `scripts/run-manifest-checks.sh --mode quick --work-id retention-auto-prune-scheduler` 통과.
