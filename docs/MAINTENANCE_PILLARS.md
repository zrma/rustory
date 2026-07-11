# Maintenance Pillars

- Audience: Rustory maintainer, AI agent
- Owner: Rustory
- Last Verified: 2026-07-12

Rustory의 다음 단계는 기능을 계속 붙이는 것보다, 오래 써도 무너지지 않는 daily-driver 운영 체계를 유지하는 것이다. 이 문서는 구현을 재서술하지 않는다. 각 유지보수 축의 판단 기준, 소유 위치, 검증 증거를 연결하는 얇은 지도다.

실제 옵션, 기본값, 데이터 구조, 실행 분기는 `src/*`, `scripts/*`, `Cargo.toml`, `docs/REPO_MANIFEST.yaml`, CLI help가 소유한다. 문서와 구현이 어긋나면 구현과 live evidence를 먼저 확인하고 문서를 따라 고친다.

## Pillars

| 축 | 왜 중요한가 | 소유 위치 | 확인 증거 |
| --- | --- | --- | --- |
| Agent handoff and gates | 사람이 모든 컨텍스트를 기억하지 않아도 후속 agent가 이어받아야 한다. | `AGENTS.md`, `docs/HANDOFF.md`, `docs/EXECUTION_LOOP.md`, `docs/CHANGE_CONTROL.md`, `docs/REPO_MANIFEST.yaml`, `scripts/start-work.sh`, `scripts/check.sh`, `scripts/finalize-and-push.sh` | `scripts/run-manifest-checks.sh`, `scripts/check-release-gates.sh`, `scripts/finalize-and-push.sh` |
| Node lifecycle and identity hygiene | self-uninstall, admin revoke/retire, 재가입, 중복 hostname/device identity가 grid membership을 흔들 수 있다. | `docs/dev-playbook.md`, `docs/security.md`, `docs/p2p.md`, `docs/daemon.md`, `src/uninstall.rs`, `src/device_retirement.rs`, `src/tracker.rs`, `src/cli.rs` | `rr uninstall --dry-run`, `rr device list`, `rr device revoke`, `rr device retire`, `rr doctor`, `rr sync-status --with-tracker`, duplicate active peer warning |
| Sync observability | pending row/delete가 실제 stuck인지, 단순 대기인지 구분해야 한다. | `docs/p2p.md`, `docs/acceptance/README.md`, `src/cli.rs`, `src/sync.rs`, `src/storage.rs` | `rr sync-status --json --with-tracker`, `rr mesh --watch`, acceptance canary |
| Log signal vs noise | relay/NAT retry, DCUtR 실패, stale daemon이 정상 동기화를 장애처럼 보이게 만들 수 있다. | `docs/p2p.md`, `docs/LESSONS_LOG.md`, `src/p2p.rs`, `src/cli.rs` | `~/.local/state/rustory/daemon.log`, `RUSTORY_P2P_LOG=verbose`, p2p log tests |
| Data hygiene and GC | dedupe/delete/tombstone/prune이 peer cursor보다 앞서가면 삭제 동기화나 복구 판단이 깨진다. | `docs/p2p.md`, `src/sync.rs`, `src/storage.rs`, `src/cli.rs` | `rr dedupe --dry-run`, `rr delete --dry-run`, `rr tombstone-gc --dry-run`, `rr sync-status --json --with-tracker` |
| Installer, update, and daemon resilience | one-shot install/update가 macOS, systemd user, container fallback에서 같은 결과를 내야 한다. | `docs/distribution.md`, `docs/daemon.md`, `install/rustory.py`, `src/self_update.rs` | `rr update`, `rr doctor --auto-fix`, daemon process check, installer smoke, self-update tests |
| CI green and release traceability | published binary, source revision, fleet state가 서로 맞아야 운영 판단을 신뢰할 수 있다. | `docs/CHANGE_CONTROL.md`, `docs/REPO_MANIFEST.yaml`, `docs/LESSONS_LOG.md`, `scripts/release-version.sh` | `cargo fmt`, `cargo test`, `cargo clippy`, `scripts/check-release-gates.sh`, GitHub Actions, deployed `rr version` |

## Operating Rules

- 새 기능은 위 축 중 하나 이상의 책임을 더 명확하게 만들 때만 추가한다.
- 같은 종류의 실패가 두 번 반복되면 설명만 남기지 말고 gate, smoke, acceptance, 또는 lessons entry로 만든다.
- `rr mesh --watch`와 `rr sync-status`는 local observation이다. 전역 peer-to-peer flow처럼 보이게 꾸미지 않는다.
- node lifecycle 변경은 membership 변경이다. revoke는 즉시 적용하고 cooperative data deletion은 별도 ticket/ACK로 추적한다.
- tombstone GC와 dedupe는 dry-run evidence, peer cursor evidence, rollback 경계를 먼저 확인한다.
- installer/update 변경은 파일 교체뿐 아니라 이미 떠 있는 daemon 재시작, fallback autostart, stale process 제거까지 검증한다.
- release/deploy 후에는 적어도 `rr version`, `rr doctor`, `rr sync-status --json --with-tracker` 중 관련 evidence를 남긴다.

## When To Escalate

아래는 agent가 임의로 넘기지 말고 사용자에게 상태를 좁혀 보고해야 하는 신호다.

- 같은 hostname에 여러 active device identity가 동시에 살아 있다.
- deletion/tombstone backlog가 여러 tick 동안 줄지 않거나, GC가 peer delete cursor보다 앞서야만 진행 가능한 상태다.
- tracker는 healthy인데 relay reservation 또는 relay circuit evidence가 반복적으로 없다.
- update/install이 binary 교체에는 성공했지만 running daemon revision이 바뀌지 않는다.
- CI가 red인데 release/deploy 요청이 이어진다.

## Handoff Prompt

무컨텍스트 agent에게 넘길 때는 이렇게 시작하면 된다.

```text
Rustory 유지보수 요청입니다. 먼저 docs/HANDOFF.md와 docs/MAINTENANCE_PILLARS.md를 읽고,
관련 pillar의 source-of-truth와 verification command를 확인한 뒤 작업하세요.
구현 동작은 문서보다 src/*, scripts/*, docs/REPO_MANIFEST.yaml, rr --help를 우선합니다.
회귀 방지는 scripts/check.sh 또는 scripts/finalize-and-push.sh 경로로 검증하세요.
```
