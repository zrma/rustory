# P2P Sync (PoC)

## 범위
- 단계 1: 수동 multiaddr로 피어를 지정해 pull 기반 동기화를 수행한다.
- tracker/relay, NAT traversal, PSK(private network), push 동기화는 범위 밖이다.

## 프로토콜
- protocol id: `/rustory/sync-pull/1.0.0`
- request: `SyncPull { cursor, limit }`
- response: `SyncBatch { entries, next_cursor }`
- 직렬화: JSON(serde_json)
- 전송: libp2p tcp + Noise + Yamux

## 사용 예시
### Peer A (서버)
```sh
rr --db-path "/tmp/rustory-a.db" p2p-serve --listen /ip4/0.0.0.0/tcp/8845
```

실행하면 다음 형태의 주소를 출력한다.
- `p2p listen: /ip4/<ip>/tcp/<port>/p2p/<peer_id>`

### Peer B (클라이언트)
```sh
rr --db-path "/tmp/rustory-b.db" p2p-sync --peers "/ip4/127.0.0.1/tcp/8845/p2p/<peer_id>" --limit 1000
```

## 커서 저장
- 동기화 커서는 `peer_state.last_cursor`에 저장한다.
- key(`peer_state.peer_id`)는 **상대 피어의 multiaddr 문자열**을 그대로 사용한다.

