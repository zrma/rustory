# Spec: async-upload-queue

## 배경

- 요청 맥락: `docs/mvp.md`의 "비동기 업로드(네트워크 실패 시 큐 유지)" 후속 항목을 실제 동작으로 연결한다.
- 현재 문제/기회: 현재는 `rr record`가 로컬 저장만 수행하고 업로드는 별도 `p2p-sync --push` 실행에 의존해, hook 기반 사용에서 업로드 지연이 쉽게 발생한다.

## 계획 스냅샷

- 목표: hook가 호출하는 `rr record`에서 선택적으로 비동기 업로드 트리거를 실행해, 네트워크 실패 시에도 로컬 큐(`pending_push`)를 유지하며 자동 재시도를 시작한다.
- 범위: `src/cli.rs`의 record 경로(환경변수 기반 async trigger + rate-limit)와 관련 테스트, 사용자 문서(`docs/hook.md`, `docs/quickstart.md`, `docs/mvp.md`)를 갱신한다.
- 검증 명령: `cargo fmt --all --check`, `cargo test --workspace`, `scripts/run-manifest-checks.sh --mode quick --work-id async-upload-queue`.
- 완료 기준: `RUSTORY_ASYNC_UPLOAD=1`일 때 `rr record`가 백그라운드 `p2p-sync --push`를 간헐적으로 트리거하고, 실패해도 record 성공/큐 보존이 유지되며 문서가 최신화된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `cargo fmt --all --check && cargo test --workspace && scripts/run-manifest-checks.sh --mode quick --work-id async-upload-queue` | record 경로 비동기 업로드 트리거/레이트리밋/테스트/문서를 구현한다. |

## 완료/미완료/다음 액션

- 완료: `src/cli.rs` record 경로에 `RUSTORY_ASYNC_UPLOAD` 기반 백그라운드 `p2p-sync --push` 트리거와 marker 파일 기반 최소 간격(rate-limit) 로직을 추가했다.
- 완료: `src/cli.rs`에 async 업로드 보조 함수/파서와 단위 테스트(불린 파싱, marker roundtrip, 트리거 간격 판정)를 추가했다.
- 완료: `docs/hook.md`, `docs/quickstart.md`, `docs/mvp.md`에 async 업로드 환경변수/동작을 반영했다.
- 미완료: `C1` 상태를 `done`으로 전환하고 마감 커밋에서 todo workspace를 삭제해야 한다.
- 다음 액션: 구현 턴 종료 절차(커밋/푸시) 수행.
- 검증 증거: `scripts/start-work.sh --work-id async-upload-queue`, `cargo fmt --all --check`, `cargo test --workspace`, `scripts/run-manifest-checks.sh --mode quick --work-id async-upload-queue`.
