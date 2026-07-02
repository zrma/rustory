# Spec: update-restarts-daemon

## 배경

- 요청 맥락: 최신 배포 뒤 외부 Linux container `samplex-x86_64`에서 `rr sync-status --json --with-tracker`의 peer별 `pending_push`가 계속 남고, daemon log에 `p2p-sync tick: selected 1/6 peers (max_peers_per_tick=1)`가 반복됐다.
- 현재 문제/기회: 현재 소스의 `rr daemon` 기본값은 모든 tracker peer를 매 tick 시도하는 `max_peers_per_tick=0`이지만, `rr update`가 byte-identical binary에서 daemon restart를 생략하면 오래 떠 있던 background daemon child가 이전 default/인자로 계속 동작할 수 있다.

## 계획 스냅샷

- 목표: `rr update`가 binary 교체 여부와 무관하게 관리 daemon을 현재 binary/default로 재시작할 수 있게 해 stale daemon child를 수동 대응 없이 복구한다.
- 범위: `src/self_update.rs`의 restart decision, 관련 regression test, `docs/daemon.md`/`docs/distribution.md`의 self-update 설명, release/rollout 증거.
- 검증 명령: `cargo test self_update --workspace`, `scripts/check.sh --fast`, `scripts/release-version.sh --profile daily-driver --gate quick`.
- 완료 기준: byte-identical update에서도 `--no-restart-daemon`이 아닌 한 restart path를 타며, release asset과 local MacBook/k8s 5개 노드가 새 버전을 실행하고 tracker가 reachable한 상태가 확인된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test self_update --workspace` | byte-identical update도 managed daemon restart decision을 유지하도록 구현하고 회귀 테스트를 추가한다. |
| C2 | done | codex | `scripts/check.sh --fast`; `cargo clippy --workspace --all-targets -- -D warnings` | fast gate와 clippy deny로 전체 Rust/문서/스크립트 회귀를 확인한다. |
| C3 | todo | codex | `scripts/release-version.sh --profile daily-driver --gate quick` | patch release를 만들고 local MacBook + k8s 5개 노드에 배포한다. |

## 완료/미완료/다음 액션

- 완료: C1, C2.
- 미완료: C3.
- 다음 액션: patch release를 발행하고 local MacBook + k8s 5개 노드에 배포한다.
- 검증 증거: `cargo test self_update --workspace` (13 passed), `scripts/check.sh --fast` (246 passed + dev build), `cargo clippy --workspace --all-targets -- -D warnings`.
