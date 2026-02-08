# Acceptance Test: Docker (macOS host + Linux container)

목표: k8s/VPS 없이도 `tracker + relay + linux peer` 구성을 docker로 띄우고, macOS host가 `rr p2p-sync`로 동기화하며 **relay fallback**이 실제로 사용되는지 확인한다.

## 빠른 실행(권장)
```sh
bash scripts/acceptance_docker_macos_linux.sh
```

성공하면 다음을 확인할 수 있다.
- macOS 측 출력에 `p2p pull summary: ... inserted=...`가 표시된다.
- relay 컨테이너 로그에 `relay: circuit accepted:`가 표시된다.

## 수동 실행(디버깅용)
1) Docker 서비스 시작(tracker/relay)
```sh
RUSTORY_ACCEPTANCE_DIR="$PWD/target/acceptance/docker-macos-linux" \
docker compose -f contrib/docker/acceptance/compose.yml up -d --build tracker relay
```

2) relay peer id 확인
```sh
docker compose -f contrib/docker/acceptance/compose.yml logs --no-color relay | grep "^relay listen:" | head -n 1
```

3) linux peer 시작(위에서 얻은 peer id 사용)
```sh
RELAY_PEER_ID="<relay_peer_id>" \
RUSTORY_ACCEPTANCE_DIR="$PWD/target/acceptance/docker-macos-linux" \
docker compose -f contrib/docker/acceptance/compose.yml up -d linux-peer
```

4) macOS host에서 동기화 실행
```sh
RUSTORY_USER_ID=acceptance \
RUSTORY_DEVICE_ID=mac \
RUSTORY_SWARM_KEY_PATH="$PWD/target/acceptance/docker-macos-linux/swarm.key" \
RUSTORY_TRACKER_TOKEN="acceptance-token" \
target/debug/rr --db-path "$PWD/target/acceptance/docker-macos-linux/mac.db" p2p-sync \
  --trackers "http://127.0.0.1:8850" \
  --relay "/ip4/127.0.0.1/tcp/4001/p2p/<relay_peer_id>" \
  --limit 1000
```

## 정리
```sh
RUSTORY_ACCEPTANCE_DIR="$PWD/target/acceptance/docker-macos-linux" \
docker compose -f contrib/docker/acceptance/compose.yml down -v
```

