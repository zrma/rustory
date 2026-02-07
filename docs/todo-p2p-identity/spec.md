# P2P Identity Persistence (PeerId 안정화)

## 문제
현재 `rr p2p-serve` / `rr relay-serve`는 실행할 때마다 새 libp2p identity keypair를 생성한다.
그 결과 PeerId가 매번 바뀌어 다음 문제가 발생한다.
- stage1 수동 multiaddr(`.../p2p/<peer_id>`)가 재시작마다 무효화된다.
- stage2에서 tracker/peerbook의 peer_id 기반 커서(`peer_state`)가 끊겨 불필요한 풀 리싱크가 잦아진다.
- relay peer id도 바뀌어, 클라이언트 설정에 넣은 relay multiaddr이 쉽게 깨진다.

## 목표
- `rr p2p-serve`는 **디스크에 영속화된 identity keypair**로 Swarm을 생성한다(재시작해도 PeerId 고정).
- `rr relay-serve`도 별도의 identity keypair를 영속화한다(재시작해도 relay PeerId 고정).
- identity 파일이 없으면 자동 생성한다.
- 파일 권한은 가능한 OS에서 최소 권한(0600)으로 제한한다.
- 경로 해석 우선순위는 기존 정책을 따른다: CLI > env > config.toml > default.

## 범위
- config:
  - config.toml에 p2p/relay identity 경로를 추가한다.
  - env override를 추가한다.
- cli:
  - `rr p2p-serve` / `rr relay-serve`에 identity key 경로 옵션을 추가한다.
- runtime:
  - identity keypair를 load-or-generate 해서 Swarm 생성 시 사용한다.
- 테스트:
  - identity keypair가 파일에 생성되고, 재로드 시 동일 PeerId를 보장하는 테스트를 추가한다(재시작 수용 테스트 성격).

## 비목표
- key rotation/마이그레이션 정책
- key 암호화(패스프레이즈) 지원
- `rr p2p-sync`의 dialer identity 고정 (동시 실행 시 PeerId 충돌 위험이 있어, 이번 단계는 유지하지 않는다)

## 결정 사항
- p2p peer 기본 경로: `~/.config/rustory/identity.key`
- relay 기본 경로: `~/.config/rustory/relay.key`
- 파일 포맷: `libp2p::identity::Keypair`의 protobuf encoding(raw bytes)
- `rr p2p-sync`는 기존처럼 ephemeral identity를 유지한다(동시 실행 충돌 회피).

## 완료 조건
- `cargo fmt/test/clippy -D warnings` 통과
- p2p-serve / relay-serve를 재시작해도 PeerId가 유지된다.
- 문서(`docs/p2p.md` 등)에 설정/파일 경로/오버라이드를 반영하고 todo 폴더는 삭제한다.

