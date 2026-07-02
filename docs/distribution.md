# Distribution

- Audience: Rustory 운영자, release 관리자
- Owner: Rustory
- Last Verified: 2026-06-30

이 문서는 Rustory binary 배포, 신규 디바이스 온보딩, self-update 경로를 정리한다.
private tracker 주소, token, registry, k8s/NAS 경로는 public repo에 기록하지 않는다.

## Release Asset Contract

`rr update`와 `install/rustory.py`는 GitHub Releases에서 raw executable asset을 받는다.

| Target | Asset |
| --- | --- |
| macOS arm64 | `rr-aarch64-apple-darwin` |
| macOS x86_64 | `rr-x86_64-apple-darwin` |
| Linux x86_64 | `rr-x86_64-unknown-linux-gnu` |
| Linux arm64 | `rr-aarch64-unknown-linux-gnu` |

각 asset 옆에는 같은 URL에 `.sha256` suffix를 붙인 checksum 파일을 둔다.
예: `rr-aarch64-apple-darwin.sha256`.

현재 플랫폼 asset을 만들 때는:

```sh
scripts/build-release-assets.sh
```

출력은 `dist/rr-<target>`, `dist/rr-<target>.sha256`, `dist/checksums.txt`다.
`dist/`는 release upload staging이며 Git에 커밋하지 않는다.

## Release Fast Path

`scripts/release-version.sh`는 version/tag 확인, release gate, asset build, GitHub Release upload,
`rr update --dry-run` 검증을 한 명령으로 묶는다. 기본 version은 `Cargo.toml`의 package
version이고 tag는 `v<version>` 형식이다.

```sh
scripts/release-version.sh --profile current --dry-run
scripts/release-version.sh --profile daily-driver --skip-upload
scripts/release-version.sh --profile daily-driver --gate none
```

프로파일 의미:

| Profile | Targets | 용도 |
| --- | --- | --- |
| `current` | 현재 머신의 target | 로컬 canary 또는 단일 asset 확인 |
| `daily-driver` | `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu` | MacBook + Linux k8s node 실사용 배포 |
| `full` | macOS arm64/x86_64 + Linux x86_64/arm64 | 지원 target 전체 게시 |

권장 순서는 코드 변경과 release asset 게시를 분리하는 것이다.

```sh
scripts/finalize-and-push.sh --message "<type>: <summary>" --work-id "<work-id>"
scripts/release-version.sh --profile daily-driver --gate none
```

같은 턴에서 source 검증까지 release script가 책임져야 하면 `--gate full --work-id <work-id>`를 쓴다.
기본 gate는 이미 main 출고 검증이 끝난 직후의 asset publish를 빠르게 하기 위해 `quick`이다.
Linux target은 현재 host와 target이 같으면 로컬에서 빌드한다. macOS 같은 non-Linux host에서는
`zig`가 있으면 Zig cross C toolchain을 먼저 써서 remote builder의 glibc version을 release asset에
묶지 않는다. baseline override가 필요하면 `RUSTORY_RELEASE_ZIG_GLIBC=<glibc-version>`을 지정한다.
Zig가 없고 `RUSTORY_RELEASE_LINUX_REMOTE=<ssh-host>`가 설정되어 있으면 native Linux SSH builder를 쓰며,
마지막 fallback은 Docker buildx다. 빌더를 고정해야 하면 `RUSTORY_RELEASE_LINUX_BUILDER=zig|ssh|docker|host`
또는 `--linux-builder zig|ssh|docker|host`를 명시한다. 실제 옵션/default는
`scripts/build-release-assets.sh --help`와 스크립트 본문을 기준으로 한다.

`--skip-upload`은 asset/checksum만 만들고 GitHub Release는 건드리지 않는다.
`--dry-run`은 release plan과 실행할 명령만 출력한다.

## One-Line Install

공식 배포 URL이 GitHub raw installer를 가리키면 신규 머신은 아래처럼 시작한다.

```sh
curl -fsSL https://raw.githubusercontent.com/zrma/rustory/main/install/rustory.py | \
  python3 - --token "$RUSTORY_TRACKER_TOKEN" --tracker "<tracker-url>" \
    --relay "<relay-multiaddr>" --user-id "<shared-user-id>" \
    --swarm-key-b64 "<base64-swarm-key>" \
    --install-hook --install-daemon --import-hishtory
```

`--tracker`는 반복 지정하거나 comma-separated 값으로 지정할 수 있다.
relay 주소까지 config에 쓰려면 `--relay "<relay-multiaddr>"`를 같이 넘긴다.
공개 tracker/relay grid에 붙이는 설치라면 relay 주소는 외부 peer가 dial 가능한 multiaddr이어야 한다.
예: `/dns4/rustory-relay.example.com/tcp/4001/p2p/<relay_peer_id>`.
`/ip4/100.64.0.0/10`, RFC1918 private IP, loopback 같은 literal relay 주소를 넘기면 installer와
`rr doctor`가 경고한다. 이런 주소는 같은 tailnet/LAN에 없는 신규 머신에서는 relay circuit을 열 수 없다.
`--token`에는 raw token 값만 전달한다. shell quoting은 명령줄 문법일 뿐이고,
token 값 자체에 앞뒤 `'` 또는 `"` 문자를 포함하면 tracker 인증이 실패한다.
기존 P2P grid에 합류시키는 설치라면 `--user-id`를 기존 grid 값으로 맞추고,
`--swarm-key-b64`로 기존 공유 `swarm.key`의 base64 값을 전달한다. 파일을 이미 배치한 운영 경로에서는
`--swarm-key-source <path>`도 계속 지원한다. `p2p_identity_key`는 디바이스별 값이므로 공유하지 않는다.
대상 머신에 다른 swarm key가 이미 있으면 installer는 기본적으로 실패하고, `--force`를 지정했을 때만 백업 후 교체한다.
`--install-hook`은 현재 shell을 자동 감지해 `~/.zshrc` 또는 `~/.bashrc`에 Rustory managed block을 추가/교체한다.
비표준 시작 파일을 쓰면 `--hook-shell bash|zsh --rc-file <path>`로 명시한다.
`--install-daemon`은 macOS에서는 `~/Library/LaunchAgents/com.rustory.daemon.plist`, Linux에서는
`~/.config/systemd/user/rustory.service`를 설치하고 기본적으로 즉시 시작한다. Linux에서
user systemd bus가 없으면 unit 파일은 보존하고 `manager=background` fallback으로 `rr daemon`을
즉시 시작하며, 같은 shell rc 파일에 background daemon autostart block을 설치해 컨테이너 재시작 뒤
첫 interactive shell에서 다시 띄운다. 설치만 하고 시작을 미루려면 `--no-start-daemon`을 함께
넘긴다. rc autostart가 싫으면 `--no-daemon-shell-autostart`를 함께 넘긴다.
`--import-hishtory`는 기본 Hishtory DB(`~/.hishtory/.hishtory.db`)가 있으면 import하고,
성공 후 user startup files에서 Hishtory hook/PATH/source 라인을 삭제한다.
Hishtory hook을 유지해야 하면 `--keep-hishtory-hooks`를 함께 넘긴다.
`rr p2p-sync --push`만 단발 실행하면 tracker에서 다른 peer는 발견할 수 있지만, 이 디바이스의
`p2p-serve` 등록은 유지되지 않는다.

installer는 다음 순서로 동작한다.

1. 현재 OS/arch에 맞는 release asset 이름을 결정한다.
2. asset과 SHA-256 checksum을 다운로드한다.
3. checksum을 검증한 뒤 `~/.local/bin/rr`에 설치한다.
4. `--swarm-key-b64` 또는 `--swarm-key-source`가 있으면 공유 swarm key를
   `~/.config/rustory/swarm.key`로 쓰고 fingerprint만 출력한다.
5. `--token`, `--tracker`, `--relay`, `--user-id`, `--device-id` 중 지정된 값이 있으면 `rr init`을 실행한다.
6. `--install-hook`이 있으면 shell rc 파일에 Rustory hook block을 설치한다.
7. `--import-hishtory`가 있으면 Hishtory DB를 import하고 Hishtory hook 라인을 제거한다.
8. `--install-daemon`이 있으면 user service를 설치하고, `--no-start-daemon`이 없으면 즉시 시작한다.
   Linux user systemd bus가 없으면 `~/.local/state/rustory/daemon.{pid,log}`를 쓰는 background
   fallback으로 시작하고, shell rc 파일에 idempotent autostart block을 설치한다.

token 값은 installer 로그에 출력하지 않는다.
swarm key 값도 installer 로그에 출력하지 않는다. true one-paste onboarding이 필요하면 private archive에
token과 `base64 <swarm.key>` 결과를 literal argument로 넣은 명령을 보관한다. public repo에는 실제 값을 기록하지 않는다.

## Init Alias

Rustory CLI binary 이름은 `rr`이지만, 프로젝트/패키지 이름은 Rustory다.
신규 peer는 아래 형태를 기본으로 쓴다.

```sh
rr init --token "$RUSTORY_TRACKER_TOKEN" --tracker "<tracker-url>"
```

기존 long option도 유지한다.

```sh
rr init --tracker-token "$RUSTORY_TRACKER_TOKEN" --trackers "<tracker-url>"
```

## Self Update

이미 설치된 머신은 현재 실행 중인 `rr` 경로를 기준으로 self-update한다.

```sh
rr update
rr update --version v1.0.10
rr update --dry-run
rr update --no-restart-daemon
```

기본값은 `zrma/rustory` GitHub Releases의 `latest` asset이다.
테스트 또는 사설 release mirror를 써야 하면 exact URL이나 base URL을 지정한다.

```sh
rr update --asset-base-url "https://example.invalid/rustory/releases/v1.0.10"
rr update --asset-url "https://example.invalid/rr-aarch64-apple-darwin" --sha256 "<sha256>"
```

`rr update`는 binary를 임시 파일로 다운로드하고 checksum을 검증한 뒤,
다운로드한 binary의 `rr version` 실행이 성공할 때만 설치 경로를 교체한다. 다운로드한
asset이 현재 설치된 파일과 byte-identical이면 파일 교체는 생략하지만, 기본적으로 관리 중인
daemon restart는 계속 시도한다. 이 때문에 이미 최신 버전이어도 `rr update`를 다시 실행하면
오래 떠 있던 daemon process/child를 현재 binary와 default로 재시작할 수 있다.

- macOS: `com.rustory.daemon` launchd user agent가 있으면 `launchctl kickstart -k`로 재시작한다.
- Linux systemd-user: `~/.config/systemd/user/rustory.service`가 있으면 `systemctl --user restart`를 시도한다.
- Linux container/background fallback: systemd user bus가 없거나 fallback pid가 있으면
  `~/.local/state/rustory/daemon.pid`의 기존 process를 종료하고 새 binary로 `rr daemon`을 다시 띄운다.

자동 재시작을 원하지 않으면 `--no-restart-daemon`을 사용한다. one-shot installer를
`--install-daemon`과 함께 다시 실행하는 경우에도 service 파일을 갱신하고 시작 경로를 다시 밟으므로,
systemd-user/launchd에서는 재시작되고 Linux systemd user bus가 없는 환경에서는 background fallback이
새 binary로 재시작된다.

## Linux user service caveat

one-shot installer의 `--install-daemon`은 Linux에서
`~/.config/systemd/user/rustory.service`를 만든 뒤 `systemctl --user`로 enable/start를
시도한다. SSH 비로그인 세션이나 container-like shell처럼 user systemd bus가 없으면
installer는 `daemon=start_deferred`를 출력하고 `manager=background` fallback으로
`rr daemon`을 시작한다. fallback 로그와 pid는 `~/.local/state/rustory/daemon.log`,
`~/.local/state/rustory/daemon.pid`에 둔다. 또한 같은 shell rc 파일에 managed autostart
block을 설치해 컨테이너 재시작 뒤 첫 interactive shell에서 죽은 daemon을 다시 띄운다.
장기 운영 서버에서는 같은 사용자 로그인 세션에서 다음을 실행해 systemd-user 관리로 전환한다.

```sh
systemctl --user daemon-reload
systemctl --user enable --now rustory.service
systemctl --user status rustory.service
```

로그아웃 뒤에도 계속 실행해야 하는 서버라면 운영 정책에 맞게
`loginctl enable-linger <user>`를 별도로 적용한다. shell autostart fallback은 interactive
shell이 열릴 때만 복구되므로, 컨테이너 entrypoint나 supervisor를 소유할 수 있는 환경에서는
그 경로에 `rr daemon`을 직접 붙이는 쪽이 더 강하다.
