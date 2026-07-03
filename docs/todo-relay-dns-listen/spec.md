# Spec: relay-dns-listen

## 배경

- 요청 맥락: 외부망 Linux 컨테이너가 v1.0.28로 업데이트된 뒤에도 tracker는 정상인데 모든 peer에 대해 `pending_push`가 줄지 않고 `rr mesh --watch`가 `queued` 상태로 머무른다.
- 현재 문제/기회: daemon 로그에서 `p2p-serve`가 `/dns4/relay.example.com/.../p2p-circuit` listener를 직접 열다가 `Multiaddr is not supported`로 닫힌다. dial 경로는 DNS relay 주소를 해석하지만, relay reservation listen 경로는 DNS를 해석하지 않아 외부 컨테이너가 relay reservation을 안정적으로 유지하지 못한다.

## 계획 스냅샷

- 목표: `p2p-serve` relay reservation listen 경로에서도 DNS relay multiaddr를 IP multiaddr로 해석해 `/p2p-circuit` listener를 열도록 고친다.
- 범위: relay listen/re-listen 주소 생성, 해당 단위 테스트, release/deploy용 문서 증거.
- 검증 명령: `cargo test relay_circuit_listen_addr --workspace`, `cargo test resolve_dns_multiaddr --workspace`, `cargo test p2p --workspace`, `scripts/smoke_p2p_local.sh`, `scripts/check.sh --fast`.
- 완료 기준: DNS relay 주소가 listen 경로에서 `/ip4/.../p2p/<relay>/p2p-circuit`로 변환되고, 기존 p2p/smoke/fast gate가 통과한다. 릴리즈 후 외부 컨테이너에서 `p2p relay reservation accepted` 로그가 재확인되면 운영 관찰 완료로 본다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test relay_circuit_listen_addr --workspace` | DNS relay listen 주소 변환 helper와 단위/relay reservation 회귀 테스트 추가 |
| C2 | done | codex | `cargo test p2p --workspace` | `p2p-serve` 초기 listen 및 listener close 후 re-listen 경로에 helper 적용 |
| C3 | done | codex | `scripts/check.sh --fast` | 회귀 검증 후 릴리즈 가능한 상태로 정리 |
| C4 | done | codex | `scripts/release-version.sh --version v1.0.29 --profile daily-driver --gate quick --work-id relay-dns-listen` | v1.0.29 릴리즈 및 로컬 맥북 + k8s 5개 노드 배포 |
| C5 | todo | user | `rr update --version v1.0.29 && rr sync-status --json --with-tracker && tail -n 80 ~/.local/state/rustory/daemon.log` | 외부망 Linux 컨테이너에서 relay reservation 복구 확인 |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3, C4.
- 미완료: C5.
- 다음 액션: 외부망 Linux 컨테이너에서 v1.0.29로 업데이트한 뒤 daemon log에 `p2p relay reservation accepted`가 나타나고 `pending_push`가 drain되는지 확인한다.
- 검증 증거: `scripts/start-work.sh --work-id relay-dns-listen`, `cargo test relay_circuit_listen_addr --workspace` (2 passed), `cargo test p2p_relay_reservation_accepts_dns_relay_listen_addr --workspace` (1 passed), `cargo test resolve_dns_multiaddr --workspace` (3 passed), `cargo test p2p --workspace` (45 passed), `scripts/smoke_p2p_local.sh` (ok), `scripts/check.sh --fast` (248 passed + clippy), `scripts/finalize-and-push.sh --message "fix: resolve dns relay listen addresses" --work-id relay-dns-listen` (pushed `aba3a4b1`), `scripts/finalize-and-push.sh --message "build: release 1.0.29" --work-id relay-dns-listen` (pushed `ccf7d440`), `scripts/release-version.sh --version v1.0.29 --profile daily-driver --gate quick --work-id relay-dns-listen` (GitHub release published), `rr update --version v1.0.29` on local macOS + `ts-sample-node`, `builder0`, `builder1`, `builder2`, `builder3` (all version `1.0.29`, daemon restarted, tracker reachable, internal peers pending 0).
