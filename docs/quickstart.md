# Quickstart

이 문서는 Rustory를 "최소 구성"으로 빠르게 써보는 흐름을 정리한다.

## 0) 준비
- Rust 툴체인: `rust-toolchain.toml` 기준(`1.89.0`)

### 빌드(로컬)
```sh
cargo build --release
./target/release/rr --help
```

또는 설치(선택):
```sh
cargo install --path .
rr --help
```

## 1) 가장 빠른 e2e 확인(권장)
레포 루트에서:
```sh
scripts/smoke_p2p_local.sh
```

tracker + relay + 2 peer + sync(push 포함)까지 로컬에서 자동으로 띄워서 검증한다.

## 2) 실사용: tracker/relay + 디바이스 온보딩

### 2-1) (항상 온라인일 필요는 없지만) tracker/relay 실행

#### Relay 서버
```sh
rr relay-serve --listen /ip4/0.0.0.0/tcp/4001
```

출력되는 `relay listen: .../p2p/<relay_peer_id>` 주소를 기록한다.

#### Tracker 서버
```sh
rr tracker-serve --bind 0.0.0.0:8850 --ttl-sec 60 --token "secret"
```

### 2-2) 각 디바이스에서 init
각 디바이스에서:
```sh
rr init \
  --user-id "<user>" \
  --device-id "<device>" \
  --trackers "http://<tracker-host>:8850" \
  --relay "/ip4/<relay-ip>/tcp/4001/p2p/<relay_peer_id>" \
  --tracker-token "secret"

rr doctor
```

`rr init`는 기본적으로 다음을 준비한다.
- `~/.config/rustory/config.toml` (설정 템플릿)
- `~/.config/rustory/swarm.key` (PSK, 같은 swarm 내 디바이스는 동일 파일 공유)
- `~/.config/rustory/identity.key` (PeerId, 디바이스별 고유)

### 2-2-1) (선택) 기존 히스토리 seed(import)
기존 셸 히스토리 파일을 DB로 가져오려면:

```sh
# zsh
rr import --shell zsh

# bash
rr import --shell bash
```

필요하면:
- 다른 파일을 지정: `rr import --shell zsh --path /path/to/file`
- 마지막 N개만: `rr import --shell zsh --limit 100000`

import는 `RUSTORY_RECORD_IGNORE_REGEX` / `record_ignore_regex`를 존중한다.

### 2-3) 주기 동기화 실행(추천: 데몬/스케줄러)
```sh
rr p2p-sync --watch --interval-sec 60 --start-jitter-sec 10 --push
```

백그라운드 실행 예시는 `docs/daemon.md` 참고.

### 2-4) hook 활성화(현재 셸 세션)
```sh
source <(rr hook --shell zsh)
```

bash/zsh 훅 상세는 `docs/hook.md` 참고.

### 2-5) (선택) 민감 커맨드 기록 제외
예:
```sh
export RUSTORY_RECORD_IGNORE_REGEX='(?i)(password|token|secret|authorization:|bearer )'
```

이 옵션은 hook이 호출하는 `rr record`에도 적용된다. 상세는 `docs/hook.md` 참고.

## 다음 문서
- P2P 상세/트러블슈팅: `docs/p2p.md`
- 데몬/스케줄러: `docs/daemon.md`
- 훅: `docs/hook.md`
