# Shell Hook (bash/zsh)

## 설치(현재 세션)
### zsh
```sh
source <(rr hook --shell zsh)
```

### bash
```sh
source <(rr hook --shell bash)
```

## 환경 변수
- `RUSTORY_HOOK_DISABLE=1`: hook 동작 비활성화(기록/검색 모두)
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

## 동작 개요
- 기록: 커맨드 종료 시 `rr record`를 백그라운드로 호출해 SQLite에 append-only 저장
- 업로드(선택): `RUSTORY_ASYNC_UPLOAD=1`이면 `rr record`가 주기 제한(`RUSTORY_ASYNC_UPLOAD_INTERVAL_SEC`)을 적용해 백그라운드 push를 트리거한다.
- 보관(선택): `RUSTORY_AUTO_PRUNE=1`이면 `rr record`가 주기 제한(`RUSTORY_AUTO_PRUNE_INTERVAL_SEC`)을 적용해 오래된 로컬 엔트리를 정리한다.
- 검색: `ctrl+r`에서 `rr search`(fzf)로 선택한 커맨드를 현재 입력 버퍼에 삽입

### duration_ms(소요 시간)
- zsh: `EPOCHREALTIME` 기반으로 `duration_ms`를 기록한다.
- bash: 가능하면(`EPOCHREALTIME` 또는 `SECONDS`) best-effort로 `duration_ms`를 기록한다.
