# Acceptance Test: Docker (macOS host + Linux container)

목표: k8s/VPS 없이도 `tracker + relay + linux peer` 구성을 docker로 띄우고, macOS host가 `rr p2p-sync`로 동기화하며 **relay fallback**이 실제로 사용되는지 확인한다.

## 빠른 실행(권장)
```sh
bash scripts/acceptance_docker_macos_linux.sh
```

성공하면 다음을 확인할 수 있다.
- macOS 측 출력에 `p2p pull summary: ... inserted=...`가 표시된다.
- macOS에서 기록한 엔트리가(`acceptance-from-mac`) linux peer DB로 push되어 들어간다.
  - 스크립트는 linux peer의 DB(`/tmp/linux.db`)를 snapshot으로 꺼내 `target/acceptance/docker-macos-linux/linux.db`에 저장한 뒤 검증한다.
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
필요하면 로컬에 엔트리를 하나 만들고(push 검증):
```sh
RUSTORY_USER_ID=acceptance RUSTORY_DEVICE_ID=mac \
target/debug/rr --db-path "$PWD/target/acceptance/docker-macos-linux/mac.db" record \
  --cmd "echo acceptance-from-mac" --cwd "/tmp" --shell zsh --hostname mac --print-id
```

```sh
RUSTORY_USER_ID=acceptance \
RUSTORY_DEVICE_ID=mac \
RUSTORY_SWARM_KEY_PATH="$PWD/target/acceptance/docker-macos-linux/swarm.key" \
RUSTORY_TRACKER_TOKEN="acceptance-token" \
target/debug/rr --db-path "$PWD/target/acceptance/docker-macos-linux/mac.db" p2p-sync \
  --trackers "http://127.0.0.1:8850" \
  --relay "/ip4/127.0.0.1/tcp/4001/p2p/<relay_peer_id>" \
  --push \
  --limit 1000
```

5) (선택) linux peer DB snapshot 꺼내기(push 검증용)
```sh
docker compose -f contrib/docker/acceptance/compose.yml stop linux-peer
LINUX_CID="$(docker compose -f contrib/docker/acceptance/compose.yml ps -q linux-peer)"
docker cp "$LINUX_CID:/tmp/linux.db" "$PWD/target/acceptance/docker-macos-linux/linux.db"
```

## 정리
```sh
RUSTORY_ACCEPTANCE_DIR="$PWD/target/acceptance/docker-macos-linux" \
docker compose -f contrib/docker/acceptance/compose.yml down -v
```
