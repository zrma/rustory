# Spec: p2p-queue-recovery

## 배경

- 요청 맥락: 일부 peer가 `queued`에 머물며 동기화되지 않는다는 운영 관찰을 재현하고 원인을 수정한 뒤 patch release를 재배포한다.
- 현재 문제/기회: `queued`는 local outbound cursor 뒤의 row/deletion이 있을 때 표시된다. live probe에서 새 entry와 deletion 모두 11개 peer queue 중 5개가 140초 뒤에도 남았고, 남은 peer를 개별 실행하면 대부분 즉시 수렴해 느린 peer의 직렬 timeout 누적과 stale relay 광고를 함께 확인했다.

## 계획 스냅샷

- 목표: relay reservation 단절 후에도 queue가 bounded time 안에 자동 회복하도록 만들고, UI와 status evidence가 실제 전송 진행/정체를 정확히 구분하게 한다.
- 범위: P2P relay 연결·재시도와 sync queue 판정 경로, 회귀 테스트, patch version metadata, 작업 패킷과 일반화 가능한 lesson만 변경한다.
- 비범위: tracker API·membership 모델·sync cursor 의미 변경, 신규 dependency, private 배포 inventory 기록.
- 검증 명령: focused P2P/watch 테스트, `scripts/check-release-gates.sh --manifest-mode full --work-id p2p-queue-recovery`, 임시 entry와 deletion의 live queue drain probe.
- 완료 기준: 실패를 재현하는 테스트가 수정 전 실패하고 수정 후 통과하며, full gate와 동일 SHA CI 후 공개 asset 및 local/관리 대상 runtime에서 version·doctor·tracker·entry/delete pending 수렴을 확인한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | live entry/delete queue drain probe | queued 관찰과 relay reservation 오류의 인과·회복 시간 재현 |
| C2 | done | codex | focused P2P/watch tests | 재현된 relay/queue 회복 결함에 대한 최소 수정과 회귀 테스트 |
| C3 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id p2p-queue-recovery` | canonical/full source gate와 공개 경계 검증 |
| C4 | in_progress | codex | `scripts/release-version.sh --profile daily-driver` | patch release 자산·checksum·build identity·GLIBC 출고 |
| C5 | todo | codex | `rr version && rr doctor && rr sync-status --json --with-tracker` | local canary와 관리 대상 runtime 순차 재배포·수렴 검증 |

## 완료/미완료/다음 액션

- 완료: C1-C3. live entry/deletion probe와 개별 peer 재시도로 순차 head-of-line blocking을 재현했다. tracked relay listener가 빈 주소 목록으로 닫혀도 stale circuit을 철회하며, 네 개 stalled peer 뒤의 healthy peer가 한 timeout budget 안에 진행되는 회귀 테스트와 canonical full gate가 통과했다.
- 미완료: C4-C5.
- 다음 액션: 검증된 patch revision을 원격에 반영하고 동일 SHA CI를 확인한 뒤 daily-driver asset을 게시해 local canary와 관리 대상에 순차 배포한다.
- 검증 증거: `cargo test p2p::tests`, `cargo clippy --all-targets --all-features -- -D warnings`, `scripts/smoke_p2p_local.sh`, `scripts/check-release-gates.sh --manifest-mode full --work-id p2p-queue-recovery`.
