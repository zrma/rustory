# P2P Sync (PoC)

이 문서는 P2P 운영 흐름, 관찰 지점, 트러블슈팅 경로를 안내한다.
현재 protocol id, codec/압축, message size limit, transport stack, CLI default는 `src/p2p.rs`, `src/p2p_codec.rs`, `src/cli.rs`, `Cargo.toml`, CLI help를 직접 확인한다.

## 범위
- 단계 1: 수동 multiaddr로 피어를 지정해 pull 기반 동기화를 수행한다.
- 단계 2: tracker/relay(디스커버리 + 중계) 기반으로 peer 목록을 얻고,
  - peer가 tracker/peerbook에 relay circuit 주소를 광고했으면 configured relay 주소로 circuit을 우선 시도하고,
  - relay reservation이 없는 peer는 configured relay를 억지로 붙여 dial하지 않으며,
  - public direct 후보가 있는 경우에만 direct dial 후보로 사용한다.

## 운영 가이드(PoC vs 안정화)
- PoC 단계에서는 tracker/relay를 로컬/임시로 띄워도 된다. 내려가면 동기화가 지연될 뿐이고, 로컬 DB가 source of truth라 데이터 유실은 없다.
- 안정화 이후에는 tracker/relay를 상시 실행하는 것을 권장한다(예: self-hosted k8s 1 replica). relay는 PeerId 고정을 위해 identity key(`~/.config/rustory/relay.key`)를 영속화(PV/Secret 마운트)하는 편이 안전하다.

## 프로토콜 확인 위치
- pull/push request-response는 압축 프로토콜을 우선하고 plain JSON fallback을 유지한다.
- 정확한 protocol id, request/response struct, wire/decoded size limit, 압축 선택 규칙은 `src/p2p.rs`와 `src/p2p_codec.rs`가 소유한다.
- transport stack과 libp2p feature 조합은 `src/p2p.rs`와 `Cargo.toml`에서 확인한다.
- payload size 오류나 batch 축소 동작은 `src/p2p_codec.rs`, `src/sync.rs`, 관련 테스트를 직접 확인한다.

## 사용 예시
### 단계 2: tracker/relay + PSK(pnet) 기반
실사용 디바이스는 보통 두 역할을 함께 띄운다.
- `rr p2p-serve`: tracker 등록 + inbound pull/push 응답
- `rr p2p-sync --watch --push`: tracker/peerbook에서 peer를 찾아 주기적으로 pull/push 수행

`p2p-sync`만 실행하면 이 디바이스는 tracker에 등록되지 않고, 다른 디바이스가 이 디바이스로 pull/push할 수 없다.
또한 inbound P2P pull/push는 요청자의 PeerId가 `peer_book` 또는 tracker의 같은 user scope에 있어야 통과한다.
실사용에서는 `p2p-serve`와 `p2p-sync`가 같은 `p2p_identity_key_path`를 쓰는 `rr daemon` 경로를 기본으로 둔다.

#### 1) Relay 서버
```sh
rr relay-serve --listen /ip4/0.0.0.0/tcp/4001
```

실행하면 다음 형태의 주소를 출력한다.
- `relay listen: /ip4/<ip>/tcp/<port>/p2p/<relay_peer_id>`

`rr relay-serve`는 Rustory private swarm의 multi-machine backfill을 daily-driver 기본값으로 보고
libp2p relay 기본값보다 큰 circuit/reservation/byte limit을 사용한다. 현재 값과 override 플래그는
`rr relay-serve --help`와 `src/p2p.rs`가 소유한다. 시작 로그에는 `relay config: ...`가 출력되므로,
운영 중 `Remote reported resource limit exceeded`가 반복되면 실제 relay 프로세스가 새 limit으로
재시작됐는지 이 로그부터 확인한다.

P2P relay 주소는 `/ip4/...`, `/ip6/...`, `/dns4/...`, `/dns6/...` 형태로 넘길 수 있다.
공개 relay는 `/dns4/rustory-relay.example.com/tcp/4001/p2p/<relay_peer_id>`처럼 DNS multiaddr을
쓰는 편이 운영상 안전하다. `/ip4/100.64.0.0/10`, RFC1918 private IP, loopback 같은 literal relay
주소는 같은 tailnet/LAN 밖의 peer가 dial할 수 없으므로 `rr doctor`가 경고한다.
tracker URL은 일반 URL이므로 DNS 호스트명을 그대로 사용할 수 있다.

#### 2) Tracker 서버
```sh
RUSTORY_TRACKER_TOKEN="secret" rr tracker-serve --bind 0.0.0.0:8850 --ttl-sec 60
```

토큰 없이 tracker를 띄우려면 loopback bind를 쓰거나 명시적으로 unsafe opt-in을 해야 한다.
```sh
rr tracker-serve --bind 127.0.0.1:8850 --ttl-sec 60
rr tracker-serve --bind 0.0.0.0:8850 --ttl-sec 60 --allow-unauthenticated
```

운영 서비스에서는 토큰이 process args에 남지 않도록 `RUSTORY_TRACKER_TOKEN` 또는
`config.toml`의 `tracker_token`을 우선한다. `--token`은 임시 실행/테스트용으로만 쓴다.

#### 3) Peer A (서버 역할)
```sh
rr --db-path "/tmp/rustory-a.db" p2p-serve \
  --listen /ip4/0.0.0.0/tcp/8845 \
  --trackers "http://127.0.0.1:8850" \
  --relay "/dns4/<relay-host>/tcp/4001/p2p/<relay_peer_id>"
```

#### 4) Peer B (클라이언트 역할)
```sh
rr --db-path "/tmp/rustory-b.db" p2p-sync \
  --trackers "http://127.0.0.1:8850" \
  --relay "/dns4/<relay-host>/tcp/4001/p2p/<relay_peer_id>" \
  --limit 1000
```

`--peers`를 생략하면 tracker에서 peer 목록을 받아 동기화한다.
tracker가 relay circuit 주소를 광고한 peer는 현재 sync 실행 환경에서 지정한 relay 주소로 다시 구성해 dial한다.
이렇게 해야 tracker에 저장된 Docker/LAN/private IP가 현재 머신에서 직접 dial 불가능해도 같은 relay PeerId를 통해 연결할 수 있다.
peer가 relay circuit 주소를 광고하지 않았고 public direct 후보도 없으면 그 peer는 이번 tick에서 건너뛴다.
이는 relay에 reservation이 없는 destination을 계속 dial해 `Relay has no reservation for destination`으로 tick을 소모하는 것을 막기 위함이다.
loopback/private/link-local 같은 주소는 tracker 광고와 blind direct dial 후보에서 제외한다.
pull/push request-response도 timeout/connection closed 같은 일시 오류에 대해 재시도할 수 있다.
현재 재시도 횟수, 타임아웃, 백오프 default는 `rr p2p-sync --help`, config resolver, 관련 코드를 확인한다.
- CLI: `--req-attempts`, `--req-timeout-base-sec`, `--req-timeout-cap-sec`, `--req-backoff-base-ms`
- config.toml: `p2p_request_attempts`, `p2p_request_timeout_base_sec`, `p2p_request_timeout_cap_sec`, `p2p_request_backoff_base_ms`
- env: `RUSTORY_P2P_REQUEST_ATTEMPTS`, `RUSTORY_P2P_REQUEST_TIMEOUT_BASE_SEC`, `RUSTORY_P2P_REQUEST_TIMEOUT_CAP_SEC`, `RUSTORY_P2P_REQUEST_BACKOFF_BASE_MS`

주기적으로 동기화를 계속 돌리려면 `--watch --interval-sec 60` 옵션을 사용한다.
여러 디바이스에서 같은 `--interval-sec`으로 동시에 데몬을 띄우면 요청이 몰릴 수 있으니,
시작 시점을 흩뿌리려면 `--start-jitter-sec 10` 같은 옵션을 함께 쓰는 것을 권장한다.
tracker에서 발견한 모든 peer를 매 tick마다 동시에 dial하면 작은 relay에서 resource limit에 걸릴 수 있다.
`--max-peers-per-tick <n>`으로 한 tick에 시도할 tracker-discovered peer 수를 제한할 수 있으며,
`0`은 제한 없음이다. 수동 `--peers` 대상은 명시적 운영 의도이므로 이 제한을 적용하지 않는다.
`rr daemon`은 daily-driver backfill 수렴을 우선해 기본값으로 모든 tracker-discovered peer를 시도한다.
작은 relay에서 fan-out을 줄여야 하면 `--max-peers-per-tick <n>`을 명시한다.

pull뿐 아니라 로컬 신규 엔트리를 peer로 업로드(push)하려면 `--push`를 켠다.
이때 push는 **현재 디바이스의 엔트리만** 전송한다(`entry.device_id == local_device_id`).
push 커서는 `peer_push_state.last_pushed_seq`(로컬 ingest_seq)로 저장해 재시작해도 이어서 진행한다.
push 응답(ack)에는 (가능하면) `inserted`/`ignored` 카운트가 포함되어, 중복/삽입 여부를 관측할 수 있다.

동기화 중에는 peer별로 요약 로그가 1줄씩 출력될 수 있다(의미가 있을 때만 출력).
- pull: `p2p pull summary: <peer>: received=<n> inserted=<n> ignored=<n>`
- push: `p2p push summary: <peer>: sent=<n> inserted=<n> ignored=<n>`

`rr p2p-serve`는 relay circuit listen 주소와 libp2p가 발견한 public **external address candidate**(상대가 dial 가능할 수 있는 후보 주소)를 tracker에 등록한다.
loopback/private/listen-only 주소는 tracker에 광고하지 않는다.
따라서 relay reservation이 잡힌 peer는 relay 경로로, public direct fallback 또는 DCUtR 업그레이드가 가능한 환경에서는 direct 경로도 활용할 수 있다.

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
- 기본 경로와 자동 생성 동작은 `rr init`, `rr swarm-key --help`, `rr doctor`, 관련 코드를 확인한다.
- 서로 다른 머신에서 통신하려면 **같은 키 파일을 공유**해야 한다.
- 오버라이드는 `--swarm-key <path>` 또는 `RUSTORY_SWARM_KEY_PATH`로 한다.
- 키가 동일한지 빠르게 확인하려면 `rr swarm-key`로 fingerprint를 비교한다.

## Identity Keypair(PeerId)
- `rr p2p-serve`는 libp2p identity keypair를 디스크에 영속화하여 **재시작해도 PeerId가 유지**되게 한다.
  - 현재 기본 경로는 `rr p2p-serve --help`, `rr doctor`, config template에서 확인한다.
  - 오버라이드: `--identity-key <path>`, `RUSTORY_P2P_IDENTITY_KEY_PATH`, `config.toml`의 `p2p_identity_key_path`
- `rr p2p-sync`도 같은 identity resolver를 사용한다. 서버는 inbound request의 libp2p PeerId를 `peer_book`/tracker metadata와 대조하고, push entry의 `user_id`/`device_id`가 그 PeerId에 묶인 값과 다르면 거부한다.
- `rr relay-serve`도 relay 전용 identity keypair를 별도로 영속화한다.
  - 현재 기본 경로는 `rr relay-serve --help`, `rr doctor`, config template에서 확인한다.
  - 오버라이드: `--identity-key <path>`, `RUSTORY_RELAY_IDENTITY_KEY_PATH`, `config.toml`의 `relay_identity_key_path`

## 커서 저장
- 동기화 커서는 `peer_state.last_cursor`에 저장한다.
- key(`peer_state.peer_id`)는 **상대 피어의 `PeerId` 문자열**을 사용한다.
  - 단계 1에서 저장한 multiaddr 키는, 수동 `--peers` 동기화 시 1회 마이그레이션된다.

## 설정 파일(config.toml)
- `~/.config/rustory/config.toml`로 runtime 설정을 지속화할 수 있다. 현재 fallback 순서와 default는 `rr doctor`, CLI help, config resolver 코드를 확인한다.
- 신규 디바이스에서는 `rr init`로 템플릿/키 파일을 먼저 준비하는 것을 권장한다.
- 예시:
```toml
db_path = "~/.rustory/history.db"
user_id = "zrma"
device_id = "macbook"
trackers = ["http://127.0.0.1:8850"]
relay_addr = "/dns4/<relay-host>/tcp/4001/p2p/<relay_peer_id>"
swarm_key_path = "~/.config/rustory/swarm.key"
p2p_identity_key_path = "~/.config/rustory/identity.key"
relay_identity_key_path = "~/.config/rustory/relay.key"
tracker_token = "secret"
p2p_watch_start_jitter_sec = 10
```

## peerbook 캐시(tracker fallback)
- `rr p2p-sync`는 tracker 조회가 성공하면, 받은 peer 목록을 로컬 DB에 캐시한다(`peer_book`).
- tracker가 일시적으로 다운되거나 결과가 비어 있으면, 최근에 본 peer 캐시를 기반으로 동기화를 시도한다.
- 기본 보존 기간은 `rr p2p-sync --help`, config resolver, 관련 코드를 확인한다.
  - `user_id`가 설정된 경우 같은 user의 peer만 사용한다.
- tracker 조회/등록은 일시적인 네트워크 오류와 재시도 가능한 HTTP 응답을 재시도한다. 현재 retry 분류와 횟수/default는 관련 코드와 CLI help를 확인한다.

## 트러블슈팅
- `rr doctor`: 이 머신에서 해석된 설정/키/트래커/릴레이 상태를 요약해서 출력한다.
  - config 파싱 실패, hook 설치/비활성화, async upload/auto prune 주기, key 파일 상태, tracker/relay 접근성을 한 번에 점검하는 시작점으로 사용한다.
  - `rr doctor --auto-fix`는 config/db/key 경로의 private permission과 누락된 기본 secret-filter regex처럼 안전하게 자동 보정 가능한 로컬 hygiene만 고친다.
  - 텍스트/JSON 출력 필드와 오류 표시는 `rr doctor --help`, `rr doctor --json`, 관련 코드가 소유한다.
- `rr sync-status [--peer <peer_id>] [--json] [--with-tracker]`: 로컬/피어별 동기화 상태와 tracker 접근성을 점검하는 시작점이다.
  - `outbound_push_pending`은 이 디바이스에서 해당 peer로 아직 push 커서가 전진하지 않은 로컬 엔트리 수다. 기존 스크립트 호환을 위해 `pending_push`도 같은 값을 유지한다.
  - `--watch`는 alternate screen TUI로 tracker 상태, 로컬 outbox 요약, 큐가 남은 peer, peer별 `direct`/`sent`/`to_send`/`drain/s`를 계속 갱신한다.
  - watch 화면의 `direct`는 이 노드가 해당 peer에서 직접 pull 완료한 cursor다. 이 값이 `0`이어도 inbound push나 다른 peer 경유 전파로 데이터가 들어올 수 있으므로 단독으로 데이터 유실 신호로 보지 않는다.
  - watch 화면의 `sent`는 이 노드의 로컬 엔트리를 해당 peer가 받아들인 push cursor이고, `to_send`는 아직 해당 peer에 수락되지 않은 로컬 엔트리 수다.
  - 다른 peer끼리 실제로 주고받는 global active flow는 아직 원격 daemon telemetry가 없으므로 fake mesh graph로 추정하지 않는다.
  - 현재 출력 필드, JSON 스키마, tracker ping 방식, peer cache fallback 표시는 `rr sync-status --help`와 관련 코드가 소유한다.
  - 예시:
    - `rr sync-status`
    - `rr sync-status --peer 12D3KooW...`
    - `rr sync-status --json`
    - `rr sync-status --with-tracker`
    - `rr sync-status --json --with-tracker`
    - `rr sync-status --watch --with-tracker`

## Docker 기반 수용 테스트(macOS host + Linux container)
루프백만으로는 NAT/프로세스 경계 이슈(특히 relay fallback)가 잘 안 잡힐 수 있어,
Docker Desktop을 이용해 macOS host + Linux 컨테이너 조합으로 최소 수용 테스트를 제공한다.

- 반복 검증 경로: `scripts/check.sh --acceptance`
- smoke는 생략하고 Docker acceptance만 더 보고 싶으면: `scripts/check.sh --fast --acceptance`
- 원커맨드: `bash scripts/acceptance_docker_macos_linux.sh`
- 절차 문서: `docs/acceptance/docker-macos-linux.md`

## Docker 기반 수용 테스트(two peer relay-only)
실사용에 가까운 NAT/공유기 분리 조건은 두 peer 사이 direct 경로가 없어야 검증된다.
`scripts/acceptance_docker_two_peer_relay.sh`는 `peer-a`와 `peer-b`를 서로 다른 Docker network에 분리하고,
tracker/relay만 양쪽 network에 붙여 relay circuit 없이는 수렴할 수 없는 구성을 만든다.

- 전체 acceptance 경로: `scripts/check.sh --acceptance`
- relay-only 시나리오만 실행: `bash scripts/acceptance_docker_two_peer_relay.sh`
- 절차 문서: `docs/acceptance/docker-two-peer-relay.md`
