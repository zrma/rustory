# P2P Sync (PoC)

## 범위
- 단계 1: 수동 multiaddr로 피어를 지정해 pull 기반 동기화를 수행한다.
- 단계 2: tracker/relay(디스커버리 + 중계) 기반으로 peer 목록을 얻고,
  - direct 연결을 우선 시도하고(direct-first),
  - 실패 시 relay로 fallback 한다.

## 프로토콜
- protocol id: `/rustory/sync-pull/1.0.0`
- request: `SyncPull { cursor, limit }`
- response: `SyncBatch { entries, next_cursor }`
- push protocol id: `/rustory/entries-push/1.0.0`
- request: `EntriesPush { entries }`
- response: `PushAck { ok }`
- 직렬화: JSON(serde_json)
- 전송: libp2p tcp + Noise + Yamux (+ pnet/relay)
- 메시지 크기 상한(초안): pull req 64KiB, pull resp 32MiB, push req 16MiB, push resp 64KiB. 초과 시 `message/request too large` 에러가 날 수 있으며, 이 경우 sync는 `limit`을 자동으로 줄여 재시도한다(단, 단일 엔트리가 너무 큰 경우는 실패할 수 있으니 필요하면 `--limit`을 조정한다).

## 사용 예시
### 단계 2: tracker/relay + PSK(pnet) 기반
#### 1) Relay 서버
```sh
rr relay-serve --listen /ip4/0.0.0.0/tcp/4001
```

실행하면 다음 형태의 주소를 출력한다.
- `relay listen: /ip4/<ip>/tcp/<port>/p2p/<relay_peer_id>`

#### 2) Tracker 서버
```sh
rr tracker-serve --bind 0.0.0.0:8850 --ttl-sec 60
```

토큰을 쓰려면:
```sh
rr tracker-serve --bind 0.0.0.0:8850 --ttl-sec 60 --token "secret"
```

#### 3) Peer A (서버 역할)
```sh
rr --db-path "/tmp/rustory-a.db" p2p-serve \
  --listen /ip4/0.0.0.0/tcp/8845 \
  --trackers "http://127.0.0.1:8850" \
  --relay "/ip4/127.0.0.1/tcp/4001/p2p/<relay_peer_id>"
```

#### 4) Peer B (클라이언트 역할)
```sh
rr --db-path "/tmp/rustory-b.db" p2p-sync \
  --trackers "http://127.0.0.1:8850" \
  --relay "/ip4/127.0.0.1/tcp/4001/p2p/<relay_peer_id>" \
  --limit 1000
```

`--peers`를 생략하면 tracker에서 peer 목록을 받아 동기화한다.
이때 tracker가 가진 peer의 `addrs`를 direct 후보로 먼저 시도하고, 실패하면 `--relay`로 relay 경유 dial을 시도한다(각 단계는 지수 backoff로 최대 3회 재시도).

주기적으로 동기화를 계속 돌리려면 `--watch --interval-sec 60` 옵션을 사용한다.

pull뿐 아니라 로컬 신규 엔트리를 peer로 업로드(push)하려면 `--push`를 켠다.
이때 push는 **현재 디바이스의 엔트리만** 전송한다(`entry.device_id == local_device_id`).
push 커서는 `peer_push_state.last_pushed_seq`(로컬 ingest_seq)로 저장해 재시작해도 이어서 진행한다.

`rr p2p-serve`는 listen 주소뿐 아니라 libp2p가 발견한 **external address candidate**(상대가 dial 가능할 수 있는 후보 주소)도 tracker에 같이 등록한다.
따라서 같은 LAN/같은 네트워크 등에서 direct-first 성공 확률이 올라간다.

## Hole Punching(DCUtR)
- relay 경유로 연결이 수립되면(libp2p `/p2p-circuit`), **가능하면 direct 연결로 업그레이드**(hole punching)한다.
- 업그레이드 성공/실패는 로그로 확인할 수 있다.
  - 성공 예: `dcutr: upgraded to direct: peer=<peer_id> connection_id=<...>`
  - 실패 예: `dcutr: upgrade failed: peer=<peer_id> error=<...>`
- 업그레이드가 실패해도 에러로 종료하지 않고, 기존처럼 relay 연결로 동기화를 계속한다.

### 단계 1: 수동 multiaddr (legacy)
#### Peer A (서버 역할)
```sh
rr --db-path "/tmp/rustory-a.db" p2p-serve --listen /ip4/0.0.0.0/tcp/8845
```

실행하면 다음 형태의 주소를 출력한다.
- `p2p listen: /ip4/<ip>/tcp/<port>/p2p/<peer_id>`

#### Peer B (클라이언트 역할)
```sh
rr --db-path "/tmp/rustory-b.db" p2p-sync --peers "/ip4/127.0.0.1/tcp/8845/p2p/<peer_id>" --limit 1000
```

## PSK(pnet) 키(swarm.key)
- p2p/relay 관련 명령은 `swarm.key`를 사용해 private network(pnet)로 통신한다.
- 기본 경로는 `~/.config/rustory/swarm.key` 이고, 없으면 자동 생성된다.
- 서로 다른 머신에서 통신하려면 **같은 키 파일을 공유**해야 한다.
- 오버라이드는 `--swarm-key <path>` 또는 `RUSTORY_SWARM_KEY_PATH`로 한다.
- 키가 동일한지 빠르게 확인하려면 `rr swarm-key`로 fingerprint를 비교한다.

## Identity Keypair(PeerId)
- `rr p2p-serve`는 libp2p identity keypair를 디스크에 영속화하여 **재시작해도 PeerId가 유지**되게 한다.
  - 기본 경로: `~/.config/rustory/identity.key`
  - 오버라이드: `--identity-key <path>`, `RUSTORY_P2P_IDENTITY_KEY_PATH`, `config.toml`의 `p2p_identity_key_path`
- `rr relay-serve`도 relay 전용 identity keypair를 별도로 영속화한다.
  - 기본 경로: `~/.config/rustory/relay.key`
  - 오버라이드: `--identity-key <path>`, `RUSTORY_RELAY_IDENTITY_KEY_PATH`, `config.toml`의 `relay_identity_key_path`

## 커서 저장
- 동기화 커서는 `peer_state.last_cursor`에 저장한다.
- key(`peer_state.peer_id`)는 **상대 피어의 `PeerId` 문자열**을 사용한다.
  - 단계 1에서 저장한 multiaddr 키는, 수동 `--peers` 동기화 시 1회 마이그레이션된다.

## 설정 파일(config.toml)
- `~/.config/rustory/config.toml`로 기본값을 설정할 수 있다(없으면 CLI/env 폴백).
- 예시:
```toml
db_path = "~/.rustory/history.db"
user_id = "zrma"
device_id = "macbook"
trackers = ["http://127.0.0.1:8850"]
relay_addr = "/ip4/127.0.0.1/tcp/4001/p2p/<relay_peer_id>"
swarm_key_path = "~/.config/rustory/swarm.key"
p2p_identity_key_path = "~/.config/rustory/identity.key"
relay_identity_key_path = "~/.config/rustory/relay.key"
tracker_token = "secret"
```

## peerbook 캐시(tracker fallback)
- `rr p2p-sync`는 tracker 조회가 성공하면, 받은 peer 목록을 로컬 DB에 캐시한다(`peer_book`).
- tracker가 일시적으로 다운되거나 결과가 비어 있으면, 최근에 본 peer 캐시를 기반으로 동기화를 시도한다.
  - 기본 보존 기간: `7d`
  - `user_id`가 설정된 경우 같은 user의 peer만 사용한다.
- tracker 조회/등록은 일시적인 네트워크 오류(transport error) 및 5xx/429/408에 대해 최대 3회 재시도한다(connect/read timeout은 attempt마다 지수 증가).
