# Shell Hook (bash/zsh)

## 설치(현재 세션)
ctrl+r 검색은 `rr search`의 inline TUI를 사용한다. 별도 `fzf` 설치는 필요 없다.
현재 셸에 hook이 적용됐는지는 `rr doctor`의 `hook:` 라인에서 `installed=true`로 확인한다.
one-shot installer의 `--install-hook`은 `# >>> rustory hook >>>` 관리 블록을 1개만 유지한다. 이전 릴리즈의 `# >>> rustory >>>` 블록이나 중복 관리 블록이 남아 있으면 installer, `rr doctor --auto-fix`, `rr update`가 현재 셸 rc 파일의 Rustory 관리 블록을 정규화한다. Rustory marker 밖의 사용자 설정은 수정하지 않는다.

### zsh
```sh
source <(rr hook --shell zsh)
```

### bash
```sh
source <(rr hook --shell bash)
```

## 환경 변수
hook runtime 설정은 `~/.config/rustory/config.toml`에도 둘 수 있다. 같은 설정이 환경 변수와 config에 모두 있으면 환경 변수가 우선한다.
정확한 현재 default와 resolver 순서는 `rr doctor`, `rr hook --help`, `rr init`가 생성하는 config template, 관련 코드가 소유한다.

- `RUSTORY_HOOK_DISABLE=1`: hook 동작 비활성화(기록/검색 모두)
  - `1/true/yes/on`은 비활성화로 해석한다.
  - unset 또는 `0/false/no/off`는 비활성화하지 않는다.
  - 알 수 없는 값은 안전을 위해 비활성화로 취급하며, `rr doctor`의 `hook:` 라인에 경고가 표시된다.
- `RUSTORY_HOOK_INSTALLED=1`: `rr hook`이 현재 셸에 export하는 설치 마커다. 직접 설정할 필요는 없고, `rr doctor`가 hook 적용 여부를 판단할 때 사용한다.
- `RUSTORY_DB_PATH=/path/to/db.sqlite`: 기본 DB 경로 오버라이드(`rr --db-path ...` 대신 사용 가능)
- `RUSTORY_SEARCH_LIMIT=<n>`: ctrl+r 검색 limit을 오버라이드한다.
  - unset이면 `config.toml`의 `search_limit_default`와 애플리케이션 default 순서로 해석된다.
- `RUSTORY_RECORD_IGNORE_REGEX="<regex>"`: 정규식에 매칭되는 커맨드는 기록하지 않는다.
  - 예: `RUSTORY_RECORD_IGNORE_REGEX='(?i)(password|token|secret|authorization:|bearer )'`
  - env가 있으면 config.toml의 `record_ignore_regex`보다 우선한다.
- 원문 첫 문자가 공백인 command는 shell history 설정과 무관하게 privacy opt-out으로 보고 기록하지 않는다. 이미 열린 shell은 새 `rr` 설치 후 managed hook을 다시 source하거나 shell을 다시 열어야 새 동작을 사용한다.
  - 정규식이 잘못된 경우는 안전을 위해 기록을 스킵한다(`rr doctor`에서 상태 확인).
  - `rr doctor`, `rr sync-status`, `rr version` 같은 Rustory 운영 명령도 일반 명령처럼 기록된다. 특정 `rr ...` 명령을 제외하려면 이 regex에 명시적으로 추가한다.
- `RUSTORY_ASYNC_UPLOAD=1`: `rr record` 성공 후 백그라운드 `rr p2p-sync --push` 트리거를 활성화한다.
- `RUSTORY_ASYNC_UPLOAD_INTERVAL_SEC=<sec>`: 비동기 업로드 트리거 최소 간격(초).
- `RUSTORY_ASYNC_UPLOAD_LIMIT=<n>`: 비동기 업로드 1회 실행 시 push 배치 크기(`--limit`).
- `RUSTORY_AUTO_PRUNE=1`: `rr record` 성공 후 주기적으로 자동 보관(prune) 실행을 활성화한다.
- `RUSTORY_AUTO_PRUNE_DAYS=<days>`: 자동 보관 기준 일수(`rr prune --older-than-days`에 대응).
- `RUSTORY_AUTO_PRUNE_INTERVAL_SEC=<sec>`: 자동 보관 실행 최소 간격(초).
- `RUSTORY_AUTO_PRUNE_KEEP_RECENT=<n>`: 자동 보관 시 최신 N개를 항상 보존한다(`rr prune --keep-recent`에 대응).
- `RUSTORY_AUTO_PRUNE_MARKER_PATH=/path/to/marker`: 자동 보관 주기 marker 파일 경로를 오버라이드한다.

## config.toml 지속 설정
`rr init`가 생성하는 `config.toml` 템플릿은 코드가 소유한다. 아래는 설정 형태 예시이며, 현재 템플릿과 default는 `rr init` 출력과 관련 코드를 확인한다.

```toml
search_limit_default = 100000

async_upload = true
async_upload_interval_sec = 15
async_upload_limit = 200
async_upload_marker_path = "~/.config/rustory/async-upload.last"

auto_prune = true
auto_prune_days = 180
auto_prune_interval_sec = 86400
auto_prune_keep_recent = 5000
auto_prune_marker_path = "~/.config/rustory/auto-prune.last"

auto_tombstone_gc = true
auto_tombstone_gc_days = 90
auto_tombstone_gc_interval_sec = 86400
auto_tombstone_gc_marker_path = "~/.config/rustory/auto-tombstone-gc.last"
```

`RUSTORY_SEARCH_LIMIT`, `RUSTORY_ASYNC_UPLOAD*`, `RUSTORY_AUTO_PRUNE*`, `RUSTORY_AUTO_TOMBSTONE_GC*` 환경 변수는 config 값보다 우선한다. 임시로 끄고 싶으면 예를 들어 `RUSTORY_ASYNC_UPLOAD=0`처럼 환경 변수로 override한다.

## 동작 개요
- 기록: 커맨드 종료 시 `rr record`를 백그라운드로 호출해 SQLite에 append-only 저장
- 업로드(선택): `RUSTORY_ASYNC_UPLOAD=1`이면 `rr record`가 주기 제한(`RUSTORY_ASYNC_UPLOAD_INTERVAL_SEC`)을 적용해 백그라운드 push를 트리거한다.
- 보관(선택): `RUSTORY_AUTO_PRUNE=1`이면 `rr record`가 주기 제한(`RUSTORY_AUTO_PRUNE_INTERVAL_SEC`)을 적용해 오래된 로컬 엔트리를 정리하고, 필요 시 최신 N개(`RUSTORY_AUTO_PRUNE_KEEP_RECENT`)를 보존한다.
- tombstone GC(선택): `RUSTORY_AUTO_TOMBSTONE_GC=1`이면 `rr record`가 주기 제한(`RUSTORY_AUTO_TOMBSTONE_GC_INTERVAL_SEC`)을 적용해 오래되고 알려진 peer로 delete cursor가 전파된 삭제 tombstone만 정리한다.
- 검색: `ctrl+r`에서 `rr search` inline TUI로 선택한 커맨드를 현재 입력 버퍼에 삽입한다. 검색 UI는 alt-screen/fullscreen을 쓰지 않고 현재 화면 아래에 compact table을 그리며, hostname, CWD, timestamp, runtime, exit code, command를 표시한다.
  - 기본 사용법은 기억나는 command, subcommand, flag, 경로 일부를 공백으로 구분해 그대로 입력하는 것이다. 별도 검색식을 배울 필요가 없다.
  - 일반 토큰은 command match를 가장 우선하고, exact phrase, 단어 전체, 단어 prefix, substring, 한 글자 오타, compact fuzzy 순으로 ranking한다. 동률에 가까운 결과에서는 현재 CWD와 hostname을 보조 신호로 사용하고 최신 기록을 우선한다.
  - 토큰은 command/CWD/hostname/device/user에 흩어져 있어도 AND 조건으로 후보를 좁힐 수 있다. 기존 `cwd:/tmp`, `hostname:node0`, `command:"docker run"`, `exit_code:127`, `-token` 문법은 고급 호환 기능으로 유지한다.
  - command가 길어 화면에서 잘리면 `shift+left` / `shift+right`로 table viewport를 좌우 이동한다. 선택 row는 `ctrl+k`로 로컬 DB에서 삭제할 수 있으며, 이 경우 셸 입력 버퍼에는 아무 텍스트도 삽입하지 않는다. limit 해석 순서는 `rr doctor`와 관련 resolver 코드를 확인한다.

### duration_ms(소요 시간)
- zsh: `EPOCHREALTIME` 기반으로 `duration_ms`를 기록한다.
- bash: 가능하면(`EPOCHREALTIME` 또는 `SECONDS`) best-effort로 `duration_ms`를 기록한다.
