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

이 README는 긴 운영 런북이 아니라 프로젝트의 공개 얼굴과 handoff index다. 실제 옵션,
default, 데이터 구조, 실행 분기는 `src/*`, `scripts/*`, `Cargo.toml`,
`docs/REPO_MANIFEST.yaml`, `rr --help`가 소유한다.

Rustory가 지키려는 경계는 단순하다.

- 기록은 각 디바이스의 local SQLite DB에 먼저 남는다.
- grid discovery는 tracker가, NAT 뒤 connectivity는 relay가 맡는다.
- 같은 grid는 shared `user_id`와 `swarm.key`를 쓰고, 각 디바이스의 `identity.key`는 고유해야 한다.
- direct-only 성공은 production readiness 증거가 아니다. 서로 다른 NAT 뒤 relay circuit 증거가 필요하다.
- AI agent가 코드, 문서, 스크립트, acceptance evidence를 따라 유지보수할 수 있어야 한다.

## Quick Start

기존 rr grid에 새 디바이스를 붙이는 설치 형태:

```sh
curl -fsSL https://raw.githubusercontent.com/zrma/rustory/main/install/rustory.py | \
  python3 - --token "$RUSTORY_TRACKER_TOKEN" \
    --tracker "<https://tracker.example.com>" \
    --relay "/dns4/<relay.example.com>/tcp/4001/p2p/<relay_peer_id>" \
    --user-id "<shared-user-id>" \
    --swarm-key-b64 "<base64-swarm-key>" \
    --install-hook \
    --install-daemon \
    --import-hishtory
```

public 문서에는 placeholder만 둔다. 실제 tracker URL, token, relay PeerId, swarm key는
private archive나 secret store에 보관하고 이 저장소에 커밋하지 않는다.

설치 후에는 `rr doctor`, `rr sync-status --json --with-tracker`, `rr mesh --watch`로
config, tracker, peer cursor, pending row/delete 상태를 확인한다. 자세한 onboarding,
self-update, daemon, Hishtory/Atuin import 절차는 `docs/quickstart.md`, `docs/distribution.md`,
`docs/daemon.md`, `docs/hishtory-migration.md`, `docs/atuin-migration.md`가 소유한다.

## Agent Navigation

AI agent에게 이 repo를 맡길 때 하네스 계약은 `docs/agent-harness.md`, 제품/운영 탐색은
`docs/HANDOFF.md`에서 시작한다. README는 방향을 잡는 landing page이고, 실제 실행 규칙은 아래 문서들이 소유한다.

- 에이전트 규칙: `AGENTS.md`
- 공통 하네스 인터페이스: `docs/agent-harness.md`
- 운영 모델: `docs/OPERATING_MODEL.md`
- 문서 책임 경계: `docs/README_OPERATING_POLICY.md`
- 구현 루프: `docs/EXECUTION_LOOP.md`
- 출고/푸시 경계: `docs/CHANGE_CONTROL.md`
- 개선/회귀 루프: `docs/IMPROVEMENT_LOOP.md`
- 에스컬레이션 기준: `docs/ESCALATION_POLICY.md`
- 반복 교훈: `docs/LESSONS_LOG.md`
- 장기 유지보수 축: `docs/MAINTENANCE_PILLARS.md`
- 보안/프라이버시 신뢰 경계: `docs/security.md`
- 진입점/검증 명령 선언: `docs/REPO_MANIFEST.yaml`

무컨텍스트 시작은 보통 `jj status`, `find docs -maxdepth 1 -type d -name 'todo-*' | sort`,
`rr --help`, `scripts/check.sh --fast`로 충분하다. 다음 판단은 `docs/HANDOFF.md`가 맡는다.

## Product Docs

- 빠른 온보딩: `docs/quickstart.md`
- 배포와 self-update: `docs/distribution.md`
- P2P tracker/relay/sync: `docs/p2p.md`
- daemon과 service manager: `docs/daemon.md`
- shell hook: `docs/hook.md`
- 보안과 프라이버시 모델: `docs/security.md`
- Hishtory migration: `docs/hishtory-migration.md`
- Atuin migration: `docs/atuin-migration.md`
- acceptance guide: `docs/acceptance/README.md`
- 전체 문서 인덱스: `docs/README.md`

## Development

README와 docs는 구현을 재서술하지 않고 source-of-truth 위치를 가리킨다. 동작이 바뀌면
먼저 코드, CLI help, 스크립트, `docs/REPO_MANIFEST.yaml`을 확인한 뒤 필요한 문서만 갱신한다.

일반적인 로컬 검증은 `scripts/check.sh --fast`다. 출고와 push는 보통
`scripts/finalize-and-push.sh --message "<type>: <summary>"` 경로로 닫는다. 더 넓은
acceptance, 릴리즈, 보안 보완은 해당 `docs/todo-*` spec과 `docs/CHANGE_CONTROL.md`를 따른다.
