# Security and Privacy Model

Last Verified: 2026-07-12

이 문서는 Rustory가 보호하는 경계와 보호하지 않는 경계를 명시한다. 암호화 구현의 source of truth는 `src/p2p.rs`, `src/transport.rs`, `src/storage.rs`, `src/tracker.rs`, `src/config.rs`다.

## 기본 신뢰 모델

Rustory daily-driver grid는 **같은 grid에 가입한 모든 peer를 신뢰하는 local-first P2P 시스템**이다.

- 각 peer는 검색을 위해 command history 원문을 local SQLite에 저장한다.
- 한 peer가 동기화한 history는 다른 authorized peer에서도 평문으로 조회할 수 있다.
- 따라서 한 peer의 사용자 권한 또는 root 권한이 침해되면 그 peer가 받은 grid history도 노출될 수 있다.
- `swarm.key`, tracker token, identity key는 가입/식별 경계이지 local DB 암호화 키가 아니다.
- 신뢰 수준이 다른 개인 머신과 서버군은 별도 `user_id`/`swarm.key` grid로 분리하는 편이 안전하다.

## 전송 경계

### P2P daily-driver 경로

- peer transport는 shared `swarm.key`를 사용하는 libp2p pnet handshake 뒤에 identity key 기반 libp2p Noise handshake를 수행한다.
- direct connection과 relay circuit 모두 peer 사이의 authenticated Noise session 안에서 pull/push payload를 교환한다.
- relay는 circuit을 전달하지만 command history를 application endpoint로 복호화하지 않는다.
- tracker는 command history를 받지 않는다. PeerId, dial address, `user_id`, `device_id`, hostname, version/build metadata, last-seen time은 tracker에 노출된다.
- production tracker URL은 HTTPS와 bearer token을 사용해야 한다.

이 보호는 P2P transport confidentiality/integrity다. 별도의 entry-level AES-GCM ciphertext를 저장하거나 전달하는 zero-knowledge backend protocol은 현재 없다.

### Debug HTTP sync 경로

`rr serve`/`rr sync`는 P2P 이전의 debug/compatibility 경로이며 entry JSON을 application-level encryption 없이 전송한다.

- loopback HTTP는 기본 허용한다.
- HTTPS peer는 허용한다.
- non-loopback plaintext HTTP client/server는 `--allow-insecure-http` 같은 명시적 opt-in 없이는 거부한다.
- `--allow-unauthenticated`는 이미 명시적인 unsafe server opt-in이므로 legacy 호환을 위해 insecure HTTP opt-in도 겸한다.
- 원격 운영 경로는 debug HTTP 대신 P2P를 사용하거나, `rr serve`를 loopback에 bind하고 TLS reverse proxy 뒤에 둔다.

## 저장 경계

- `history.db`의 command, cwd, hostname과 관련 metadata는 SQLite 평문 column이다.
- DB/config/key 파일은 private permission으로 생성하지만 같은 사용자와 root의 읽기를 막지는 못한다.
- MacBook은 FileVault, Linux node는 LUKS 등 full-disk encryption을 사용하고 plaintext backup 접근 범위를 제한한다.
- `rr delete --vacuum`은 local SQLite/WAL 흔적을 줄이지만 이미 복제된 peer, snapshot, backup의 secure erasure를 보장하지 않는다. deletion tombstone이 모든 peer에 수렴했는지 별도 확인한다.

## 민감 command 기록 방지

- bash/zsh generated hook은 원문 첫 문자가 공백인 command를 의도적인 privacy opt-out으로 보고 기록하지 않는다.
- `record_ignore_regex`/`RUSTORY_RECORD_IGNORE_REGEX`는 알려진 secret pattern을 기록/import 전에 차단하는 defense-in-depth다.
- regex는 임의의 secret 값을 완전하게 판별할 수 없으므로 앞 공백 opt-out과 secret manager 사용을 대체하지 않는다.
- 이미 저장된 민감 row는 `rr delete --cmd-regex ... --dry-run`으로 확인한 뒤 삭제하고 peer deletion pending 수렴을 확인한다.

## 키 유출과 노드 이탈

- authorized peer는 정상 기능상 history 원문을 복호화해 검색해야 하므로 같은 grid의 peer compromise를 transport 암호화로 막을 수 없다.
- `rr device revoke`는 tracker의 durable enrollment에서 정확한 PeerId membership을 박탈한다. strict-ready peer는 inbound/outbound 모두 authoritative tracker를 확인하고, 로컬 revocation cache를 stale `peer_book`보다 먼저 평가한다.
- `rr device retire`는 revoke와 함께 대상 daemon에 fixed `full_uninstall` ticket을 남긴다. 대상이 `allow_remote_retirement=true`로 명시적으로 허용했고 launchd/systemd-user 관리 daemon으로 실행 중일 때만 별도 recovery helper가 로컬 uninstall을 수행한다. ticket에는 shell command나 경로가 들어가지 않으며 Linux background fallback은 crash/reboot 복구를 보장할 수 없어 revoke-only로 동작한다.
- admin API는 fleet tracker token과 별도 admin token을 모두 요구하고, production admin/retirement 제어면은 HTTPS에서만 동작한다. loopback HTTP는 test build 전용이다. tracker security-state 파일은 absolute path의 private regular file이어야 한다.
- proof/capability endpoint는 bounded authenticated header를 body보다 먼저 처리한다. 인증되지 않은 legacy body는 읽지 않고, body가 필요한 요청도 async ingress의 10초 deadline과 64-request 동시 상한 안에서만 읽는다. TLS reverse proxy의 read/write/body/header limit도 이보다 느슨하게 두지 않고 admin, register, signed unregister, retirement poll/ack/complete endpoint를 정상 poll/retry cadence보다 여유 있는 peer/IP별 한도로 rate-limit한다. `X-Rustory-Device-Request`와 request body는 access log에서 redact/미기록하고 tracker 응답은 cache하지 않는다.
- tracker security state는 process-lifetime exclusive lock으로 단일 writer만 허용한다. transactional shared store가 도입되기 전에는 같은 state path를 공유하는 여러 replica로 tracker를 실행하지 않는다.
- destructive cleanup을 시작하기 전에 daemon 시작 시 검증한 exact cleanup path plan과 ticket별 256-bit completion capability를 private receipt에 저장하고 tracker에는 capability의 SHA-256 hash만 보낸다. helper는 이후 config path가 바뀌어도 receipt의 불변 plan만 사용한다. receipt에는 fleet token이나 identity private key를 복제하지 않는다. config/identity/binary 삭제 뒤에는 이 capability가 `Running → Completed` 전이에만 쓰이고, 응답이 유실되면 독립 helper가 확인될 때까지 재시도한 뒤 receipt와 helper copy를 삭제한다.
- tracker revoke record는 incident 대응을 위해 즉시 생성된다. 응답의 `membership_enforcement_complete=true`는 tracker가 마지막으로 수락한 signed registration capability와 현재 active peer를 기준으로 한 advisory snapshot이다. live attestation이 아니며 node의 offline rollback까지 증명하지 않는다. false warning은 revoke 실패가 아니라 active legacy/미준비 peer 때문에 fleet 전체 강제가 아직 확인되지 않았다는 뜻이다. 기본값은 꺼져 있어 기존 fleet 동작을 유지하지만 strict mode를 켜면 미등록/구버전 peer는 의도적으로 거부된다.
- 노드를 분실하거나 `swarm.key`/tracker token이 유출되면 device revoke만으로 충분하다고 가정하지 않는다. revoke는 Rustory history pull/push membership을 막지만 유출된 shared credential 자체를 폐기하지 않는다.
- transport-level grid 접근까지 폐기하려면 정상 node에서 tracker token과 `swarm.key`를 rotation하고, 필요하면 relay allowlist도 갱신한다. 이미 다른 peer/backup에 복제된 history는 revoke나 remote uninstall로 secure erase되지 않는다.
- 오프라인 대상에도 revoke와 ticket은 즉시 durable하게 남지만 로컬 파일 삭제는 대상이 다시 온라인이 되어 정상 helper를 실행해야 완료된다. 침해된 대상, root 권한 대상, 전원이 꺼진 대상의 파일을 Rustory만으로 강제 삭제한다고 간주하지 않는다.

## Application-level E2EE 도입 기준

현재 relay/tracker는 history payload를 저장하지 않으므로 wire entry를 다시 AEAD로 감싸는 것은 Noise와 보호 범위가 겹친다. 다음 중 하나를 도입할 때는 Hishtory와 같은 client-side application encryption을 필수 설계 조건으로 다시 검토한다.

- 중앙 store-and-forward history backend
- S3/object storage queue
- untrusted persistence layer에 sync payload 보관
- authorized peer가 아닌 서비스에서 history payload를 처리하는 기능

이때도 local search DB와 authorized endpoint compromise는 별도 문제이며, key recovery/rotation, nonce/version format, authenticated deletion metadata, migration/rollback을 함께 설계해야 한다.

## 운영 확인

- `rr doctor`: DB/config/key permission과 tracker HTTPS/reachability를 확인한다.
- `rr sync-status --json --with-tracker`: peer membership, warnings, deletion/push pending을 확인한다.
- `rr mesh --watch`: active peer와 상대 version을 확인한다.
- `rr device list|status`: enrollment, strict membership enforcement, retirement capability와 cleanup ACK를 확인한다.
- `rr hook --shell bash|zsh`: 현재 generated hook의 privacy 동작을 확인한다.
