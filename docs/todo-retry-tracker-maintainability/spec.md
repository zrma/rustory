# Spec: retry-tracker-maintainability

## 배경

- 요청 맥락: 기존 동작을 보존하면서 HTTP retry 계약과 tracker HTTP 라우팅의 유지보수성을 개선하고 patch release까지 배포한다.
- 현재 문제/기회: `request_with_retry`는 정상 불변식을 `Option`과 `expect`로 표현하고 핵심 재시도 분기 테스트가 부족하다. `route_http_request`는 peer endpoint 구현까지 한 함수에 포함해 보안·상태 전이 경계를 검토하기 어렵다.

## 계획 스냅샷

- 목표: 재시도 횟수·오류 분류를 characterization test로 고정하고 panic-free loop로 단순화하며, tracker peer endpoint를 동작 변경 없이 handler로 추출한다.
- 범위: `src/http_retry.rs`, `src/tracker.rs`, 관련 테스트, patch version metadata, 작업 패킷과 일반화 가능한 lesson만 변경한다.
- 비범위: P2P event loop/CLI module 분할, retry timing/default 변경, tracker API·응답·인증·상태 전이 변경, 새 dependency 도입.
- 검증 명령: `cargo test http_retry`, `cargo test tracker`, `scripts/check.sh`, `scripts/check-release-gates.sh --manifest-mode full --work-id retry-tracker-maintainability`.
- 완료 기준: focused/full gate가 통과하고 공개 release asset의 version/revision/GLIBC 기준을 검증한 뒤 local canary와 관리 대상 peer의 `rr version`, `rr doctor`, `rr sync-status --json --with-tracker`가 정상이다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test http_retry` | HTTP retry의 zero-attempt, retryable, non-retryable 계약을 고정하고 panic-free loop로 정리 |
| C2 | todo | codex | `cargo test tracker` | tracker peer register/unregister/list/authorize를 동작 보존 handler로 추출 |
| C3 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id retry-tracker-maintainability` | canonical/full source gate와 공개 경계 검증 |
| C4 | todo | codex | `scripts/release-version.sh --version v1.0.63 --profile daily-driver --gate none --work-id retry-tracker-maintainability` | v1.0.63 source/tag/assets/checksum/GLIBC 출고 |
| C5 | todo | codex | `rr version && rr doctor && rr sync-status --json --with-tracker` | local canary와 관리 대상 peer 배포·상태 검증 |

## 완료/미완료/다음 액션

- 완료: 작업 범위와 회귀 방지 검증 경계를 확정했다. HTTP retry의 zero-attempt, 즉시 실패, transient 복구, attempt budget과 상태 분류를 테스트로 고정하고 panic-free loop로 정리했다.
- 미완료: C2-C5.
- 다음 액션: tracker peer endpoint 구현을 독립 handler로 추출한다.
- 검증 증거: `cargo test http_retry`(6 passed), `cargo clippy --all-targets -- -D warnings`.
