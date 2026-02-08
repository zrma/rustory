# Daemon / Scheduler (`p2p-sync --watch`)

`rr p2p-sync --watch`는 “주기적으로 pull/push를 반복”하는 긴 실행 프로세스다.
이 문서는 이를 백그라운드(로그인 시 자동 시작, 죽으면 재시작)로 돌리는 예시를 정리한다.

## 권장 전제
- 설정은 `~/.config/rustory/config.toml`에 넣고, 데몬 실행 커맨드는 짧게 유지한다.
  - `trackers`, `relay_addr`, `swarm_key_path`, `p2p_identity_key_path`, `tracker_token` 등
- `user_id`, `device_id`는 고정값을 사용한다(환경변수 또는 config).
- `--push`는 **로컬 디바이스 엔트리만** 전송한다(`entry.device_id == local_device_id`).
- `--watch` 실행 중 중지(SIGTERM/Ctrl-C)를 받으면 빠르게 종료한다(서비스 매니저 stop에 정상 반응).
- 여러 디바이스가 같은 주기로 동시에 시작하면 요청이 몰릴 수 있으니, 필요하면 `--start-jitter-sec`을 켠다.

## 기본 실행 커맨드(예시)
설정 파일을 이미 채워뒀다면:

```sh
rr p2p-sync --watch --interval-sec 60 --start-jitter-sec 10 --push
```

CLI로 다 넣는 형태(예시):

```sh
rr --db-path "$HOME/.rustory/history.db" p2p-sync \
  --watch --interval-sec 60 \
  --start-jitter-sec 10 \
  --push \
  --swarm-key "$HOME/.config/rustory/swarm.key" \
  --trackers "http://<tracker-host>:8850" \
  --relay "/ip4/<relay-ip>/tcp/<port>/p2p/<relay_peer_id>"
```

## macOS (launchd, user agent)

### 1) plist 예시
파일: `~/Library/LaunchAgents/com.rustory.p2p-sync.plist`

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.rustory.p2p-sync</string>

  <key>ProgramArguments</key>
  <array>
    <string>/Users/YOU/.cargo/bin/rr</string>
    <string>p2p-sync</string>
    <string>--watch</string>
    <string>--interval-sec</string>
    <string>60</string>
    <string>--push</string>
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
  <string>/tmp/rustory-p2p-sync.out.log</string>
  <key>StandardErrorPath</key>
  <string>/tmp/rustory-p2p-sync.err.log</string>
</dict>
</plist>
```

### 2) 시작/중지/로그
```sh
# 로드(활성화)
launchctl bootstrap "gui/$UID" ~/Library/LaunchAgents/com.rustory.p2p-sync.plist
launchctl enable "gui/$UID/com.rustory.p2p-sync"

# 즉시 시작(재시작 강제는 -k)
launchctl kickstart -k "gui/$UID/com.rustory.p2p-sync"

# 상태 확인
launchctl print "gui/$UID/com.rustory.p2p-sync"

# 언로드(비활성화)
launchctl bootout "gui/$UID" ~/Library/LaunchAgents/com.rustory.p2p-sync.plist

# 로그 확인(위 plist 경로 기준)
tail -f /tmp/rustory-p2p-sync.err.log
```

## Linux (systemd --user)

### 1) unit 예시
파일: `~/.config/systemd/user/rustory.service`

```ini
[Unit]
Description=Rustory p2p-sync watch

[Service]
ExecStart=%h/.cargo/bin/rr p2p-sync --watch --interval-sec 60 --push
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
