# Spec: retry-duration-saturation

## 배경

- 요청 맥락: 활성 `docs/todo-*`와 GitHub 이슈가 없는 상태에서 다음 유지보수 후보를 검토했고, 네트워크 재시도 경계 조건을 작게 닫을 수 있는 마일스톤으로 선정했다.
- 현재 문제/기회: `src/http_retry.rs`의 exponential duration 계산은 `checked_mul` overflow 시 base duration으로 되돌아갈 수 있다. `p2p_request_attempts` 같은 재시도 설정이 비정상적으로 커지면 timeout/backoff가 cap으로 포화되지 않고 짧아질 수 있다.
- 후보 검토: tracker user_id URL 인코딩은 이미 공백/슬래시 케이스가 테스트되고 있어 제외했다. 이번 범위는 retry duration saturation만 다룬다.

## 계획 스냅샷

- 목표: exponential timeout/backoff 계산이 overflow와 큰 attempt 값에서도 cap 또는 `Duration::MAX`로 단조 포화되도록 보장한다.
- 범위: `src/http_retry.rs`의 duration 계산과 단위 테스트, 필요한 경우 retry 정책 문서 한정.
- 검증 명령: `cargo test http_retry`, `scripts/run-manifest-checks.sh --mode quick --work-id retry-duration-saturation`, 출고 시 `scripts/finalize-and-push.sh --message "<type>: <summary>" --work-id retry-duration-saturation`.
- 완료 기준: overflow/cap 경계 테스트가 추가되고, retry duration 계산이 base로 되돌아가지 않으며, manifest quick 및 출고 게이트가 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test http_retry` | `exp_duration` overflow/cap 경계 테스트를 추가하고 실패를 확인한다. |
| C2 | done | codex | `cargo test http_retry` | `exp_duration`이 overflow 시 cap 또는 `Duration::MAX`로 포화되도록 수정한다. |
| C3 | done | codex | `scripts/run-manifest-checks.sh --mode quick --work-id retry-duration-saturation` | todo readiness/manifest quick 게이트와 체크포인트를 갱신한다. |
| C4 | todo | codex | `scripts/check-todo-closure.sh` | 마감 커밋에서 작업 디렉터리를 삭제하고 교훈 로그에 식별자를 남긴다. |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3.
- 미완료: C4.
- 다음 액션: 구현 커밋을 출고한 뒤 마감 커밋에서 작업 디렉터리를 삭제하고 교훈 로그에 식별자를 남긴다.
- 검증 증거: `cargo test http_retry` 최초 실행은 overflow 테스트 2개 실패를 확인했고, 수정 후 `cargo test http_retry`, `cargo fmt --all --check`, `scripts/run-manifest-checks.sh --mode quick --work-id retry-duration-saturation` 통과.
