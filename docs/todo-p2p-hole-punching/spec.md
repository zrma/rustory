# P2P NAT Traversal: Hole Punching (DCUtR) 도입

## 목표
- relay 경유로만 연결되는 환경에서, 가능하면 **direct 연결로 업그레이드**(hole punching)한다.
- 기존 보안/멤버십(Noise + pnet PSK) 및 동기화 모델(append + dedup + cursor=ingest_seq)을 유지한다.
- direct 연결이 불가능한 경우에는 기존처럼 relay 경유로 동기화가 계속되게 한다.

## 현재 상태
- relay v2 서버/클라이언트 + tracker: 구현됨
  - `/Users/zrma/code/src/rustory/src/p2p.rs`
  - 문서: `/Users/zrma/code/src/rustory/docs/p2p.md`
- dial 전략: direct-first + relay fallback + 지수 backoff 3회: 구현됨

## 범위
- `rr p2p-serve` / `rr p2p-sync`에 DCUtR를 붙여 **relay 연결 이후 direct 업그레이드**를 시도한다.
- 성공/실패/업그레이드 이벤트를 로그로 관측 가능하게 한다.

## 결정 사항
- 이번 단계는 `dcutr`만 먼저 도입한다(`autonat/upnp`는 후속).
- relay로 연결이 수립된 경우에만 dcutr 업그레이드를 시도한다.
- 업그레이드 실패는 치명적 에러로 취급하지 않고 relay 연결로 동기화를 계속한다(로그만 남김).

## 비목표
- UPnP / PCP 자동 포트 매핑(후속)
- autonat 기반의 공인/비공인 판정 및 최적화(후속)
- 멀티 주소 우선순위/성공률 기반 라우팅(후속)

## 설계 초안
### 연결 흐름
1. 기존처럼 direct-first dial 시도
2. direct가 안 되면 relay fallback으로 연결 수립
3. relay 연결이 수립된 경우, DCUtR behaviour를 통해 direct 업그레이드를 자동으로 시도
4. 업그레이드 성공 시 이후 요청은 direct 연결로 흐르도록 기대(또는 direct 우선으로 재-dial)

### 구현 포인트(초안)
- libp2p feature: `dcutr` 추가
- behaviour:
  - `relay::client::Behaviour` + `identify::Behaviour`는 이미 사용 중
  - `dcutr::Behaviour`를 추가하고, identify 이벤트/주소 정보를 적절히 연결한다(튜토리얼 기반)
- dial/connection 이벤트 핸들링:
  - `SwarmEvent::Behaviour(...)`에서 dcutr 이벤트 로깅

### 테스트 전략(초안)
- 최소: 컴파일 + clippy + 기존 e2e 테스트 유지
- 가능하면: 로컬에서 relay + 2 peer를 띄워 relay 연결 후 dcutr 업그레이드 이벤트가 발생하는지 smoke 테스트

## 완료 조건
- `cargo fmt/test/clippy -D warnings` 통과
- relay 경유 연결 상황에서 dcutr 업그레이드 시도가 확인된다(로그/이벤트)
- 문서(`/Users/zrma/code/src/rustory/docs/p2p.md`)에 동작/제약을 반영하고 todo 폴더는 삭제한다.
