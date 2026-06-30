# Spec: installer-daemon-service

## 배경

- 요청 맥락: 신규 머신에서 one-shot installer만 실행해도 Rustory가 tracker/relay grid의 상시 멤버가 되어야 한다.
- 현재 문제/기회: hook/import만 끝나고 daemon 등록이 빠지면 peer가 tracker에서 금방 사라진다. 또한 tailnet/private relay 주소는 외부 신규 머신에서 dial할 수 없으므로 public relay multiaddr 기준이 필요하다.

## 계획 스냅샷

- 목표: installer가 macOS/Linux user service를 설치하고, public relay 주소를 기준으로 운영 문서와 진단 경고를 맞춘다.
- 범위: installer, CLI doctor 경고, DNS relay multiaddr transport, public 문서, private install archive 갱신.
- 검증 명령: `python3 -m py_compile install/rustory.py`, `RUSTORY_RELEASE_LINUX_BUILDER=zig scripts/build-release-assets.sh --target x86_64-unknown-linux-gnu --dist-dir /tmp/rustory-release-portable-linux`, `cargo test relay_addr_warning_flags_tailnet_and_private_addresses`, `scripts/check.sh --fast`, `scripts/run-manifest-checks.sh --mode quick --work-id installer-daemon-service`.
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

## 완료/미완료/다음 액션

- 완료: C1-C6, C8-C9.
- 미완료: C7.
- 다음 액션: release/push 후 private install archive를 public relay DNS 주소 기준으로 갱신한다. 외부 WAN L4 포워딩은 cluster/repo 내부 배포와 별도 운영 체크포인트로 확인한다.
- 검증 증거: `python3 -m py_compile install/rustory.py`, `RUSTORY_RELEASE_LINUX_BUILDER=zig scripts/build-release-assets.sh --target x86_64-unknown-linux-gnu --dist-dir /tmp/rustory-release-portable-linux`, `strings /tmp/rustory-release-portable-linux/rr-x86_64-unknown-linux-gnu | rg GLIBC_ | sort -u` (max `GLIBC_2.17`), `cargo fmt --all --check`, `cargo test relay_addr_warning_flags_tailnet_and_private_addresses`, `cargo test --workspace` (212 passed), `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/check.sh --fast`, k8s `rustory-relay` Pod/Service Running, internal DNS `rustory-relay` TCP/4001 success, public WAN IP TCP/4001 refused.
