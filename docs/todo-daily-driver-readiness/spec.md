# Spec: daily-driver-readiness

## 배경

- 요청 맥락: Hishtory public sync가 503으로 실사용 신뢰를 잃어 Rustory를 daily driver로 전환해야 한다.
- 현재 문제/기회: Docker relay-only acceptance는 green이지만, 실사용 전환에서는 tracker/token/relay 설정 오류를 daemon 시작 전에 더 빨리 잡아야 한다.

## 계획 스냅샷

- 목표: Rustory를 direct-only가 아닌 tracker + relay 기반 daily-driver 경로로 안전하게 전환할 수 있게 한다.
- 범위: daemon 전환 전 preflight guard, 관련 문서, Docker relay acceptance 재검증, 다음 multi-machine soak 항목 추적.
- 검증 명령: `cargo fmt --all --check`, `cargo test daemon_ --workspace`, `scripts/check.sh --fast --acceptance`.
- 완료 기준: daemon preflight guard가 코드/문서/테스트에 반영되고, relay circuit을 실제 사용한 acceptance가 통과하며, 남은 실사용 soak 항목이 명시된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test daemon_ --workspace` | `rr daemon --preflight`로 configured tracker ping을 자식 프로세스 시작 전에 검증한다. |
| C2 | done | codex | `scripts/check.sh --fast --acceptance` | Docker macOS/Linux acceptance와 two-peer relay-only acceptance를 재실행해 relay circuit 사용과 DB 수렴을 확인한다. |
| C3 | done | codex | `rr doctor --json`, `rr sync-status --json --with-tracker`, canary sync | 로컬 MacBook과 원격 Linux peer에서 실제 tracker/token/relay 설정을 preflight로 확인하고 canary sync 증거를 남긴다. |
| C4 | in_progress | codex | `rr sync-status --json --with-tracker` | 24시간 soak 또는 사용자가 승인한 축약 soak에서 반복 실패/timeout 폭증이 없는지 확인한다. |
| C5 | done | codex | `rr --version`, `rr version --json`, `rr doctor --json`, `scripts/check-script-smoke.sh --work-id daily-driver-readiness` | daily-driver production 기준점의 package version과 build revision 추적 경로를 제공하고, Codex-authored commit trailer 누락을 finalize 경로에서 차단한다. |
| C6 | done | codex | `cargo test search::tests --workspace`, `cargo test --workspace`, fzf `--filter` metadata query smoke | ctrl+r 검색을 hishtory-like metadata table로 바꾸고 hostname/CWD/command를 함께 검색 대상으로 삼는다. |

## 완료/미완료/다음 액션

- 완료: daemon preflight guard 구현, 문서 반영, Docker relay acceptance 재검증, MacBook + 원격 Linux peer 실제 tracker/token/relay 설정과 canary sync.
- 미완료: 24시간 soak 증거.
- 다음 액션: MacBook LaunchAgent와 원격 Linux peer systemd user service를 24시간 관찰하고, 양쪽 `rr sync-status --json --with-tracker`와 relay journal의 circuit 증거를 보존한다.
- 검증 증거: `cargo fmt --all --check`, `cargo test daemon_ --workspace`, `scripts/check.sh --fast --acceptance`.

## 2026-06-28 MacBook + Remote Linux Peer Evidence

- Remote Linux peer: tracker, relay, and peer daemon enabled/running under systemd user service with linger enabled.
- MacBook: `~/Library/LaunchAgents/com.rustory.daemon.plist` loaded/running via launchd.
- Tracker auth: unauthenticated `/api/v1/ping` returned 401; bearer-token `/api/v1/ping` returned 200.
- Shared swarm fingerprint matched on both peers.
- Relay addr used a persistent relay identity and a private network address; exact value is intentionally not committed.
- Canary sync after service install: relay circuit count increased and both DBs contained both peer canary rows.
- Canary sync after hook guard release reinstall: relay circuit count increased again and both DBs contained both peer canary rows.
- Final `sync-status`: tracker reachable on both machines, `pending_push=0` on the peer status rows.
- Hishtory local import: real DB import received 151981 rows, inserted 149672, skipped 2309 by privacy guard; immediate re-import inserted 0 and ignored 149672.
- Hishtory import propagation: remote Linux peer reached the same entry count as the MacBook after tracker + relay sync; final peer status reported `pending_push=0`.
- Large import retry hardening: relay resource-limit / reset-style dial failures observed during import propagation are now classified as retryable P2P transient failures.
- Shell handoff cleanup: active Hishtory profile hooks were disabled while preserving Hishtory files/DB as fallback, a stale zsh-only profile on the bash-only Linux peer was removed, and Rustory hook startup noise was fixed.
- Relay restart hardening: `p2p-serve` removes closed relay-circuit listen addresses from tracker registration and immediately re-listens on the configured relay, so relay restart does not leave stale peer records until TTL expiry.
- Release identity: `rr --version` now reports `1.0.0 (rev <build_revision>)`; `rr version --json` and `rr doctor --json` expose `version`, `build_revision`, `build_revision_source`, and `build_dirty`; `scripts/finalize-and-push.sh` uses `describe_with_attribution.sh` and verifies the `Co-authored-by` trailer before push.
- Ctrl+r search UX: `rr search` now uses a hishtory-like table that displays hostname, CWD, timestamp, runtime, exit code, and command while searching across hostname/device/full-CWD/command metadata; fzf filter smoke matched `smp pro doc` and `pro doc` style queries against split hostname/CWD/command tokens.
