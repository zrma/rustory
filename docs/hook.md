# Shell Hook (bash/zsh)

## 설치(현재 세션)
ctrl+r 검색은 외부 실행 파일 `fzf`를 사용한다. 설치/PATH 상태는 `rr doctor`의 `fzf:` 라인에서 확인한다.
현재 셸에 hook이 적용됐는지는 `rr doctor`의 `hook:` 라인에서 `installed=true`로 확인한다.

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

- `RUSTORY_HOOK_DISABLE=1`: hook 동작 비활성화(기록/검색 모두)
- `RUSTORY_HOOK_INSTALLED=1`: `rr hook`이 현재 셸에 export하는 설치 마커다. 직접 설정할 필요는 없고, `rr doctor`가 hook 적용 여부를 판단할 때 사용한다.
- `RUSTORY_DB_PATH=/path/to/db.sqlite`: 기본 DB 경로 오버라이드(`rr --db-path ...` 대신 사용 가능)
- `RUSTORY_SEARCH_LIMIT=100000`: ctrl+r 검색 시 `rr search --limit` 기본값 오버라이드
- `RUSTORY_RECORD_IGNORE_REGEX="<regex>"`: 정규식에 매칭되는 커맨드는 기록하지 않는다.
  - 예: `RUSTORY_RECORD_IGNORE_REGEX='(?i)(password|token|secret|authorization:|bearer )'`
  - env가 있으면 config.toml의 `record_ignore_regex`보다 우선한다.
  - 정규식이 잘못된 경우는 안전을 위해 기록을 스킵한다(`rr doctor`에서 상태 확인).
- `RUSTORY_ASYNC_UPLOAD=1`: `rr record` 성공 후 백그라운드 `rr p2p-sync --push` 트리거를 활성화한다.
- `RUSTORY_ASYNC_UPLOAD_INTERVAL_SEC=15`: 비동기 업로드 트리거 최소 간격(초). 기본값은 `15`.
- `RUSTORY_ASYNC_UPLOAD_LIMIT=200`: 비동기 업로드 1회 실행 시 push 배치 크기(`--limit`). 기본값은 `200`.
- `RUSTORY_AUTO_PRUNE=1`: `rr record` 성공 후 주기적으로 자동 보관(prune) 실행을 활성화한다.
- `RUSTORY_AUTO_PRUNE_DAYS=180`: 자동 보관 기준 일수(`rr prune --older-than-days`에 대응). 기본값은 `180`.
- `RUSTORY_AUTO_PRUNE_INTERVAL_SEC=86400`: 자동 보관 실행 최소 간격(초). 기본값은 `86400`(1일).
- `RUSTORY_AUTO_PRUNE_KEEP_RECENT=0`: 자동 보관 시 최신 N개를 항상 보존한다(`rr prune --keep-recent`에 대응). 기본값은 `0`(비활성).
- `RUSTORY_AUTO_PRUNE_MARKER_PATH=/path/to/marker`: 자동 보관 주기 marker 파일 경로를 오버라이드한다. 기본값은 `~/.config/rustory/auto-prune.last`.

## config.toml 지속 설정
`rr init`가 생성하는 `config.toml` 템플릿에는 아래 hook runtime 옵션이 주석으로 포함된다. 필요할 때 주석을 해제해 셸별 export 없이 지속 설정으로 사용할 수 있다.

```toml
async_upload = true
async_upload_interval_sec = 15
async_upload_limit = 200
async_upload_marker_path = "~/.config/rustory/async-upload.last"

auto_prune = true
auto_prune_days = 180
auto_prune_interval_sec = 86400
auto_prune_keep_recent = 5000
auto_prune_marker_path = "~/.config/rustory/auto-prune.last"
```

`RUSTORY_ASYNC_UPLOAD*` / `RUSTORY_AUTO_PRUNE*` 환경 변수는 config 값보다 우선한다. 임시로 끄고 싶으면 예를 들어 `RUSTORY_ASYNC_UPLOAD=0`처럼 환경 변수로 override한다.

## 동작 개요
- 기록: 커맨드 종료 시 `rr record`를 백그라운드로 호출해 SQLite에 append-only 저장
- 업로드(선택): `RUSTORY_ASYNC_UPLOAD=1`이면 `rr record`가 주기 제한(`RUSTORY_ASYNC_UPLOAD_INTERVAL_SEC`)을 적용해 백그라운드 push를 트리거한다.
- 보관(선택): `RUSTORY_AUTO_PRUNE=1`이면 `rr record`가 주기 제한(`RUSTORY_AUTO_PRUNE_INTERVAL_SEC`)을 적용해 오래된 로컬 엔트리를 정리하고, 필요 시 최신 N개(`RUSTORY_AUTO_PRUNE_KEEP_RECENT`)를 보존한다.
- 검색: `ctrl+r`에서 `rr search`(fzf)로 선택한 커맨드를 현재 입력 버퍼에 삽입

### duration_ms(소요 시간)
- zsh: `EPOCHREALTIME` 기반으로 `duration_ms`를 기록한다.
- bash: 가능하면(`EPOCHREALTIME` 또는 `SECONDS`) best-effort로 `duration_ms`를 기록한다.
