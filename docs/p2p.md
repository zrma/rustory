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

#### Device enrollment와 revoke 제어면

기본 tracker API와 config default는 기존 peer와 호환된다. durable revoke를 활성화할 때는 fleet
token과 분리된 admin token, absolute private state path를 먼저 추가한다. admin token은 peer에
배포하지 않는다.

```sh
export RUSTORY_TRACKER_TOKEN='<fleet-token>'
export RUSTORY_TRACKER_ADMIN_TOKEN="$(openssl rand -hex 32)"
export RUSTORY_TRACKER_SECURITY_STATE_PATH='/var/lib/rustory/tracker-security.json'
rr tracker-serve --bind 0.0.0.0:8850 --ttl-sec 60
```

안전한 전환 순서는 다음과 같다.

1. 새 버전을 모든 정상 node에 먼저 배포한다. 이 단계에서는 `require_device_membership=false`,
   `allow_remote_retirement=false`를 유지하고 각 node의 signed registration을 기다린다.
2. admin 환경에서 `rr device list`로 observed PeerId를 확인하고, 정확한 대상마다
   `rr device enroll --peer-id '<peer-id>'`를 실행한다.
3. tracker를 같은 state path와 `--require-device-enrollment`로 재시작한다. 이후 unsigned/unknown
   identity의 register/unregister는 shared fleet token을 알아도 거부된다.
4. 정상 node config를 `require_device_membership=true`로 바꾸고 daemon을 재시작한다. 이 모드는
   authoritative tracker가 정확히 하나이고 HTTPS여야 하며(loopback test HTTP 예외), tracker 장애
   중에는 history sync를 fail-closed한다.
5. `rr device list`에서 남을 모든 enrolled node가 현재 membership protocol과
   `membership_enforced=true`를 보고하는지 확인한다. offline인 strict-enrolled node는 준비된 것으로
   취급하지만, 현재 active인데 미등록인 legacy peer는 fleet-wide enforcement 미완료 신호다.

strict mode 전환 뒤 새 장비를 추가할 때 tracker를 non-strict로 되돌릴 필요는 없다. 새 장비의 첫 signed
registration은 sync membership에는 403으로 거부되지만 `rr device list`에는
`enrolled=false active=false` pending observation으로 최대 15분 표시된다. 관리자가 정확한 PeerId를
확인해 `rr device enroll`한 뒤 다음 registration부터 활성화된다. 이미 revoke된 `user_id + device_id`는
새 identity로 우회할 수 없으므로 교체 장비는 의도적으로 새 `device_id`를 부여해 이 절차를 따른다.

incident revoke가 다른 node의 offline 상태 때문에 막히지 않도록 tracker는 대상 membership을 즉시
durable하게 revoke한다. 다만 남을 enrolled node 중 protocol/enforcement가 부족하거나 현재 active인
미등록 legacy peer가 있으면 응답의 `membership_enforcement_complete=false`와 CLI warning으로
"tracker revoke 기록"과 "fleet 전체 강제 확인"을 분리한다. 이 warning이 나오면 legacy peer를
업데이트/enroll하고, 대상이 신뢰되지 않는 경우 shared tracker token과 `swarm.key`도 rotation한다.
`true`도 마지막으로 수락된 signed registration capability의 advisory snapshot이지 live attestation은
아니다. node가 이후 offline rollback되었는지 증명하지 않으므로 rollout/incident 때 각 node의 현재
binary version과 fresh registration 시각을 별도로 확인한다.
대상 자체는 offline이어도 revoke할 수 있으며, full-uninstall capability를 광고했던 cooperative 대상은
재접속 때 ticket을 처리한다. 동일 PeerId에 다른 cleanup 정책을 덮어쓰는 것은 거부한다. scheduling
실패나 대상의 명시적 거부는 같은 admin retire 명령으로 안전 precondition을 다시 확인한 뒤
`Pending`으로 재큐잉할 수 있다. 먼저 revoke-only로 격리한 대상은 이후 precondition이 충족되면
`rr device retire`로 새 full-uninstall ticket을 발급하는 단방향 승격이 가능하다. full-uninstall을
revoke-only로 낮추거나 이미 실행 중인 ticket의 cleanup 정책을 바꾸는 것은 거부한다.

```sh
# 한 번 생성해 tracker와 관리 CLI가 같은 secret-manager 값을 사용한다.
export RUSTORY_TRACKER_ADMIN_TOKEN="$(openssl rand -hex 32)"
rr device list
rr device enroll --peer-id '<exact-peer-id>'
rr device revoke --peer-id '<exact-peer-id>' --yes
rr device retire --peer-id '<exact-peer-id>' --yes
rr device status --peer-id '<exact-peer-id>'
```

admin 요청은 일반 tracker bearer token에 더해 admin token을 요구하고 production에서는 HTTPS로만
전송된다. loopback HTTP 허용은 repository test build에만 존재하며 운영 우회 수단이 아니다.
admin token은 fleet token과 다른 최소 32-byte random 값이어야 한다(`openssl rand -hex 32` 권장).
TLS reverse proxy에서 admin endpoint, `/api/v1/peers/register`, signed
`/api/v1/peers/unregister`, `/api/v1/devices/retirement/{poll,ack,complete}`에 peer/IP별 rate
limit을 적용한다. 정상 poll 30초와 completion retry 5~60초 cadence보다 충분히 여유 있게 두고
request/response read/write timeout을 설정하며,
backend port는 loopback/private network policy로 직접 접근을 막는다.
CLI 인자의 `--admin-token`보다 `RUSTORY_TRACKER_ADMIN_TOKEN`을 권장한다. tracker security state는
재시작 후 enrollment, revocation, cleanup ACK와 ticket-scoped completion capability hash를 복원하며
group/world-readable 파일이나 symlink를 거부한다. state file에는 process-lifetime exclusive lock을
잡으므로 같은 path를 공유하는 tracker replica는 하나만 실행한다.
대상 helper의 retirement `poll`/`ack`는 enrolled identity의 timestamp/nonce/signature proof로 인증하므로
fleet bearer token rotation 뒤에도 복구된다. 이 두 endpoint는 자신의 ticket만 조회/전이할 수 있고,
admin/list/register API의 bearer-token 요구를 완화하지 않는다.
자발적 uninstall의 signed unregister도 같은 identity proof로 자기 enrollment만 제거할 수 있어 stale
fleet token 때문에 탈퇴가 막히지 않는다. proof가 없는 legacy unregister는 기존처럼 bearer token을
요구한다.

tracker ingress는 proof/capability 요청을 bounded header에서 먼저 검증하고, 인증되지 않은 요청은 body를
읽지 않는다. legacy JSON body는 유효한 fleet bearer가 있을 때만 허용한다. 전체 body read에는 10초
deadline과 64개 동시 요청 상한을 두므로 느린 unauthenticated body 하나가 register/admin/sync 요청 전체를
멈추지 않는다. TLS reverse proxy에도 request-body/header size와 read/write timeout을 같은 수준 이하로 둔다.
`X-Rustory-Device-Request`와 해당 request body에는 signed proof 및 completion capability가 포함될 수
있으므로 access log에서 header/body를 반드시 redact하거나 기록하지 않는다. tracker 응답은
`Cache-Control: no-store, private`이므로 proxy에서 cache하지 않는다.

### Strict mode rollback 경계

wire JSON 필드는 additive라 새 client가 구 tracker의 `revocations` 없는 list 응답을 읽고 구 client가 새
list의 추가 필드를 무시하는 것은 가능하다. 그러나 strict membership을 활성화한 뒤 tracker를 이 기능
이전 버전으로 내리는 것은 보안상 호환되지 않는다. 구 tracker는 durable enrollment/revocation/ticket
state를 읽거나 강제하지 못한다.

rollback이 불가피하면 다음 순서를 지킨다.

1. device enroll/revoke/retire mutation을 중지하고 private tracker security-state 파일과 lock 경로를
   보존한다. 이 파일을 구 tracker가 이해하지 못한다고 삭제하거나 빈 파일로 대체하지 않는다.
2. 모든 full-uninstall ticket이 terminal인지, 대상 helper/receipt가 `Running` 또는 ACK retry 중이 아닌지
   확인한다. 실행 중 helper가 있으면 새 tracker/client를 유지해 완료 ACK까지 수렴시킨다.
3. 아직 current binary인 모든 정상 node에서 먼저 `allow_remote_retirement=false`, 그 다음
   `require_device_membership=false`를 config에 저장하고 daemon을 재시작한다. 새 tracker를 유지한 채
   모든 node가 non-strict registration을 다시 보고하고 더 이상 helper가 없는지 확인한다. 이 단계부터
   durable revoke의 fleet 강제를 의도적으로 포기한 상태이므로 접근을 별도 network policy로 제한한다.
4. revoke된 대상이나 credential 노출 가능성이 있으면 정상 node의 tracker token과 `swarm.key`를 먼저
   rotation한다. rollback은 이미 유출된 shared credential을 폐기하지 않는다.
5. strict enforcement를 포기한다는 운영 결정을 기록하고 current client의 strict flag가 모두 내려간
   뒤에만 tracker binary를 구 버전으로 내린다. 마지막으로 client binary를 coordinated downgrade한다.
   strict flag를 둔 채 tracker부터 내리면 current client가 membership API 404로 fail-closed하고, 구
   client를 먼저 내리면 strict tracker가 registration을 거부하므로 어느 쪽도 단독 binary downgrade로
   시작하지 않는다.
6. 재전진할 때 보존한 security state로 새 tracker를 먼저 복구하고 enrollment/enforcement coverage를
   다시 확인한 뒤 client를 올린다.

실제 fleet에서 full uninstall을 허용하기 전에는 disposable macOS/Linux node에서 별도 launchd label,
systemd-user cgroup과 reboot recovery, offline 재접속, cleanup 후 completion ACK 재시도를 각각 통과시킨다.
unit/integration gate만 통과한 build는 membership revoke에는 쓸 수 있지만 live full-uninstall enablement의
최종 증거로 간주하지 않는다.
재현 경계와 최신 acceptance evidence는 `docs/acceptance/device-retirement-vms.md`가 소유한다.

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
- push: `p2p push summary: <peer>: sent=<n> inserted=<n> ignored=<n> entry_inserted=<n> entry_ignored=<n> deletion_inserted=<n> deletion_ignored=<n> deletion_deleted=<n>`

`rr p2p-serve`는 relay circuit listen 주소와 libp2p가 발견한 public **external address candidate**(상대가 dial 가능할 수 있는 후보 주소)를 tracker에 등록한다.
loopback/private/listen-only 주소는 tracker에 광고하지 않는다.
따라서 relay reservation이 잡힌 peer는 relay 경로로, public direct fallback 또는 DCUtR 업그레이드가 가능한 환경에서는 direct 경로도 활용할 수 있다.

## Hole Punching(DCUtR)
- relay 경유로 연결이 수립되면(libp2p `/p2p-circuit`), **가능하면 direct 연결로 업그레이드**(hole punching)한다.
- 기본 daemon 로그는 sync summary와 non-retryable warning 중심으로 유지한다. relay/dial/connection/DCUtR 상세 이벤트가 필요하면 `RUSTORY_P2P_LOG=verbose`를 설정하고 daemon 또는 대상 명령을 다시 시작한다.
- verbose 모드에서는 업그레이드 성공/실패를 로그로 확인할 수 있다.
  - 성공 예: `dcutr: upgraded to direct: peer=<peer_id> connection_id=<...>`
  - 실패 예: `warn: dcutr direct upgrade failed: peer=<peer_id> error=<...>`
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
- 이 노드가 상대에게 보낸 entry/delete의 전달 커서는 `peer_push_state`와 `peer_delete_push_state`에 저장한다.
  - outbound push는 원격이 batch를 수락한 응답을 받은 뒤에만 전진한다.
  - inbound pull은 libp2p가 응답 frame을 보냈다는 transport event만으로 전진하지 않는다. 상대가 다음 pull 요청에 이전 응답의 cursor를 실어 보냈을 때 그 cursor까지 로컬 DB에 반영했다는 애플리케이션 확인으로 간주한다.
  - 최초 push/pull 시도부터 peer별 전달 상태를 `0`으로 등록하므로, 실패하거나 아직 확인되지 않은 row와 deletion tombstone은 prune/GC floor를 통과하지 않는다.
- pull 요청의 확인 cursor는 로컬 SQLite가 지금까지 할당한 sequence high-water mark를 넘을 수 없고 뒤로 이동하지 않는다. 이미 안전하게 prune/GC된 row의 cursor 확인은 허용된다.

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
require_device_membership = true
# allow_remote_retirement = false # target-side destructive opt-in
```

일반 실행에서는 `require_device_membership`과 `allow_remote_retirement`를 각각
`RUSTORY_REQUIRE_DEVICE_MEMBERSHIP`, `RUSTORY_ALLOW_REMOTE_RETIREMENT`로 override할 수 있다. 하지만
remote retirement capability는 recovery helper가 재구성할 수 있도록 두 opt-in과 `user_id`,
`device_id`, tracker/DB/key 경로가 `config.toml`에도 일치하게 저장된 경우에만 광고한다.
remote retirement를 켠 node는 두 값을 모두 true로 두고 launchd 또는 systemd-user가 관리하는
`rr daemon`으로 실행해야 capability를 광고한다. standalone `rr p2p-serve`와 Linux background
fallback은 파일 삭제 capability를 광고하지 않고 revoke-only로 남는다.

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
  - `local_head`는 하위 호환을 위해 유지하는 로컬 `ingest_seq` cursor이고 저장 row 수가 아니다. `AUTOINCREMENT` gap과 삭제 때문에 실제 row 수보다 클 수 있으며, 실제 저장량은 additive JSON 필드 `local_row_count`와 text/watch의 `stored rows`로 확인한다.
  - `outbound_push_pending`은 이 디바이스에서 해당 peer로 아직 push/delete 커서가 전진하지 않은 로컬 엔트리와 deletion tombstone의 합계다. 기존 스크립트 호환을 위해 `pending_push`도 같은 값을 유지한다.
  - JSON/text 출력의 `peer_rr_version`/`rr_version`은 peer가 tracker에 마지막으로 보고한 `rr` 버전이다. `p2p-sync`의 tracker discovery가 받은 값은 `peer_book`에 캐시되므로 이후 tracker를 조회하지 않는 상태 화면에서도 마지막 보고 값을 재사용하며, 아직 버전을 보고하지 않은 구버전/수동 peer는 unknown으로 남는다.
  - JSON/text 출력의 `outbound_push_pending_entries`, `outbound_push_pending_deletions`, `pending_push_entries`, `pending_push_deletions`, `pull_delete_cursor`, `push_delete_cursor`는 row backlog와 삭제 tombstone backlog를 분리해 보여준다.
  - deletion tombstone backlog가 0으로 안정된 뒤에만 오래된 tombstone을 `rr tombstone-gc`로 정리한다. GC는 알려진 peer의 delete push cursor floor를 넘지 않는 tombstone만 삭제한다.
  - `--watch`는 alternate screen TUI로 tracker 상태, 로컬 outbox 요약, 큐가 남은 peer, peer별 `pull_cur`/`push_cur`/`to_send`/`drain/s`를 계속 갱신한다. peer 이름 옆의 `[1.0.45]`는 마지막 보고 버전이고, `!`는 현재 로컬 `rr`보다 낮음, `+`는 높음, `?`는 unknown 또는 semantic version으로 해석할 수 없음을 뜻한다. 이는 GitHub의 최신 release를 실시간 조회한 표식이 아니라 현재 실행 중인 로컬 버전과의 상대 비교다.
  - watch 화면의 `pull_cur`는 이 노드가 해당 peer에서 직접 pull 완료한 sequence cursor다. 이 값이 `0`이어도 inbound push나 다른 peer 경유 전파로 데이터가 들어올 수 있으므로 단독으로 데이터 유실 신호로 보지 않는다.
  - watch 화면의 `push_cur`는 이 노드의 로컬 엔트리를 해당 peer가 받아들인 sequence cursor이고, `to_send`는 아직 해당 peer에 수락되지 않은 실제 로컬 엔트리와 deletion tombstone 수다.
  - watch/mesh 화면의 `idle`은 tracker heartbeat가 오래됐지만 이 노드에서 해당 peer로 보낼 row/delete backlog가 없다는 뜻이다. `stale`은 오래된 heartbeat와 남은 `to_send`가 동시에 있을 때만 표시한다.
  - tracker 등록 hostname은 `HOSTNAME`/`HOST`가 없으면 OS hostname으로 보완한다. 값을 얻지 못한 peer의 `unknown` sentinel은 실제 공유 hostname이 아니므로 active duplicate warning 대상에서 제외한다.
  - 다른 peer끼리 실제로 주고받는 global active flow는 아직 원격 daemon telemetry가 없으므로 fake mesh graph로 추정하지 않는다.
  - 현재 출력 필드, JSON 스키마, tracker ping 방식, peer cache fallback 표시는 `rr sync-status --help`와 관련 코드가 소유한다.
- `rr mesh [--watch] [--no-tracker]`: `sync-status`와 같은 로컬 cursor 데이터를 사람이 보기 쉬운 mesh dashboard로 렌더링한다.
  - 기본값은 configured tracker를 ping해서 `Outbox` 패널에 tracker health를 포함한다. 네트워크 조회를 피하려면 `--no-tracker`를 사용한다.
  - `Mesh Topology`는 braille canvas로 이 노드(local hub)와 각 peer 사이의 관측 가능한 edge, peer 상태, queue/active packet marker를 표시한다. peer 위치는 device/peer display name 오름차순으로 고정해 watch tick마다 노드가 뒤섞이지 않게 한다. peer끼리 직접 주고받는 global graph는 아직 daemon telemetry가 없으므로 그리지 않는다.
  - `Outbox`는 전체 `to_send`, queue trend sparkline, pull/push/drain rate, hot peer를 보여준다.
  - `Flow Lanes`는 topology와 같은 stable name order로 peer별 마지막 보고 `rr` 버전, `pull_cur`, `push_cur`, `to_send`, coverage를 보여준다. cursor는 sequence 위치이며 row count가 아니다. 버전 배지의 `!`/`+`/`?` 의미는 `sync-status --watch`와 같다. 상태 기반 attention sorting이 필요하면 `rr sync-status --watch --with-tracker`를 사용하고, 정밀 자동화나 JSON이 필요하면 `rr sync-status --json --with-tracker`를 사용한다.
  - 예시:
    - `rr sync-status`
    - `rr sync-status --peer 12D3KooW...`
    - `rr sync-status --json`
    - `rr sync-status --with-tracker`
    - `rr sync-status --json --with-tracker`
    - `rr sync-status --watch --with-tracker`
    - `rr mesh --watch`
    - `rr mesh --watch --no-tracker`

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
