# Daemon / Scheduler (`rr daemon`)

실사용 디바이스의 기본 실행면은 `rr daemon`이다.
이 명령은 내부에서 두 하위 프로세스를 supervision한다.
- `rr p2p-serve`: 이 디바이스를 tracker에 등록하고 inbound pull/push 요청에 응답한다.
- `rr p2p-sync --watch --push`: 주기적으로 peer를 찾아 pull/push를 반복한다.

이 문서는 이를 백그라운드(로그인 시 자동 시작, 죽으면 재시작)로 돌리는 예시를 정리한다.
현재 CLI 옵션, default, config resolver, signal 처리 세부는 `rr daemon --help`, `rr p2p-serve --help`, `rr p2p-sync --help`, `rr doctor`, config template, 관련 코드를 직접 확인한다.
아래 launchd/systemd 단편은 운영 형태 예시이며, 템플릿의 최신 내용은 `contrib/daemon/*`가 소유한다.

## 권장 전제
- 설정은 `~/.config/rustory/config.toml`에 넣고, 데몬 실행 커맨드는 짧게 유지한다.
  - `trackers`, `relay_addr`, `swarm_key_path`, `p2p_identity_key_path`, `tracker_token` 등
- `user_id`, `device_id`는 고정값을 사용한다(환경변수 또는 config).
- `rr daemon`을 쓰면 `p2p-serve`와 `p2p-sync --watch --push`를 한 서비스로 같이 관리한다.
- 분리 운영 시 `p2p-serve` 없이 `p2p-sync --watch --push`만 실행하면 tracker에 이 디바이스가 등록되지 않는다.
- `--push`는 **로컬 디바이스 엔트리만** 전송한다(`entry.device_id == local_device_id`).
- `rr daemon` 중지(SIGTERM/Ctrl-C)는 하위 serve/sync 프로세스까지 종료한다.
- 여러 디바이스가 같은 주기로 동시에 시작하면 요청이 몰릴 수 있으니, 필요하면 `--start-jitter-sec`을 켠다.

## Enable 전 preflight

launchd/systemd에 등록하기 전에 각 머신에서 foreground 실행으로 다음을 확인한다.
현재 출력 필드와 default는 CLI help와 관련 코드를 직접 확인한다.

```sh
rr doctor
rr doctor --json
rr swarm-key
rr sync-status --with-tracker
```

체크할 기준:
- `rr doctor`가 config invalid 상태로 시작하지 않는다.
- 같은 swarm에 넣을 머신들의 `rr swarm-key` fingerprint가 같다.
- `user_id`는 같고 `device_id`는 머신마다 다르다.
- tracker가 reachable이고, tracker token 설정이 양쪽에서 일치한다.
- relay multiaddr에는 영속화된 relay PeerId가 들어 있다.
- `rr daemon` foreground 실행 후 tracker에 peer가 등록되고 relay reservation이 발생한다.
- `rr daemon`의 sync watch 1주기에서 pull/push summary가 출력된다.

실사용 readiness에서는 direct-only 성공을 합격 증거로 보지 않는다.
서로 다른 NAT/WiFi/router 뒤 peer를 대상으로 tracker + relay circuit 경로가 실제로 쓰였는지 확인한 뒤 daemon으로 전환한다.

## 실행 커맨드 예시
설정 파일을 이미 채워뒀다면:

```sh
rr daemon
```

분리 운영이 필요하면 다음 두 명령을 별도 서비스로 관리한다.

```sh
rr p2p-serve
```

```sh
rr p2p-sync --watch --interval-sec 60 --start-jitter-sec 10 --push
```

CLI로 다 넣는 형태(예시):

```sh
rr --db-path "$HOME/.rustory/history.db" daemon \
  --interval-sec 60 \
  --start-jitter-sec 10 \
  --swarm-key "$HOME/.config/rustory/swarm.key" \
  --trackers "http://<tracker-host>:8850" \
  --relay "/ip4/<relay-ip>/tcp/<port>/p2p/<relay_peer_id>"
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
