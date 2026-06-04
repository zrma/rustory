# Spec: sync-token-normalization

## 배경

- 요청 맥락: 활성 todo/GitHub issue가 없어 다음 유지보수 후보를 검토하던 중 bearer token 경계가 CLI resolver와 transport/tracker 내부 API로 나뉘어 있음을 확인했다.
- 현재 문제/기회: CLI resolver는 공백 token을 `None`으로 정규화하지만, tracker/HTTP sync 서버 라우터와 일부 client config 경계는 raw token을 직접 받을 수 있다. 공백 token은 인증 헤더로 의미가 없으므로, server-side 보호 경계에서는 명시적으로 거부하고 client-side 송신 경계에서는 빈 bearer 값을 보내지 않도록 정규화한다.

## 계획 스냅샷

- 목표: HTTP sync와 tracker bearer token 경계에서 공백 token이 무의미한 인증 상태로 흘러가지 않게 하고, 회귀 테스트로 고정한다.
- 범위: `src/transport.rs`, `src/tracker.rs`, 관련 단위 테스트, closure용 `docs/LESSONS_LOG.md` 기록.
- 검증 명령: `PATH=/opt/homebrew/Cellar/rustup/1.29.0_1/bin:$PATH cargo test transport::tests::http_sync_rejects_blank_configured_token tracker::tests::tracker_rejects_blank_configured_token --workspace`, `PATH=/opt/homebrew/Cellar/rustup/1.29.0_1/bin:$PATH scripts/check.sh --fast`, `scripts/run-manifest-checks.sh --mode quick --work-id sync-token-normalization`.
- 완료 기준: 공백 configured token이 서버 시작 전 오류 또는 인증 실패로 닫히고, HTTP sync/tracker 기존 token 성공/실패 테스트가 유지되며, 완료 todo 삭제와 교훈 로그 기록이 함께 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `scripts/start-work.sh --work-id sync-token-normalization` | 활성 todo/issue 부재 확인 후 토큰 정규화 hardening 범위와 검증 기준을 확정 |
| C2 | todo | codex | `PATH=/opt/homebrew/Cellar/rustup/1.29.0_1/bin:$PATH cargo test transport::tests::http_sync_rejects_blank_configured_token tracker::tests::tracker_rejects_blank_configured_token --workspace` | HTTP sync/tracker configured token 경계에서 공백 token을 거부/정규화하고 회귀 테스트 추가 |
| C3 | todo | codex | `scripts/run-manifest-checks.sh --mode quick --work-id sync-token-normalization` | 완료 상태를 교훈 로그에 반영하고 todo workspace 삭제 |

## 완료/미완료/다음 액션

- 완료: C1.
- 미완료: C2, C3.
- 다음 액션: `src/transport.rs`와 `src/tracker.rs`에 blank token 회귀 테스트를 먼저 추가한 뒤 구현한다.
- 검증 증거: `scripts/start-work.sh --work-id sync-token-normalization`, `scripts/check-todo-readiness.sh docs/todo-sync-token-normalization`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-sync-token-normalization/open-questions.md`.
