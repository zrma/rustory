# Disposable device-retirement VM acceptance

이 acceptance는 production node를 희생하지 않고 cooperative remote full uninstall의 실제 OS manager
경계를 검증한다. Tart의 local APFS clone과 Lima의 local VM만 사용하므로 cloud VM 비용은 들지 않는다.
base VM은 재사용하고, 각 시나리오는 고정 이름의 clone에서 실행한 뒤 삭제한다.

## 소유 경계

- 준비·상태·fault proxy·안전 cleanup 진입점: `scripts/acceptance_device_retirement_vms.sh`
- HTTPS fault mode: `scripts/acceptance/device-retirement/Caddyfile*`
- 제품 동작과 옵션: `rr --help`, `rr device --help`, `install/rustory.py --help`
- 제품 보안 경계와 rollout 정책: `docs/security.md`, `docs/p2p.md`

스크립트의 `cleanup --yes`는 고정된 scenario clone만 삭제하고 Tart/Lima base와 다른 VM은 건드리지
않는다. production tracker, production token, production `swarm.key`, production node는 이 acceptance에
사용하지 않는다.

## 순차 시나리오

1. `preflight`로 Tart, Lima, Caddy, Tailscale, 현재 `rr` build, base VM과 fault config를 확인한다.
2. host loopback tracker 앞에 VM gateway로 bind한 HTTPS Caddy를 두고, acceptance 전용 fleet/admin
   token과 private tracker security state를 사용한다.
3. Tart clone에서 launchd managed daemon을 enrollment한 뒤 정상 retirement를 수행한다. membership
   revoke, local full uninstall, completion과 범위 밖 sentinel 보존을 분리해 확인하고 재부팅 후에도
   daemon/helper가 복구되지 않는지 본다.
4. 두 번째 Tart clone은 완전히 정지한 상태에서 ticket을 발급한다. tracker에는 revoke와 pending
   ticket이 즉시 남고, clone 재시작 뒤에만 local cleanup이 실행되어 completed로 수렴해야 한다.
5. 세 번째 Tart clone에서 `caddy fail-ack`로 Running ACK만 503 처리한다. 이 동안 ticket은 pending,
   원본 binary/config/daemon은 유지되고 ticket별 launchd helper만 backoff 재시도해야 한다. normal
   proxy 복구 뒤에만 cleanup과 completion이 일어난다.
6. Lima clone에서 systemd-user managed daemon을 enrollment하고 `caddy fail-complete`로 completion만
   503 처리한다. Running ACK 뒤 원본 관리 파일은 삭제되지만 ticket은 running이고 receipt, helper
   copy, enabled ticket별 unit은 남아야 한다.
7. 6번 중간 상태에서 Lima clone을 재부팅한다. helper가 자동 재개되고 normal proxy 복구 뒤 completed,
   receipt/helper/unit 제거로 수렴해야 한다. 범위 밖 sentinel은 끝까지 보존한다.
8. `status`로 base만 남았는지 확인하고 `cleanup --yes`로 fixed scenario clone을 정리한다.

ACK fault는 실제 tracker를 중지하지 않는다. admin 조회와 ticket 발급 경로를 계속 사용할 수 있어야
하며, Caddy가 정확히 ACK 또는 complete endpoint만 차단한다. 대상이 삭제를 시작할 수 있는 시점과
tracker가 terminal status를 기록하는 시점을 독립적으로 관찰하기 위한 경계다.

## 2026-07-12 검증 증거

- macOS 26.5 Tart/launchd: 정상, offline ticket 후 reconnect, Running ACK 503 retry를 통과했다.
- Ubuntu 26.04 Lima/systemd-user: completion ACK 503 상태의 cleanup, VM 재부팅, enabled helper 재개,
  proxy 복구 뒤 receipt/helper/unit 정리를 통과했다.
- 모든 시나리오에서 revoke는 즉시 반영됐고, fixed full-uninstall 외 임의 command/path는 전달되지
  않았다. 대상 opt-in과 OS manager가 없는 production node에서는 이 경로를 enable하지 않는다.
- Linux x86_64 release asset은 ARM Lima에서 Rosetta로 실행됐다. macOS native relay와의 private-swarm
  연결은 Rosetta 조합에서 `Decrypt`가 재현되어 Linux clone 내부의 disposable x86_64 relay로
  enrollment transport를 격리했다. 이는 실제 x86_64 Linux node의 출고 판정 근거로 사용하지 않으며,
  release asset의 glibc compatibility는 별도 release gate가 소유한다.

## 중단 기준

- revoke 후 대상이 tracker에 active로 남거나 새 register가 허용된다.
- Running ACK 성공 전에 관리 파일 삭제가 시작된다.
- completion 실패 중 receipt/helper가 사라져 재부팅 후 복구할 수 없다.
- full uninstall이 관리 범위 밖 sentinel을 삭제한다.
- proxy 정상화 뒤에도 terminal completion 또는 helper self-cleanup으로 수렴하지 않는다.

하나라도 발생하면 production full retirement enablement를 중단하고 membership revoke-only로 유지한다.
