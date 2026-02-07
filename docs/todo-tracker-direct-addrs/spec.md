# Tracker Direct Addr Accumulation

## 문제
`rr p2p-serve`는 tracker에 자기 `addrs`를 등록한다.
현재 등록되는 주소는 주로 `SwarmEvent::NewListenAddr` 기반인데,
- `--listen /ip4/0.0.0.0/tcp/0` 같은 설정에서는 tracker에 `0.0.0.0` 리슨 주소가 올라가고(상대가 dial 불가)
- relay listen 주소(`/p2p-circuit`)는 direct dial 후보가 아니다.

그 결과 tracker 기반 동기화에서 direct 후보가 비어 있거나 품질이 낮아, 거의 항상 relay dial로만 시작한다.

## 목표
- `rr p2p-serve`가 **dial 가능한 direct 후보 주소**를 tracker 등록 `addrs`에 축적한다.
- 최소 구현으로 libp2p가 제공하는 external addr candidate 이벤트를 활용한다.

## 접근(권장)
- `SwarmEvent::NewExternalAddrCandidate { address }` 및 `SwarmEvent::ExternalAddrConfirmed { address }` 를 수신하면,
  - (필터) `0.0.0.0/::` 또는 `p2p-circuit` 포함 주소는 제외
  - `/p2p/<local_peer_id>` suffix를 보장(기존 정책과 동일)
  - `known_addrs`에 추가하고 tracker에 즉시 재등록한다.

참고: identify behaviour는 outbound ephemeral port 문제를 완화하기 위해 candidate address에 포트 번역을 적용할 수 있으므로,
`identify::Event::Received.info.observed_addr`를 직접 쓰는 것보다 swarm external addr candidate을 우선 사용한다.

## 범위
- `src/p2p.rs`의 `serve_async` 이벤트 루프
- (필요 시) 주소 필터/정규화 helper + unit test

## 비목표
- autonat 도입 및 실제 도달성 확인
- tracker 프로토콜 변경(구조화된 addr 메타데이터 등)

## 완료 조건
- `cargo fmt/test/clippy -D warnings` 통과
- tracker 등록 addrs가 listen addr 뿐 아니라 external addr candidate도 포함할 수 있다(테스트로 최소 검증)
- todo 폴더 삭제
