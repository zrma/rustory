# P2P 단계 2 Spec: tracker/relay (디스커버리 + 중계)

## 목표
- 수동 multiaddr 입력 없이도 peer를 발견하고(p2p tracker),
- 직접 연결이 어려운 네트워크 환경에서도 동기화가 가능하게 한다(p2p relay).
- 기존 동기화 모델(append + dedup + cursor=ingest_seq)을 유지한다.

## 현재 상태(단계 1)
- `rr p2p-serve`/`rr p2p-sync`가 수동 multiaddr로 pull 동기화 가능
- 전송: libp2p tcp + Noise + Yamux
- 프로토콜: request-response(JSON) `/rustory/sync-pull/1.0.0`

## 범위
- tracker 서비스(서버): peer 등록/조회 API 제공
- relay 서비스(서버): **libp2p circuit relay v2** 기반의 중계 제공
- peer(클라이언트):
  - tracker에 등록/갱신(heartbeat/TTL)
  - tracker로부터 peer 목록을 받아 dial 전략 결정
  - **relay 우선(relay-first)** 으로 dial 시도(직접 연결 최적화는 후속)
- 보안/멤버십:
  - **PSK(private network, pnet)** 를 이번 단계에 포함한다
  - tracker는 옵션으로 토큰 기반 인증을 추가할 수 있다(추가 방어선)
- 설정: `~/.config/rustory/config.toml`을 이번 단계에 도입한다(없으면 CLI/env 폴백)

## 비목표(이번 단계에서 하지 않음)
- NAT hole punching / dcutr
- push 기반 동기화(업로드 최적화)
- 사용자 계정/로그인 시스템
- 저장 암호화(E2EE)

## 아키텍처 개요
### Tracker
- peer가 주기적으로 자신의 연결 정보(예: multiaddr, peer_id)를 등록한다.
- 클라이언트는 tracker에서 peer 목록을 조회해 sync 대상 후보를 얻는다.
- tracker 다운 시:
  - 신규 발견이 늦어질 수 있으나, 로컬 DB가 source of truth이므로 데이터 유실은 없다.

### Relay
- direct dial이 실패하거나 불안정할 때, relay 경유 주소로 dial을 시도한다.
- PoC에서는 **relay 우선(relay-first)** 으로 단순화한다.

## Tracker API(초안)
**HTTP 기반(구현 단순화)** 으로 시작한다(이번 결정).
- `POST /api/v1/peers/register`
  - body: `{ peer_id, addrs: [multiaddr], meta?: { device_id, hostname, user_id, version } }`
  - 응답: `{ ok: true, ttl_sec }`
- `GET /api/v1/peers`
  - query: `user_id=<...>` (옵션)
  - 응답: `{ peers: [ { peer_id, addrs, meta, last_seen } ] }`

## Relay 구성(초안)
- (결정) libp2p circuit relay v2 서버를 k8s에 배치
- peer는 relay 주소를 알고 있어야 하며, relay 경유용 multiaddr(`/p2p-circuit`)를 구성한다.

## Dial 전략(초안)
1. tracker에서 받은 peer 정보로 **relay 경유 주소(/p2p-circuit)** 를 구성해 dial 시도
2. (후속) direct 최적화가 필요하면 direct 우선 또는 fallback 전략을 추가
3. 연결되면 `SyncPull` request/response로 pull 반복

## peer_state 키 정책
- `peer_state.peer_id`에는 **상대 `PeerId` 문자열**을 저장한다(이번 결정).
- multiaddr은 tracker에서 최신 값을 받아 dial에만 사용한다(주소 변경 대응).

## 테스트 전략
- tracker: in-memory 또는 loopback으로 register/list e2e 테스트
- relay/direct: 최소 2 peer가 tracker를 통해 발견하고 1회 동기화하는 e2e 테스트
- 기존 기준선 유지: `cargo fmt/test/clippy -D warnings`
