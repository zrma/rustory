# Distribution

- Audience: Rustory 운영자, release 관리자
- Owner: Rustory
- Last Verified: 2026-06-29

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

## One-Line Install

공식 배포 URL이 GitHub raw installer를 가리키면 신규 머신은 아래처럼 시작한다.

```sh
curl -fsSL https://raw.githubusercontent.com/zrma/rustory/main/install/rustory.py | \
  python3 - --token "$RUSTORY_TRACKER_TOKEN" --tracker "<tracker-url>" \
    --install-hook --import-hishtory
```

`--tracker`는 반복 지정하거나 comma-separated 값으로 지정할 수 있다.
relay 주소까지 config에 쓰려면 `--relay "<relay-multiaddr>"`를 같이 넘긴다.
`--install-hook`은 현재 shell을 자동 감지해 `~/.zshrc` 또는 `~/.bashrc`에 Rustory managed block을 추가/교체한다.
비표준 시작 파일을 쓰면 `--hook-shell bash|zsh --rc-file <path>`로 명시한다.
`--import-hishtory`는 기본 Hishtory DB(`~/.hishtory/.hishtory.db`)가 있으면 import하고,
성공 후 user startup files에서 Hishtory hook/PATH/source 라인을 삭제한다.
Hishtory hook을 유지해야 하면 `--keep-hishtory-hooks`를 함께 넘긴다.

installer는 다음 순서로 동작한다.

1. 현재 OS/arch에 맞는 release asset 이름을 결정한다.
2. asset과 SHA-256 checksum을 다운로드한다.
3. checksum을 검증한 뒤 `~/.local/bin/rr`에 설치한다.
4. `--token`, `--tracker`, `--relay`, `--user-id`, `--device-id` 중 지정된 값이 있으면 `rr init`을 실행한다.
5. `--install-hook`이 있으면 shell rc 파일에 Rustory hook block을 설치한다.
6. `--import-hishtory`가 있으면 Hishtory DB를 import하고 Hishtory hook 라인을 제거한다.

token 값은 installer 로그에 출력하지 않는다.

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
rr update --version v1.0.4
rr update --dry-run
```

기본값은 `zrma/rustory` GitHub Releases의 `latest` asset이다.
테스트 또는 사설 release mirror를 써야 하면 exact URL이나 base URL을 지정한다.

```sh
rr update --asset-base-url "https://example.invalid/rustory/releases/v1.0.4"
rr update --asset-url "https://example.invalid/rr-aarch64-apple-darwin" --sha256 "<sha256>"
```

`rr update`는 binary를 임시 파일로 다운로드하고 checksum을 검증한 뒤,
다운로드한 binary의 `rr version` 실행이 성공할 때만 설치 경로를 교체한다.
