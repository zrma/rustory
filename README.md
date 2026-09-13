# Rustory

<p align="center">
  <img src="docs/assets/rustory-mark.svg" width="360" alt="두 개의 양피지 기록이 붉은 실로 연결된 Rustory 로고">
</p>

<p align="center">
  <strong>로컬에 남기고, P2P로 잇는다.</strong>
</p>

Rustory는 `rr` 하나로 기록하고, `Ctrl-R`로 다시 찾고, 여러 디바이스와 동기화하는
local-first 셸 히스토리 도구다.

- **Local-first** — 각 명령은 디바이스의 SQLite DB에 먼저 기록된다.
- **Fast recall** — 익숙한 `Ctrl-R` 흐름에서 필요한 히스토리를 빠르게 찾는다.
- **P2P grid** — 서로 다른 WiFi/NAT/router 뒤의 머신도 tracker와 relay를 통해 이어진다.

## Quick Start


기존 rr grid에 새 디바이스를 붙이는 설치 형태:

```sh
export RUSTORY_TRACKER_TOKEN="<fleet-token>"
curl -fsSL https://raw.githubusercontent.com/zrma/rustory/main/install/rustory.py | \
  python3 - --tracker "<https://tracker.example.com>" \
    --relay "/dns4/<relay.example.com>/tcp/4001/p2p/<relay_peer_id>" \
    --user-id "<shared-user-id>" \
    --swarm-key-b64 "<base64-swarm-key>" \
    --install-hook \
    --install-daemon \
    --import-hishtory
```

tracker token, relay PeerId와 swarm key는 개인 설정이다. 실제 값은 secret store 등
안전한 장소에 보관하고 저장소나 공개 문제 보고에 첨부하지 않는다.

설치 후 `rr doctor`로 설정을 확인하고, `rr sync-status --json --with-tracker`와
`rr mesh --watch`로 동기화와 연결 상태를 확인한다. 새 환경 준비와 자세한 설치 방법은
[빠른 시작](docs/quickstart.md)을 따른다.

같은 grid는 `user_id`와 `swarm.key`를 공유하지만 각 디바이스의 `identity.key`는
고유해야 한다. tracker와 relay를 포함한 연결·개인정보 경계는
[P2P 가이드](docs/p2p.md)와 [보안 모델](docs/security.md)에서 확인한다.

## Product Docs

- [빠른 온보딩](docs/quickstart.md) · [배포와 self-update](docs/distribution.md)
- [P2P tracker·relay·sync](docs/p2p.md) · [daemon](docs/daemon.md) · [shell hook](docs/hook.md)
- [보안과 프라이버시](docs/security.md)
- [Hishtory 가져오기](docs/hishtory-migration.md) · [Atuin 가져오기](docs/atuin-migration.md)
- [전체 문서 인덱스](docs/README.md)

## Development

개발 작업은 [Handoff](docs/HANDOFF.md)에서 관련 문서와 검증 경로를 찾는다.
AI 작업 지침은 [AGENTS.md](AGENTS.md)를 따른다. 일반적인 빠른 로컬 검증은
`scripts/check.sh --fast`이며, 출고 검증과 게시 절차는
[Change Control](docs/CHANGE_CONTROL.md)이 소유한다.

CLI 옵션과 기본값의 기준은 `rr --help`와 해당 구현이다.
