# Push Optimization: Prevent Echo/Gossip

## 문제
현재 `--push`는 로컬 DB에 있는 엔트리를 그대로 push한다.
이 방식은 `p2p-sync`가 먼저 pull을 수행한 뒤(push 앞), 그 pull로 인해 로컬 DB에 새로 들어온 **타 디바이스 엔트리까지** 다시 다른 peer로 push하게 되어:
- 불필요한 네트워크 트래픽(중복 전송)이 늘고
- 의도치 않은 gossip(브릿징)처럼 보일 수 있다.

## 목표
- `--push`는 **현재 디바이스에서 기록된 엔트리만** 전송한다.
  - 즉, `entry.device_id == local_device_id`인 엔트리만 push 대상이다.
- 재시작해도 이어서 동작하도록 push 커서 영속화는 유지한다.
- 로컬 스모크에서 “A/B가 서로의 엔트리를 받지 않는다(브릿징 방지)”를 검증한다.

## 범위
- P2P push(`rr p2p-sync --push`)에서 device_id 필터링을 적용한다.
- HTTP push(`rr sync --push`)에도 동일한 필터링을 적용한다(디버그 경로지만 동작 일관성 유지).
- SQLite 조회를 `device_id`로 필터링하여 불필요한 스캔을 줄인다.
- 문서(`docs/p2p.md`, `README.md`)에 `--push` 의미(로컬 디바이스만)를 명시한다.

## 비기능/운영 요구
- idempotent 유지(중복 push 안전).
- 실패 시 커서가 잘못 전진하지 않는다.
- watch 모드에서 불필요한 dial/push가 늘지 않는다(전송할 로컬 엔트리가 없으면 push는 0건).

