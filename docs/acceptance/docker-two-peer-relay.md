# Acceptance Test: Docker Two-Peer Relay-Only

목표: 두 peer를 서로 다른 Docker bridge network에 분리하고, tracker/relay만 양쪽 network에 붙여 **peer 간 direct 경로 없이** `tracker + relay` 동기화가 수렴하는지 확인한다.
정확한 컨테이너 이름, 토큰, DB 경로, 검증 문자열은 `scripts/acceptance_docker_two_peer_relay.sh`가 소유한다.

## 빠른 실행
```sh
bash scripts/acceptance_docker_two_peer_relay.sh
```

전체 acceptance gate에 포함해서 실행하려면:
```sh
scripts/check.sh --fast --acceptance
```

## 검증 축
- `peer-a`와 `peer-b`는 서로 다른 Docker network에만 붙어 있고, 서로의 Docker DNS 이름을 직접 해석할 수 없어야 한다.
- tracker와 relay는 두 network에 모두 붙어 있어야 한다.
- 각 peer는 실제 shell 명령 3개를 수행한 뒤 `rr record`로 기록한다.
- `rr p2p-sync --trackers ... --relay ... --push`를 양방향으로 반복했을 때 두 peer DB가 같은 6개 엔트리로 수렴해야 한다.
- relay 로그의 `relay: circuit accepted:` 카운트가 sync 수행 중 증가해야 한다.

## 디버깅
기본 실행은 종료 시 컨테이너와 network를 정리한다. 실패 상태를 남기려면:
```sh
RUSTORY_ACCEPTANCE_KEEP=1 bash scripts/acceptance_docker_two_peer_relay.sh
```

주요 확인 지점:
```sh
docker logs rustory-two-peer-relay-tracker
docker logs rustory-two-peer-relay-relay
docker logs rustory-two-peer-relay-peer-a
docker logs rustory-two-peer-relay-peer-b
```

성공 시 snapshot DB는 `target/acceptance/docker-two-peer-relay/peer-a.db`,
`target/acceptance/docker-two-peer-relay/peer-b.db`에 남는다.
