# Quickstart

이 문서는 Rustory를 "최소 구성"으로 빠르게 써보는 흐름을 정리한다.

## 0) 준비
- ctrl+r 검색 UX는 `rr search` inline TUI가 담당한다. 별도 `fzf` 설치는 필요 없다.

### 공식 배포 설치
Release asset이 준비된 버전은 아래처럼 설치하고 바로 tracker grid에 참여시킬 수 있다.
tracker URL/token은 private 값이므로 public 문서에는 실제 값을 두지 않는다.

```sh
curl -fsSL https://raw.githubusercontent.com/zrma/rustory/main/install/rustory.py | \
  python3 - --token "$RUSTORY_TRACKER_TOKEN" --tracker "<tracker-url>" \
    --relay "<relay-multiaddr>" --user-id "<shared-user-id>" \
    --swarm-key-b64 "<base64-swarm-key>" \
    --install-hook --install-daemon --import-hishtory
```

`--install-hook`은 현재 shell에 맞춰 Rustory hook을 `~/.zshrc` 또는 `~/.bashrc`에 설치한다.
`--install-daemon`은 `rr daemon`을 user service로 등록해 이 머신을 tracker/relay grid의 상시 멤버로 유지한다.
`--import-hishtory`는 `~/.hishtory/.hishtory.db`를 가져온 뒤 user startup files의 Hishtory hook 라인을 제거한다.
Hishtory DB/디렉터리는 fallback source로 남긴다.
`--token`에는 raw token만 전달한다. token 값 앞뒤에 literal `'` 또는 `"`가 들어가면 `rr doctor`에서
`length`가 예상보다 길어지고 tracker ping이 401로 실패한다.
기존 P2P grid에 합류시키려면 같은 `user_id`, 같은 relay 주소, 같은 공유 `swarm.key`가 필요하다.
신규 머신에 파일을 미리 둘 필요가 없게 하려면 `swarm.key` 내용을 base64로 인코딩해 `--swarm-key-b64`에 넣는다.
이미 파일 배포 경로가 있는 운영 환경에서는 `--swarm-key-source <path>`도 사용할 수 있다.
relay 주소는 외부 신규 머신이 dial 가능한 public multiaddr이어야 하며, DNS 이름은
`/dns4/<host>/tcp/<port>/p2p/<relay_peer_id>` 형태로 쓸 수 있다.
`p2p_identity_key`는 디바이스별로 새로 생성되어야 하므로 복사하지 않는다.

설치 후 binary만 갱신하려면:

```sh
rr update
rr update --dry-run
```

배포/업데이트 상세는 `docs/distribution.md` 참고.

### 빌드(로컬 개발)
Rust 툴체인: `rust-toolchain.toml` 기준

```sh
cargo build --release
./target/release/rr --help
```

또는 설치(선택):
```sh
cargo install --path .
rr --help
```

## 1) 가장 빠른 e2e 확인(권장)
레포 루트에서:
```sh
scripts/smoke_p2p_local.sh
```

tracker + relay + 2 peer + sync(push 포함)까지 로컬에서 자동으로 띄워서 검증한다.

## 2) 실사용: tracker/relay + 디바이스 온보딩

### 2-1) (항상 온라인일 필요는 없지만) tracker/relay 실행

PoC 단계(권장):
- tracker/relay는 로컬/임시로 띄워도 된다. 내려가면 동기화가 지연될 뿐이고, 로컬 DB가 source of truth라 데이터 유실은 없다.

안정화 이후(권장):
- self-hosted k8s 같은 환경에 tracker/relay를 상시(보통 1 replica)로 실행한다.
- relay는 `relay_addr`에 peer id가 박히므로, PeerId가 바뀌지 않게 identity key(`~/.config/rustory/relay.key`)를 영속화(PV/Secret 마운트)하는 것을 권장한다.

#### Relay 서버
```sh
rr relay-serve --listen /ip4/0.0.0.0/tcp/4001
```

출력되는 `relay listen: .../p2p/<relay_peer_id>` 주소를 기록한다.
운영 중 resource limit이 반복되면 `rr relay-serve --help`의 capacity 플래그와 relay 시작 로그의
`relay config:` 값을 먼저 확인한다.

#### Tracker 서버
```sh
rr tracker-serve --bind 0.0.0.0:8850 --ttl-sec 60 --token "secret"
```

### 2-2) 각 디바이스에서 init
각 디바이스에서:
```sh
rr init \
  --user-id "<user>" \
  --device-id "<device>" \
  --tracker "http://<tracker-host>:8850" \
  --relay "/dns4/<relay-host>/tcp/4001/p2p/<relay_peer_id>" \
  --token "secret"

rr doctor
rr doctor --json
```

`config status: invalid: ...`가 표시되면 `~/.config/rustory/config.toml`을 먼저 고친다. 기존 설정을 버리고 새 템플릿으로 복구하려면 `rr init --force ...`를 사용한다. `--force`는 기존 config를 덮어쓰므로 필요한 값은 먼저 보관한다. `rr doctor`는 설정 파일이 잘못된 상태에서도 계속 실행되어 어떤 파일과 에러를 봐야 하는지 보여준다.
`db permissions`, `config permissions`, key 파일 권한, 기본 `record_ignore_regex` 누락처럼 안전하게 자동 보정 가능한 로컬 hygiene은 `rr doctor --auto-fix`로 정리할 수 있다. relay 주소 변경, token 변경, 손상된 config 수정처럼 운영 판단이 필요한 항목은 자동으로 바꾸지 않는다.

`rr doctor`의 `db status:`/`db permissions:` 라인에서 로컬 DB 파일 존재 여부, 저장된 entry 수, peer book/sync peer 수, 파일 권한 경고를 확인할 수 있다. `entries=0`이면 ctrl+r 검색 후보가 아직 없는 상태다.
`hook:` 라인에서는 현재 셸에서 hook 설치 마커가 보이는지(`installed`), `RUSTORY_HOOK_DISABLE`로 비활성화됐는지(`disabled`), ctrl+r 검색 limit 값이 어떤 resolver 경로로 해석됐는지 확인한다.

`rr init`가 현재 준비하는 생성물은 명령 출력과 관련 코드를 확인한다. 대표적으로 아래 아티팩트를 점검한다.
- `~/.config/rustory/config.toml` (설정 템플릿)
- `~/.config/rustory/swarm.key` (PSK, 같은 swarm 내 디바이스는 동일 파일 공유)
- `~/.config/rustory/identity.key` (PeerId, 디바이스별 고유)

### 2-2-1) (선택) 기존 히스토리 seed(import)
기존 셸 히스토리 파일을 DB로 가져오려면:

```sh
# zsh
rr import --shell zsh

# bash
rr import --shell bash

# Hishtory local SQLite DB
rr import --shell hishtory

# Atuin local SQLite DB
rr import --shell atuin
```

필요하면:
- 다른 파일을 지정: `rr import --shell zsh --path /path/to/file`
- 마지막 N개만: `rr import --shell zsh --limit 100000`

import는 `RUSTORY_RECORD_IGNORE_REGEX` / `record_ignore_regex`를 존중한다.
Hishtory 또는 Atuin에서 Rustory P2P grid로 점진 이관하려면 각각 `docs/hishtory-migration.md`, `docs/atuin-migration.md`를 먼저 확인한다.

### 2-3) P2P 멤버 실행(추천: daemon)
각 디바이스가 클러스터 멤버로 보이려면 serve 등록과 watch sync가 같이 돌아야 한다.
실사용 기본 경로는 두 하위 프로세스를 supervision하는 `rr daemon`이다.

```sh
rr daemon
```

분리 운영이 필요할 때만 아래처럼 `p2p-serve`와 `p2p-sync --watch --push`를 별도 프로세스로 관리한다.

```sh
rr p2p-serve
```

```sh
rr p2p-sync --watch --interval-sec 60 --start-jitter-sec 10 --push
```

백그라운드 실행 예시는 `docs/daemon.md` 참고.

### 2-4) hook 활성화(현재 셸 세션)
```sh
source <(rr hook --shell zsh)
```

bash/zsh 훅 상세는 `docs/hook.md` 참고.

### 2-5) (선택) 민감 커맨드 기록 제외
원문 첫 문자가 공백인 command는 bash/zsh hook이 기록하지 않는다. 일회성 민감 command에는 앞 공백 opt-out을 사용할 수 있지만, shell 자체 history와 audit/logging 정책은 별도이므로 secret manager를 우선한다.

예:
```sh
export RUSTORY_RECORD_IGNORE_REGEX='(?i)(password|token|secret|authorization:|bearer )'
```

이 옵션은 hook이 호출하는 `rr record`에도 적용된다. 상세는 `docs/hook.md` 참고.

### 2-5-1) (선택) 기록 직후 비동기 업로드 트리거
hook 기반 기록 직후 업로드를 자동으로 트리거하려면 아래처럼 설정한다. 숫자 값은 예시이며, 현재 default와 해석 순서는 `rr doctor`, `docs/hook.md`, 관련 코드를 확인한다.
```sh
export RUSTORY_ASYNC_UPLOAD=1
export RUSTORY_ASYNC_UPLOAD_INTERVAL_SEC=15
export RUSTORY_ASYNC_UPLOAD_LIMIT=200
```

지속 설정으로 남기려면 `~/.config/rustory/config.toml`에 같은 형태의 값을 둘 수 있다. 같은 이름의 `RUSTORY_*` 환경 변수가 있으면 환경 변수가 우선한다.

```toml
async_upload = true
async_upload_interval_sec = 15
async_upload_limit = 200
```

업로드 실패 시에도 로컬 기록은 유지되며, 다음 트리거에서 `pending_push` 큐가 다시 전송된다.
설정 해석/주기 상태는 `rr doctor`의 `async upload` 라인에서 즉시 확인할 수 있다.

### 2-5-2) (선택) 기록 시 자동 보관(prune) 스케줄링
오래된 로컬 엔트리를 주기적으로 자동 정리하려면 아래처럼 설정한다. 보존 일수/간격/개수는 예시이며, 현재 default와 해석 순서는 `rr doctor`, `docs/hook.md`, 관련 코드를 확인한다.
```sh
export RUSTORY_AUTO_PRUNE=1
export RUSTORY_AUTO_PRUNE_DAYS=180
export RUSTORY_AUTO_PRUNE_INTERVAL_SEC=86400
export RUSTORY_AUTO_PRUNE_KEEP_RECENT=5000
```

지속 설정으로 남기려면 `~/.config/rustory/config.toml`에 같은 형태의 값을 둘 수 있다. 같은 이름의 `RUSTORY_*` 환경 변수가 있으면 환경 변수가 우선한다.

```toml
auto_prune = true
auto_prune_days = 180
auto_prune_interval_sec = 86400
auto_prune_keep_recent = 5000
```

`rr record` 성공 후 주기 제한에 맞춰 자동 prune이 실행되며, `RUSTORY_AUTO_PRUNE_KEEP_RECENT`를 지정하면 최신 N개는 삭제 대상에서 제외된다. 자동 보관 실패 시에도 기록 자체는 유지된다.
설정 해석/주기 상태는 `rr doctor`의 `auto prune` 라인에서 즉시 확인할 수 있다.

### 2-6) (선택) 오래된 로컬 히스토리 수동 정리
먼저 영향 범위를 확인한다.
```sh
rr prune --older-than-days 180 --keep-recent 5000 --dry-run
```

결과가 의도와 같으면 실제 삭제를 수행한다.
```sh
rr prune --older-than-days 180 --keep-recent 5000
```

### 2-7) (선택) 동일 명령 중복 로컬 정리
같은 UTC 날짜, 같은 `device_id`, hostname, CWD, command, exit code가 반복된 row를 먼저 확인한다. 기본 scope는 현재 device id이며, 실제 삭제는 하지 않는다.

```sh
rr dedupe
```

결과가 의도와 같으면 명시 적용한다. 기본 보존 대상은 최신 row이며, 필요하면 oldest를 남길 수 있다.

```sh
rr dedupe --apply
rr dedupe --keep oldest --older-than-days 14 --apply --vacuum
```

`rr dedupe --apply`는 삭제 tombstone을 남겨 peer로 전파된다. 아직 peer로 push되지 않은 row는 peer push cursor 기준으로 삭제 후보에서 제외되며, tombstone 자체도 각 peer의 delete cursor가 따라올 때까지 `rr sync-status --json`의 deletion pending으로 보일 수 있다.

### 2-8) (선택) 전파 완료된 삭제 tombstone 정리
삭제 tombstone은 이미 삭제한 row가 오래된 peer나 재import 경로에서 다시 살아나는 것을 막는다. 따라서 바로 지우지 말고, 먼저 오래된 tombstone 후보만 확인한다.

```sh
rr tombstone-gc --older-than-days 90
```

결과가 의도와 같으면 명시 적용한다. SQLite 파일 크기까지 줄여야 하면 `--vacuum`을 같이 쓴다.

```sh
rr tombstone-gc --older-than-days 90 --apply
rr tombstone-gc --older-than-days 90 --apply --vacuum
```

알려진 peer가 하나라도 있으면 Rustory는 각 peer의 delete push cursor가 해당 tombstone의 `delete_seq`를 따라온 것만 GC한다. 알려진 peer가 전혀 없는 단일 머신 DB에서는 age 기준만 적용된다. 몇 주 이상 꺼져 있던 새/구 peer는 GC된 tombstone을 받을 수 없으므로, 운영 grid에서는 `rr sync-status --json --with-tracker`의 deletion pending이 0으로 안정된 뒤 긴 보존 기간을 두고 실행한다.

자동 정리가 필요하면 기본 off 상태에서 명시적으로 켠다.

```toml
auto_tombstone_gc = true
auto_tombstone_gc_days = 90
auto_tombstone_gc_interval_sec = 86400
auto_tombstone_gc_marker_path = "~/.config/rustory/auto-tombstone-gc.last"
```

설정 해석/주기 상태는 `rr doctor`의 `auto tombstone gc` 라인에서 확인한다.

### 2-9) (선택) 민감 로컬 엔트리 삭제
`record_ignore_regex`는 기록/import 전 차단이 목적이다. 이미 들어간 로컬 row는 먼저 dry-run으로 확인한다.

```sh
rr delete --cmd-regex '(?i)(password|token|secret|authorization:|bearer )' --dry-run
```

결과가 의도와 같으면 명시 확인을 붙여 삭제한다. SQLite 파일/WAL에 남은 삭제 흔적까지 줄여야 하는 경우 `--vacuum`을 같이 쓴다.

```sh
rr delete --cmd-regex '(?i)(password|token|secret|authorization:|bearer )' --yes --vacuum
rr delete --entry-id '<entry_id>' --yes --vacuum
```

`rr delete`는 삭제 tombstone을 남겨 peer로 전파된다. 민감 row를 이미 여러 peer로 sync한 뒤 삭제했다면 `rr sync-status --json --with-tracker`에서 deletion pending이 0까지 줄어드는지 확인한다.

### 2-10) (선택) 노드 이탈과 재가입
현재 머신을 Rustory grid에서 빼려면 대상 머신에서 직접 uninstall을 실행한다.

```sh
rr uninstall --yes
```

기본 uninstall은 관리 중인 daemon을 멈추고, shell hook과 daemon autostart/service를 제거하며,
tracker에 현재 PeerId unregister를 시도한다. 또한 로컬 DB/config/state를 제거한다. binary까지 지우려면
명시적으로 `--remove-binary`를 붙인다.

apply 전에 `--dry-run`으로 DB, config/key, runtime marker, state/log, custom rc 경로를 확인한다.
config를 읽을 수 없거나 관리 daemon 정지가 불완전하면 로컬 데이터 삭제 전에 중단한다. DB/config
디렉터리의 알 수 없는 sibling 파일은 재귀 삭제하지 않고 남겨 사용자 파일을 보존한다.

```sh
rr uninstall --yes --remove-binary
```

로컬 DB나 config/key를 보존해야 하는 점검 상황에서는 keep flag를 먼저 dry-run으로 확인한다.

```sh
rr uninstall --dry-run --keep-db --keep-config
rr uninstall --yes --keep-db --keep-config
```

installer에서 `--rc-file <path>`로 비표준 shell 시작 파일을 지정했다면 uninstall에도 같은 경로를
전달해 그 파일의 Rustory hook/daemon autostart 관리 블록까지 제거한다.

```sh
rr uninstall --dry-run --rc-file "~/.config/custom-shell.rc"
rr uninstall --yes --rc-file "~/.config/custom-shell.rc"
```

uninstall은 현재 머신의 membership 정리일 뿐이다. 이미 다른 peer가 받은 history row를 되돌리거나,
다른 머신 DB에 남은 과거 hostname/device metadata를 삭제하지 않는다. 따라서 같은 hostname의 과거
history row가 검색되는 것은 정상이다.

재가입은 새 설치와 같은 절차를 따른다. `identity.key`를 보존하지 않고 다시 설치하면 새 PeerId로
grid에 들어오며, historical row의 과거 `device_id`/hostname과 새 peer identity가 함께 보일 수 있다.
이 자체는 데이터 손상이 아니다. 다만 `rr sync-status --json --with-tracker`나 watch UI에서 같은
hostname 또는 같은 `device_id`가 동시에 active duplicate warning으로 나오면 이전 daemon이 아직
살아 있거나 같은 config/key를 다른 환경에 복제한 상태일 수 있으므로 대상 노드의 process/config를
먼저 정리한다.

원격 노드 강제 uninstall은 현재 지원하지 않는다. 인증된 원격 제어면과 대상 사용자 확인 없이 다른
머신의 hook/DB/config를 지우는 기능은 grid 안정성과 보안 경계를 동시에 깨뜨릴 수 있으므로, 운영
절차는 대상 노드에서 직접 `rr uninstall --yes`를 실행하는 방식으로 제한한다.

## 다음 문서
- 배포/installer/self-update: `docs/distribution.md`
- P2P 상세/트러블슈팅: `docs/p2p.md`
- 데몬/스케줄러: `docs/daemon.md`
- 훅: `docs/hook.md`
