# Daemon / Scheduler (`rr daemon`)

실사용 디바이스의 기본 실행면은 `rr daemon`이다.
이 명령은 내부에서 두 하위 프로세스를 supervision한다.
- `rr p2p-serve`: 이 디바이스를 tracker에 등록하고 inbound pull/push 요청에 응답한다.
- `rr p2p-sync --watch --push`: 주기적으로 peer를 찾아 pull/push를 반복한다.

이 문서는 이를 백그라운드(로그인 시 자동 시작, 죽으면 재시작)로 돌리는 예시를 정리한다.
현재 CLI 옵션, default, config resolver, signal 처리 세부는 `rr daemon --help`, `rr p2p-serve --help`, `rr p2p-sync --help`, `rr doctor`, config template, 관련 코드를 직접 확인한다.
아래 launchd/systemd 단편은 운영 형태 예시이며, 템플릿의 최신 내용은 `contrib/daemon/*`가 소유한다.

신규 머신 온보딩에서는 installer의 `--install-daemon`을 기본 경로로 사용한다.

```sh
curl -fsSL https://raw.githubusercontent.com/zrma/rustory/main/install/rustory.py | \
  python3 - --token "$RUSTORY_TRACKER_TOKEN" --tracker "<tracker-url>" \
    --relay "<relay-multiaddr>" --user-id "<shared-user-id>" \
    --swarm-key-b64 "<base64-swarm-key>" \
    --install-hook --install-daemon --import-hishtory
```

macOS는 `~/Library/LaunchAgents/com.rustory.daemon.plist`, Linux는
`~/.config/systemd/user/rustory.service`가 생성된다. 설치만 하고 시작을 미루려면
`--no-start-daemon`을 함께 넘긴다.

Linux에서 SSH 비로그인 세션, container-like shell, `sudo -u` 환경처럼 user systemd
bus가 없는 상태에서는 installer가 unit 파일을 설치한 뒤 `daemon=start_deferred`를
출력하고 `manager=background` fallback으로 `rr daemon`을 즉시 시작한다. fallback은
`~/.local/state/rustory/daemon.pid`와 `~/.local/state/rustory/daemon.log`를 사용하며,
같은 rc 파일에 managed shell autostart block을 추가해 컨테이너 재시작 뒤 첫 interactive
shell에서 죽은 daemon을 다시 띄운다. 이 autostart block을 원하지 않으면
`--no-daemon-shell-autostart`를 함께 넘긴다. 장기 운영 서버에서는 가능하면 같은 사용자
로그인 세션에서 아래 `systemctl --user` 명령을 직접 실행해 systemd-user 관리로 전환한다.

```sh
systemctl --user daemon-reload
systemctl --user enable --now rustory.service
systemctl --user status rustory.service
```

컨테이너처럼 user systemd manager가 없는 환경에서는 fallback daemon이 해당 process/container
lifetime 동안 grid presence를 유지하고, 재시작 뒤에는 다음 interactive shell startup에서
자동 복구된다. shell이 한 번도 열리지 않으면 daemon도 자동으로 떠 있지 않다.

## 관리 로그 상한과 자동 정리

Rustory가 직접 소유하는 daemon 로그는 64 MiB를 초과하면 파일을 제자리에서 비운다. `rr daemon`은
시작 직후 한 번 확인하고 실행 중에는 60초마다 다시 확인하므로, launchd나 background fallback에서
반복 오류가 발생해도 로그가 무제한으로 자라지 않는다. 정리 대상은 다음 경로로 제한된다.

- macOS launchd: `~/Library/Logs/rustory-daemon.out.log`,
  `~/Library/Logs/rustory-daemon.err.log`
- Linux background fallback: `${XDG_STATE_HOME:-$HOME/.local/state}/rustory/daemon.log`

systemd-user가 journald에 기록한 로그는 Rustory가 삭제하지 않는다. journald 보존 정책은 시스템
운영 정책으로 관리한다. 정리 코드는 현재 사용자 소유의 단일-link 일반 파일만 열며 symbolic link,
hard link, 디렉터리는 거부한다.

데몬 재시작을 기다리지 않고 여러 머신에서 같은 정책을 적용하려면 각 노드에서 다음 명령을 실행한다.
상한 이하는 보존하고, 상한을 초과한 Rustory 관리 로그만 비운다. `rr doctor --auto-fix`도 같은 정리를
수행한다.

```sh
rr logs cleanup
```

`rr update`는 기본적으로 관리 중인 daemon을 재시작한다. 새 release asset이 현재 설치
파일과 byte-identical이어도 restart를 시도하므로, 오래 떠 있던 daemon child가 이전
인자/default로 남아 있을 때 `rr update`를 다시 실행하는 것이 안전한 복구 경로가 된다.
macOS launchd, Linux systemd-user, Linux background fallback pid 파일을 순서대로 처리하므로
일반 업데이트 뒤에 수동 재시작을 별도로 기억할 필요는 없다. 단,
`rr update --no-restart-daemon`을 사용했거나 service/fallback이 아직 설치되지 않은 머신에서는
아래 플랫폼별 수동 명령으로 직접 재시작한다.

Linux background fallback은 `daemon.pid`의 process 하나만 종료하지 않고, daemon이 독립 process
group leader인 경우 그 group을 종료한 뒤 같은 설치 경로에서 떠 있는 오래된 `rr daemon`/`rr p2p-serve`/`rr p2p-sync`
잔여 process도 정리하고 새 daemon을 띄운다. fallback shell autostart는 `setsid`가 있으면 새 session으로
daemon을 시작한다. 외부망 컨테이너 로그에서 같은 파일에 `info:`와
`warn:` timeout 로그가 섞이거나 새 default와 맞지 않는 `p2p-sync tick`이 계속 보이면 오래된
fallback child가 남은 신호로 보고 `rr update`를 다시 실행해 정리한다.

## 권장 전제
- 설정은 `~/.config/rustory/config.toml`에 넣고, 데몬 실행 커맨드는 짧게 유지한다.
  - `trackers`, `relay_addr`, `swarm_key_path`, `p2p_identity_key_path`, `tracker_token` 등
- `user_id`, `device_id`는 고정값을 사용한다(환경변수 또는 config).
- `rr daemon`을 쓰면 `p2p-serve`와 `p2p-sync --watch --push`를 한 서비스로 같이 관리한다.
- 분리 운영 시 `p2p-serve` 없이 `p2p-sync --watch --push`만 실행하면 tracker에 이 디바이스가 등록되지 않는다.
- inbound P2P 요청은 요청자의 PeerId가 tracker/peerbook의 같은 user scope에 있어야 통과하므로, 분리 운영에서도 `p2p-serve`와 `p2p-sync`가 같은 `p2p_identity_key_path`를 사용해야 한다.
- `--push`는 **로컬 디바이스 엔트리만** 전송한다(`entry.device_id == local_device_id`).
- `rr daemon` 중지(SIGTERM/Ctrl-C)는 하위 serve/sync 프로세스까지 종료한다.
- 여러 디바이스가 같은 주기로 동시에 시작하면 요청이 몰릴 수 있으니, 필요하면 `--start-jitter-sec`을 켠다.
- `rr daemon`은 큰 Hishtory import/backfill 수렴을 우선해 기본값으로 모든 tracker-discovered peer를 매 tick 시도한다. 작은 self-hosted relay에 fan-out이 몰리면 `--max-peers-per-tick <n>`으로 낮춘다.

## Enable 전 preflight

launchd/systemd에 등록하기 전에 각 머신에서 foreground 실행으로 다음을 확인한다.
현재 출력 필드와 default는 CLI help와 관련 코드를 직접 확인한다.

```sh
rr doctor
rr doctor --json
rr swarm-key
rr sync-status --with-tracker
rr daemon --preflight
```

체크할 기준:
- `rr doctor`가 config invalid 상태로 시작하지 않는다.
- 같은 swarm에 넣을 머신들의 `rr swarm-key` fingerprint가 같다.
- `user_id`는 같고 `device_id`는 머신마다 다르다.
- tracker가 reachable이고, tracker token 설정이 양쪽에서 일치한다.
- relay multiaddr에는 영속화된 relay PeerId가 들어 있다.
- `rr daemon --preflight`가 자식 프로세스를 띄우기 전에 configured tracker ping을 통과한다.
- `rr daemon` foreground 실행 후 tracker에 peer가 등록되고 relay reservation이 발생한다.
- `rr daemon`의 sync watch 1주기에서 pull/push summary가 출력된다.

## Cooperative remote retirement

remote full uninstall은 기본적으로 꺼져 있다. strict enrollment 전환을 끝낸 뒤 대상별로 다음 두
설정을 함께 켜고 managed daemon을 재시작해야 한다.

```toml
require_device_membership = true
allow_remote_retirement = true
```

installer 자동화에서는 HTTPS tracker를 정확히 하나 지정하고 다음 flag를 함께 쓴다.

```sh
python3 install/rustory.py \
  --tracker 'https://tracker.example' \
  --require-device-membership \
  --allow-remote-retirement \
  --install-daemon
```

기존 `config.toml`이 있으면 installer는 이를 `--force`로 재생성하지 않는다. 대신 두 security key만
atomic하게 `true`로 병합하고 주석, plugin table, 기타 설정을 그대로 보존한다. 이때 authoritative HTTPS
tracker가 이미 config에 정확히 하나 저장되어 있지 않으면 성공처럼 보이는 partial activation 대신
실패한다. 먼저 config를 명시적으로 수정한 뒤 재시도한다.

`rr p2p-serve` child는 managed daemon marker, strict membership, target opt-in이 모두 있을 때만
retirement protocol을 signed registration에 광고한다. opt-in, user/device id, tracker와 cleanup 관련
DB/key/marker 경로는 helper가 재구성할 수 있게 `config.toml`에 저장되어 effective runtime과 일치해야
한다. env-only opt-in 또는 CLI/env path override가 config와 다르면 parent는 capability와 monitor를 모두
끄고 revoke-only로 남는다. daemon parent가 fixed ticket을 poll하고 다음 별도 execution context로
internal helper를 넘긴다.

- macOS: `com.rustory.retire.<ticket>` one-shot launchd agent. 기존
  `com.rustory.daemon`을 `bootout`해도 helper는 별도 label이라 생존한다.
- Linux systemd-user: ticket별 `rustory-retire-<ticket>.service` recovery unit을 private file로 만들고
  enable한 뒤 기존 `rustory.service`와 다른 cgroup에서 실행한다. `Restart=on-failure`와 boot enable로
  crash/reboot를 복구하며 handoff 실패 시 background로 우회하지 않고 파일 삭제 전에 실패한다.
- Linux background fallback: membership revoke는 지원하지만 full-uninstall capability는 광고하지 않는다.
  shell-start process만으로는 cleanup 중 crash/reboot 뒤 recovery helper를 확실히 다시 실행할 수 없기
  때문이다. full uninstall이 필요하면 systemd-user와 linger를 정상 구성한다.

installer가 service를 만들 때 resolved absolute `XDG_STATE_HOME`을 launchd/systemd environment에 고정해
daemon과 recovery helper가 install 시점과 같은 receipt/log state를 사용한다. fixed private
`managed-state-home` metadata로 이후 `rr update`/uninstall에도 같은 값을 복원한다. bounded
`managed-state-homes.json`은 이전 값까지 보존해 state home 변경 뒤 남은 Rustory state도 uninstall에서
정리한다. immediate/background
rc child에도 raw `~/...`나 빈 env 대신 resolved absolute 값을 고정한다. systemd-user parent는 모든
config/key/cleanup preflight를 통과한 뒤 exact managed signature가 일치하는 기존 background
parent/child를 먼저 정리하고, rc block도 active unit을 확인한다. 따라서 manual takeover 직후나 이후
interactive shell에서 duplicate daemon이 생기지 않는다.

helper argument는 canonical ticket UUID 하나뿐이고 remote ticket에는 실행 command나 local path가
없다. daemon은 시작 시 effective DB/config/key/state/managed-rc 경로를 검증해 exact cleanup plan을
만들고, scheduler는 현재 `rr`를 private helper로 복사하면서 이 plan과 identity key의 경로를 0600
receipt에 먼저 고정한다. helper는 Pending ticket에서 local config/identity/user/device/tracker와 opt-in을
다시 대조하고 ticket별 256-bit completion capability를 receipt에 내구성 있게 기록한 뒤 tracker가
hash-bound `Running` ACK를 확인해야만 기존 uninstall executor를 호출한다. config가 그 사이 바뀌어도
삭제 경로는 startup-pinned plan에서 늘어나거나 바뀌지 않는다. receipt에는 fleet token이나 identity
private key가 들어가지 않는다.
manager stop이 불완전하면 기존 uninstall과 같이 DB/config/binary 삭제 전에 중단한다.

cleanup이 성공하면 receipt에 먼저 완료를 기록한 뒤, 이미 config/identity/binary가 없어도 completion
capability만으로 `Completed` ACK를 확인할 때까지 재시도한다. tracker는 capability hash만 보관하고 이
경로는 해당 ticket의 `Running → Completed` 외 상태를 바꿀 수 없다. ACK 확인 후 helper copy, manager
artifact/enable link, receipt를 삭제한다. helper crash/reboot는 launchd의 persistent agent 또는
systemd의 enabled recovery unit이 같은 receipt에서 재개한다. device proof로 인증하는 poll/Running ACK는
fleet bearer token을 receipt에 넣지 않으므로 token rotation에도 복구된다. scheduling 실패는 Pending에서
`Failed`, 실행 직전 target preflight가 실패하면 `Refused`로 남고 같은 admin retire 명령으로
precondition을 재검증해 재큐잉할 수 있다. `Running → Failed`는 허용하지 않아 늦게 도착한 scheduler
오류가 이미 승인된 cleanup을 덮어쓰지 못한다. cleanup 도중
오류는 destructive step을 idempotent하게 재시도하기 위해 `Running`을 유지한다.

이 기능은 협조적인 정상 대상의 정리를 자동화한다. offline 대상은 ticket을 보관했다가 재접속 때
처리하고, 침해된 대상이나 root/MDM 수준 파일 삭제를 강제하지 않는다. membership revoke는 cleanup
성공 여부와 무관하게 유지된다.

실사용 readiness에서는 direct-only 성공을 합격 증거로 보지 않는다.
서로 다른 NAT/WiFi/router 뒤 peer를 대상으로 tracker + relay circuit 경로가 실제로 쓰였는지 확인한 뒤 daemon으로 전환한다.

## 실행 커맨드 예시
설정 파일을 이미 채워뒀다면:

```sh
rr daemon
```

전환 직전에는 다음처럼 tracker reachable/token mismatch를 먼저 실패시킬 수 있다.
부팅 직후 네트워크가 늦게 붙는 서비스 매니저 환경에서는 기본 `rr daemon`을 유지하고,
수동 검증이나 배포 전 체크에서만 `--preflight`를 쓰는 편이 운영상 안전하다.

```sh
rr daemon --preflight
```

분리 운영이 필요하면 다음 두 명령을 별도 서비스로 관리한다.

```sh
rr p2p-serve
```

```sh
rr p2p-sync --watch --interval-sec 60 --start-jitter-sec 10 --max-peers-per-tick 1 --push
```

CLI로 다 넣는 형태(예시):

```sh
rr --db-path "$HOME/.rustory/history.db" daemon \
  --interval-sec 60 \
  --start-jitter-sec 10 \
  --max-peers-per-tick 0 \
  --swarm-key "$HOME/.config/rustory/swarm.key" \
  --trackers "http://<tracker-host>:8850" \
  --relay "/dns4/<relay-host>/tcp/<port>/p2p/<relay_peer_id>"
```

## macOS (launchd, user agent)

### 1) plist 예시
파일:
- `~/Library/LaunchAgents/com.rustory.daemon.plist`

레포 템플릿:
- `contrib/daemon/launchd/com.rustory.daemon.plist`

가장 빠른 방법은 위 템플릿들을 복사해서 `ProgramArguments`의 `rr` 경로와
`RUSTORY_USER_ID`/`RUSTORY_DEVICE_ID`를 환경에 맞게 수정하는 것이다.

`daemon` 예시:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.rustory.daemon</string>

  <key>ProgramArguments</key>
  <array>
    <string>/Users/YOU/.cargo/bin/rr</string>
    <string>daemon</string>
    <string>--interval-sec</string>
    <string>60</string>
    <string>--start-jitter-sec</string>
    <string>10</string>
  </array>

  <key>EnvironmentVariables</key>
  <dict>
    <key>RUSTORY_USER_ID</key>
    <string>zrma</string>
    <key>RUSTORY_DEVICE_ID</key>
    <string>macbook</string>
  </dict>

  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>

  <key>StandardOutPath</key>
  <string>/tmp/rustory-daemon.out.log</string>
  <key>StandardErrorPath</key>
  <string>/tmp/rustory-daemon.err.log</string>
</dict>
</plist>
```

### 2) 시작/중지/로그
```sh
# 로드(활성화)
launchctl bootstrap "gui/$UID" ~/Library/LaunchAgents/com.rustory.daemon.plist
launchctl enable "gui/$UID/com.rustory.daemon"

# 즉시 시작(재시작 강제는 -k)
launchctl kickstart -k "gui/$UID/com.rustory.daemon"

# 상태 확인
launchctl print "gui/$UID/com.rustory.daemon"

# 언로드(비활성화)
launchctl bootout "gui/$UID" ~/Library/LaunchAgents/com.rustory.daemon.plist

# 로그 확인(위 plist 경로 기준)
tail -f /tmp/rustory-daemon.err.log
```

## Linux (systemd --user)

### 1) unit 예시
파일:
- `~/.config/systemd/user/rustory.service`

레포 템플릿:
- `contrib/daemon/systemd/rustory.service`

`daemon` 예시:

```ini
[Unit]
Description=Rustory daemon

[Service]
ExecStart=%h/.cargo/bin/rr daemon --interval-sec 60 --start-jitter-sec 10
Restart=always
RestartSec=5
Environment=RUSTORY_USER_ID=zrma
Environment=RUSTORY_DEVICE_ID=laptop

[Install]
WantedBy=default.target
```

### 2) 시작/중지/로그
```sh
systemctl --user daemon-reload
systemctl --user enable --now rustory.service

systemctl --user status rustory.service
journalctl --user -u rustory.service -f

systemctl --user restart rustory.service
systemctl --user stop rustory.service
```

`systemctl --user`가 `Failed to connect to bus`,
`DBUS_SESSION_BUS_ADDRESS`, `XDG_RUNTIME_DIR` 관련 메시지로 실패하면 현재 shell이
user systemd bus에 붙어 있지 않은 것이다. unit 파일은 이미 설치되어 있으므로
로그인 세션에서 다시 실행하거나, 정책상 허용된다면 `loginctl enable-linger <user>`를
설정한 뒤 재시도한다.

#### (옵션) 로그인 없이도 계속 돌리고 싶다면
배포/운영 정책에 따라 다르지만, “사용자가 로그아웃해도 user service가 계속 실행”되길 원하면
Linux에서 `loginctl enable-linger <user>`를 고려할 수 있다.
환경/보안 정책에 맞게 선택한다.

## Enable 후 운영 확인

서비스 매니저로 전환한 뒤에는 foreground 때와 같은 증거를 다시 남긴다.

```sh
rr doctor --json
rr sync-status --json --with-tracker
```

macOS는 `launchctl print`와 plist의 stderr/stdout 로그를 함께 확인하고, Linux는 `systemctl --user status`와 `journalctl --user -u ...`를 함께 확인한다.
Hishtory migration 중이면 `docs/hishtory-migration.md`의 soak 합격 기준을 통과하기 전까지 Hishtory hook/daemon을 끄지 않는다.
