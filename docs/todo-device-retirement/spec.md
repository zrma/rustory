# Spec: device-retirement

## 배경

- 요청 맥락: 기존 `rr uninstall`은 대상 머신이 스스로 탈퇴하는 경로만 제공한다. 관리 머신에서 특정 장비의 grid membership을 즉시 박탈하고, 온라인인 정상 대상에는 같은 안전 경계를 재사용한 로컬 uninstall까지 요청할 수 있어야 한다.
- 현재 문제: tracker unregister는 일시적인 presence 삭제일 뿐이며 shared tracker token과 `swarm.key`를 가진 노드는 새 identity로 재등록할 수 있다. 또한 inbound P2P authorization은 tracker보다 로컬 `peer_book`을 먼저 신뢰해 퇴역한 PeerId를 계속 허용할 수 있다.
- 파괴적 경계: 관리 측 revoke는 강제할 수 있지만 대상 파일 삭제는 대상 OS의 협조적 실행이다. 임의 remote command/path를 허용하지 않고, 사전 opt-in된 daemon만 고정형 full-uninstall ticket을 별도 one-shot helper로 수행한다.

## 계획 스냅샷

- 목표: admin/device 인증을 분리한 durable device enrollment/revoke와 cooperative remote uninstall을 구현한다.
- 범위:
  - tracker admin token, private durable security-state 파일, identity-signed device request와 explicit enrollment를 추가한다.
  - revoke가 register/list/P2P inbound authorization과 stale `peer_book`보다 우선하도록 한다.
  - 관리 CLI에 device list/enroll/revoke/retire/status를 추가하고 destructive retire는 `--yes`를 요구한다.
  - 대상 daemon은 `allow_remote_retirement=true`이고 launchd/systemd-user recovery manager가 있을 때만 고정형 ticket을 poll하고, 별도 manager job/process group의 internal helper가 기존 uninstall executor를 재사용한다.
  - cleanup 뒤에는 fleet token/identity가 아닌 ticket-scoped completion capability receipt로 ACK를 복구하고 확인 뒤 helper/receipt를 자동 정리한다.
  - status는 membership revoke와 cleanup pending/running/completed/failed를 분리한다.
  - 구버전 tracker 응답은 additive JSON 필드로 계속 읽되, 강한 revoke 보장은 enrollment-required tracker와 revoke-aware peer만으로 구성된 fleet에서만 활성화한다.
- 제외:
  - 임의 shell command/원격 경로 전달, root/MDM 수준 secure erase, 오프라인·침해 대상의 로컬 파일 삭제 보장.
  - 이번 slice에서 public release, live tracker 설정 변경, production fleet 배포나 production 장비 retirement 실행.
- 보안 불변조건:
  - admin API는 일반 fleet token만으로 호출할 수 없고 별도 admin token을 함께 요구한다.
  - device proof의 public key는 PeerId와 일치해야 하며 timestamp/nonce/signature를 검증한다.
  - enrollment-required 모드에서는 등록된 identity 외 새 PeerId가 shared token을 알아도 가입할 수 없다.
  - revoke record는 tracker 재시작 후에도 남고, 로컬 deny cache가 peer book보다 먼저 평가된다.
  - remote ticket은 fixed cleanup enum만 포함하며 대상의 명시적 opt-in 없이는 파일을 삭제하지 않는다.
  - receipt에는 fleet token이나 identity private key를 복제하지 않고, completion capability는 해당 ticket의 `Running → Completed` 전이에만 쓴다.
- 검증 명령: `scripts/acceptance_device_retirement_vms.sh preflight`, `scripts/check-release-gates.sh --manifest-mode full --work-id device-retirement`.
- 완료 기준: C1..C8이 `done`이고 보안 우회/정상 동작/전체 repository gate가 모두 재현 가능하게 통과한 뒤 todo를 VCS closeout에서 삭제한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test tracker_device_proof --workspace` | PeerId-bound proof, skew, nonce replay, 정상 signed registration 검증 |
| C2 | done | codex | `cargo test tracker_device_enrollment --workspace` | admin token 분리, explicit enrollment, unknown identity fail-closed 구현 |
| C3 | done | codex | `cargo test tracker_device_retirement --workspace` | durable revoke/ticket/status와 register/list 차단 구현 |
| C4 | done | codex | `cargo test revoked_peer --workspace` | tracker revoke를 local deny cache에 반영하고 stale peer book보다 우선 |
| C5 | done | codex | `cargo test retirement --workspace` | fixed ticket, local opt-in, one-shot helper scheduling과 cleanup ACK 상태 검증 |
| C6 | done | codex | `cargo test cli --workspace` | device list/enroll/retire/status CLI, destructive confirmation, secret redaction 검증 |
| C7 | done | codex | `python3 install/test_rustory.py` | installer/config/service migration과 remote-retirement opt-in 회귀 검증 |
| C8 | in_progress | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id device-retirement` | disposable OS acceptance는 통과. 최종 working copy gate와 VCS closeout에서 todo 삭제가 남음 |

## 완료/미완료/다음 액션

- 완료: C1..C7과 disposable OS acceptance. macOS launchd 정상/offline/ACK retry, Linux systemd-user cleanup/completion retry/reboot recovery가 모두 수렴했고 범위 밖 sentinel이 보존됐다.
- 미완료: C8의 최종 working copy gate 및 사용자 승인 뒤 VCS closeout. production enablement와 실제 node retirement는 별도 change-control 경계다.
- 다음 액션: full gate를 통과한 working copy를 커밋할 권한을 받으면 C8을 `done`으로 바꾸고 todo 폴더를 삭제하는 closeout change를 만든다.
- 검증 증거: `cargo test --workspace` (412 passed), `cargo test --no-default-features --workspace` (395 passed), `cargo clippy --workspace --all-targets -- -D warnings`, `python3 install/test_rustory.py` (20 passed, 1 skipped), P2P smoke, Zig Linux x86_64 build `glibc_compat=ok required_max=2.17 allowed_max=2.17`, 독립 보안 재감사 P0/P1 없음, `docs/acceptance/device-retirement-vms.md`의 2026-07-12 Tart/Lima evidence.
