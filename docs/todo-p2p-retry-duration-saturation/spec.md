# Spec: p2p-retry-duration-saturation

## 배경

- 요청 맥락: 활성 todo/GitHub 이슈가 없어 다음 유지보수 후보를 검토하던 중 P2P retry duration 계산이 HTTP retry와 다른 overflow fallback을 쓰는 것을 확인했다.
- 현재 문제/기회: `src/p2p.rs`의 `exp_duration`은 multiplication overflow 시 `base`로 되돌아가며, 큰 attempt 또는 큰 base 값에서 timeout/backoff가 짧아질 수 있다.

## 계획 스냅샷

- 목표: P2P dial/pull/push retry의 exponential duration 계산이 overflow에서도 cap 또는 `Duration::MAX`로 포화되도록 맞춘다.
- 범위: `src/p2p.rs`의 retry duration helper와 해당 회귀 테스트, 작업 증적 문서만 수정한다.
- 검증 명령: `cargo test p2p::tests::exp_duration --workspace`, `cargo fmt --all --check`, `scripts/run-manifest-checks.sh --mode quick --work-id p2p-retry-duration-saturation`.
- 완료 기준: overflow 회귀 테스트가 기존 실패를 재현한 뒤 수정 후 통과하고, quick manifest 게이트가 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test p2p::tests::exp_duration --workspace` | P2P retry duration overflow 회귀 테스트 추가 및 실패 재현 |
| C2 | done | codex | `cargo test p2p::tests::exp_duration --workspace` | overflow fallback을 cap/`Duration::MAX` 포화 정책으로 수정 |
| C3 | done | codex | `cargo fmt --all --check` | 포맷 검증 |
| C4 | done | codex | `scripts/run-manifest-checks.sh --mode quick --work-id p2p-retry-duration-saturation` | 작업 todo 및 저장소 quick gate 검증 |
| C5 | todo | codex | `scripts/finalize-and-push.sh --message "docs: close p2p retry duration todo" --work-id p2p-retry-duration-saturation` | 구현 커밋 후 lessons log와 todo 삭제로 마감 |

## 완료/미완료/다음 액션

- 완료: C1 실패 재현 확인(`cargo test p2p::tests::exp_duration --workspace` 실패: overflow 시 base로 되돌아감), C2 수정 후 동일 명령 통과, C3/C4 검증 통과.
- 미완료: C5.
- 다음 액션: 구현 단위 커밋/푸시를 진행한 뒤 todo 마감 커밋에서 lessons log와 todo 삭제를 정리한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-p2p-retry-duration-saturation`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-p2p-retry-duration-saturation/open-questions.md`, 실패 재현 `cargo test p2p::tests::exp_duration --workspace`, 수정 후 `cargo test p2p::tests::exp_duration --workspace`, `cargo fmt --all --check`, `scripts/run-manifest-checks.sh --mode quick --work-id p2p-retry-duration-saturation`.
