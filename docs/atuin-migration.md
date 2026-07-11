# Atuin Migration Runbook

- Audience: Atuin 로컬 히스토리를 Rustory로 점진 이관하는 사용자와 유지보수자
- Owner: Rustory
- Last Verified: 2026-07-11

Rustory는 Atuin server나 sync protocol에 연결하지 않는다. `rr import --shell atuin`은 각 머신의 로컬 materialized SQLite DB를 read-only로 읽어 Rustory 로컬 DB에 추가한다.

## 경계

- 기본 입력 경로는 Atuin과 같이 `${XDG_DATA_HOME:-~/.local/share}/atuin/history.db`이며 `--path`로 바꿀 수 있다. Atuin config에서 별도 `data_dir`를 지정했다면 `--path`를 사용한다.
- import는 additive/idempotent다. Atuin `id`를 우선 source key로 사용하고, 비어 있으면 원본 row 필드의 deterministic composite key를 사용한다.
- `deleted_at`이 있는 schema에서는 삭제되지 않은 row만 가져온다. Atuin deletion을 Rustory tombstone으로 변환하지 않는다.
- 현재 Rustory 모델에 대응 필드가 없는 `author`와 `intent`는 가져오지 않는다.
- TEXT/BLOB에 invalid UTF-8이 있으면 replacement 문자로 변환해 해당 row import를 계속한다.
- Atuin의 encrypted `records.db`, server API, sync-v2 record replay는 지원하지 않는다.

## 사전 점검

민감한 명령을 제외하려면 import 전에 `record_ignore_regex` 또는 `RUSTORY_RECORD_IGNORE_REGEX`를 설정한다. 운영 DB에 바로 쓰기 전에는 별도 Rustory DB로 smoke를 실행한다.

```bash
rr --db-path /tmp/rustory-atuin-smoke.db import --shell atuin --limit 1000
rr --db-path /tmp/rustory-atuin-smoke.db search --limit 20
```

출력의 `received`, `inserted`, `ignored`, `skipped`를 확인한다. 같은 명령을 다시 실행했을 때 기존 row가 `ignored`로 수렴해야 한다.

## 실제 import

```bash
rr import --shell atuin

# 다른 DB 또는 최신 N개만 가져오기
rr import --shell atuin --path /path/to/history.db --limit 100000
```

Atuin row의 원래 `hostname`, command time, duration, exit code, CWD는 보존한다. Rustory `device_id`는 import를 실행한 현재 device로 기록하므로 이후 `rr p2p-sync --push` 대상이 된다.

여러 머신이 같은 Atuin 계정의 materialized history를 가지고 있다면 한 머신씩 import와 push를 진행하고, 다음 머신에서 중복 row가 `ignored`로 처리되는지 확인한다.

## 선택형 adapter 계약

flat-file `bash`/`zsh` importer는 핵심 기능이고, SQLite importer는 독립 Cargo feature다.

- 기본 build: `import-atuin`, `import-hishtory` 모두 활성
- Atuin 제외: `cargo build --no-default-features --features import-hishtory`
- Hishtory 제외: `cargo build --no-default-features --features import-atuin`
- SQLite adapter 전부 제외: `cargo build --no-default-features`

feature가 빠진 source 이름은 `rr import` dispatch에 등록되지 않는다. adapter별 schema/query/idempotency 코드는 `src/history_import/atuin.rs`와 `src/history_import/hishtory.rs`가 각각 소유하며, CLI는 공통 `import_path_into_store` 계약만 호출한다.

## 검증

```bash
cargo test atuin_import --workspace
cargo test history_import --workspace
cargo test --no-default-features --workspace
cargo check --no-default-features --features import-atuin
cargo check --no-default-features --features import-hishtory
```
