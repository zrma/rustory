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

## 동작 개요
- 기록: 커맨드 종료 시 `rr record`를 백그라운드로 호출해 SQLite에 append-only 저장
- 검색: `ctrl+r`에서 `rr search`(fzf)로 선택한 커맨드를 현재 입력 버퍼에 삽입

### duration_ms(소요 시간)
- zsh: `EPOCHREALTIME` 기반으로 `duration_ms`를 기록한다.
- bash: 가능하면(`EPOCHREALTIME` 또는 `SECONDS`) best-effort로 `duration_ms`를 기록한다.
