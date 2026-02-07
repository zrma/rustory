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
- 직렬화: JSON(serde_json)
- 전송: libp2p tcp + Noise + Yamux (+ pnet/relay)

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
tracker_token = "secret"
```

## peerbook 캐시(tracker fallback)
- `rr p2p-sync`는 tracker 조회가 성공하면, 받은 peer 목록을 로컬 DB에 캐시한다(`peer_book`).
- tracker가 일시적으로 다운되거나 결과가 비어 있으면, 최근에 본 peer 캐시를 기반으로 동기화를 시도한다.
  - 기본 보존 기간: `7d`
  - `user_id`가 설정된 경우 같은 user의 peer만 사용한다.
