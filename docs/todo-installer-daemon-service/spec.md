# Spec: installer-daemon-service

## 배경

- 요청 맥락: 신규 머신에서 one-shot installer만 실행해도 Rustory가 tracker/relay grid의 상시 멤버가 되어야 한다.
- 현재 문제/기회: hook/import만 끝나고 daemon 등록이 빠지면 peer가 tracker에서 금방 사라진다. 또한 tailnet/private relay 주소는 외부 신규 머신에서 dial할 수 없으므로 public relay multiaddr 기준이 필요하다.

## 계획 스냅샷

- 목표: installer가 macOS/Linux user service를 설치하고, public relay 주소를 기준으로 운영 문서와 진단 경고를 맞춘다.
- 범위: installer, CLI doctor 경고, DNS relay multiaddr transport, public 문서, private install archive 갱신.
- 검증 명령: `python3 -m py_compile install/rustory.py`, `cargo test sync_status_report_includes_pending_push_and_filter discover_targets_skips_self_peer_book_entries_by_peer_id_and_device_id tracker_target_addrs_requires_dialable_direct_or_advertised_relay_reservation inbound_push_provenance_accepts_arch_suffix_device_renames`, `RUSTORY_RELEASE_LINUX_BUILDER=zig scripts/build-release-assets.sh --target x86_64-unknown-linux-gnu --dist-dir /tmp/rustory-release-portable-linux`, `cargo test relay_addr_warning_flags_tailnet_and_private_addresses`, `scripts/check.sh --fast`, `scripts/run-manifest-checks.sh --mode quick --work-id installer-daemon-service`.
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고 검증 명령이 재현 가능하게 남는다. 단, 외부 라우터/NAT L4 포워딩처럼 이 repo에서 직접 소유하지 않는 운영 작업은 후속 체크포인트로 남긴다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `python3 -m py_compile install/rustory.py` | `install/rustory.py`가 `--install-daemon`, `--no-start-daemon`, daemon interval/jitter 옵션을 제공한다. |
| C2 | done | codex | `python3 -m py_compile install/rustory.py` | macOS launchd user agent와 Linux systemd user service를 installer가 생성/시작한다. |
| C3 | done | codex | `cargo test relay_addr_warning_flags_tailnet_and_private_addresses` | installer와 `rr doctor`가 tailnet/private/loopback literal relay 주소를 경고한다. |
| C4 | done | codex | `cargo test relay_addr_warning_flags_tailnet_and_private_addresses` | Rustory P2P transport가 DNS relay multiaddr(`/dns4/...`)을 dial할 수 있다. |
| C5 | done | codex | `scripts/run-manifest-checks.sh --mode quick --work-id installer-daemon-service` | one-shot install 문서가 hook + daemon + Hishtory import + public relay 주소를 기준으로 갱신된다. |
| C6 | done | codex | `scripts/check.sh --fast` | fast checks와 관련 단위 테스트가 통과한다. |
| C7 | todo | owner | `nc -vz -w 5 <public-relay-ip> 4001` | 클러스터 외부 인터넷에서 relay TCP/4001이 public IP를 통해 도달 가능한지 라우터/NAT/LB 경로를 확인한다. |
| C8 | done | codex | `RUSTORY_RELEASE_LINUX_BUILDER=zig scripts/build-release-assets.sh --target x86_64-unknown-linux-gnu --dist-dir /tmp/rustory-release-portable-linux` | Linux x86_64 release asset이 remote builder glibc에 묶이지 않는 경로로 빌드된다. |
| C9 | done | codex | `python3 -m py_compile install/rustory.py` | installer가 다운로드한 binary 검증 실패 시 `rr version`의 stdout/stderr를 숨기지 않고 출력한다. |
| C10 | done | codex | `python3 -m py_compile install/rustory.py` | Linux user systemd bus가 없는 환경에서 `--install-daemon`이 traceback으로 실패하지 않고 unit 설치 후 start deferred 안내로 정상 종료한다. |
| C11 | done | codex | `cargo test sync_status_report_includes_pending_push_and_filter discover_targets_skips_self_peer_book_entries_by_peer_id_and_device_id` | `sync-status --watch`/P2P discovery가 local PeerId와 정규화된 같은 `device_id`를 가진 stale self peer를 제외한다. |
| C12 | done | codex | `python3 -m py_compile install/rustory.py` | Linux user systemd bus가 없는 container-like 환경에서 `--install-daemon`이 `manager=background` fallback으로 `rr daemon`을 즉시 시작한다. |
| C13 | done | codex | `python3 -m py_compile install/rustory.py` | background fallback이 shell rc autostart block을 설치해 컨테이너 재시작 뒤 첫 interactive shell에서 daemon을 자동 복구한다. |
| C14 | done | codex | `cargo test tracker_target_addrs_requires_dialable_direct_or_advertised_relay_reservation tracker_announce_addr_from_listen_addr_filters_local_direct_but_keeps_relay` | tracker/peerbook 주소 선택이 relay reservation 없는 peer에 configured relay를 억지로 붙여 dial하지 않고, loopback/private listen 주소를 tracker에 광고하지 않는다. |
| C15 | done | codex | `cargo test inbound_push_provenance_accepts_arch_suffix_device_renames daemon_` | 같은 PeerId가 `node1`에서 `node1-x86_64`처럼 device label만 바뀐 경우 push provenance를 같은 기기로 허용하고, daemon 기본 fan-out을 backfill 수렴 기준으로 둔다. |
| C16 | done | codex | `cargo test advance_last_pushed_seq_never_moves_cursor_backward loopback_direct_dial_failure_is_log_noise import_hishtory_sqlite_preserves_metadata_and_is_idempotent`, `scripts/check.sh --fast` | peer가 이 노드의 데이터를 pull한 경우도 push coverage로 반영해 `sync-status`의 큰 outbound pending이 실제 전파 상태와 어긋나지 않게 하고, relay가 살아 있는 상태의 loopback direct upgrade 실패는 경고 노이즈로 분류한다. |
| C17 | done | codex | `cargo test sync_status_watch --workspace` | `sync-status --watch` 기본 화면에서 추정 mesh graph를 제거하고 `direct`/`sent`/`to_send` 중심의 운영 대시보드로 가독성을 개선한다. |
| C18 | done | codex | `cargo test mesh_ --workspace`, `scripts/check.sh --fast` | 최종 UX 방향에 맞춰 `rr mesh --watch`를 별도 local mesh dashboard로 추가하고, `Peer Ring`/`Outbox`/`Flow Lanes`로 tracker health, queue trend, local edge 상태를 분리해 보여준다. |

## 완료/미완료/다음 액션

- 완료: C1-C6, C8-C18.
- 미완료: C7.
- 다음 액션: release/push 후 private install archive를 public relay DNS 주소 기준으로 갱신한다. 외부 WAN L4 포워딩은 cluster/repo 내부 배포와 별도 운영 체크포인트로 확인한다.
- 검증 증거: `python3 -m py_compile install/rustory.py`, installer temp HOME background autostart smoke, `cargo test sync_status_report_includes_pending_push_and_filter discover_targets_skips_self_peer_book_entries_by_peer_id_and_device_id`, `cargo test tracker_target`, `cargo test tracker_announce`, `cargo test inbound_push_provenance`, `cargo test discover_targets`, `cargo test daemon_`, `RUSTORY_RELEASE_LINUX_BUILDER=zig scripts/build-release-assets.sh --target x86_64-unknown-linux-gnu --dist-dir /tmp/rustory-release-portable-linux`, `strings /tmp/rustory-release-portable-linux/rr-x86_64-unknown-linux-gnu | rg GLIBC_ | sort -u` (max `GLIBC_2.17`), `cargo fmt --all --check`, `cargo test relay_addr_warning_flags_tailnet_and_private_addresses`, `cargo test advance_last_pushed_seq_never_moves_cursor_backward`, `cargo test loopback_direct_dial_failure_is_log_noise`, `cargo test import_hishtory_sqlite_preserves_metadata_and_is_idempotent`, `cargo test mesh_ --workspace`, `cargo test sync_status_watch --workspace`, `target/debug/rr mesh`, `cargo test p2p --workspace`, `cargo test --workspace` (226 passed), `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/check.sh --fast`, k8s `rustory-relay` Pod/Service Running, internal DNS `rustory-relay` TCP/4001 success, public WAN IP TCP/4001 refused.
