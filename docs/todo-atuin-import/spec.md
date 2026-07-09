# Spec: atuin-import

## 배경

- 요청 맥락: Hishtory import와 같은 방식으로 Atuin 사용자의 기존 로컬 히스토리를 Rustory 로컬 DB로 seed할 수 있게 한다. 대상은 Atuin server나 sync protocol이 아니라 각 머신의 로컬 materialized SQLite DB이다.
- 현재 문제/기회: Atuin은 서버 동기화형 shell history 도구 중 현역 대표주자이고, 로컬 `history.db`에는 sync 결과가 materialized 되어 있다. Rustory가 이 DB를 read-only/idempotent로 import하면 Atuin에서 Rustory P2P grid로 점진 이관할 수 있다.
- 확인한 source-of-truth: Atuin 기본 DB 경로는 `../atuin/crates/atuin-client/src/settings.rs`의 `history.db` default이며, `history` 테이블 base schema는 `../atuin/crates/atuin-client/migrations/20210422143411_create_history.sql`, `deleted_at` 추가는 `20230319185725_deleted_at.sql`, `author`/`intent` 추가는 `20260224000100_history_author_intent.sql`에서 확인한다. 현재 Atuin 저장 경로는 `../atuin/crates/atuin-client/src/database.rs`가 `timestamp`/`deleted_at`를 nanoseconds로 저장하고 `deleted_at is null`만 기본 list 대상으로 삼는다.

## 계획 스냅샷

- 목표: `rr import --shell atuin`이 Atuin 로컬 `history.db`를 read-only로 읽어 Rustory 로컬 DB에 idempotent하게 추가한다.
- 범위: `src/history_import.rs`, `src/cli.rs`, import 회귀 테스트, Atuin migration 문서/인덱스/manifest 포인터. Atuin server API, encrypted `records.db`, Atuin sync-v2 record replay, Rustory grid deletion/tombstone 생성은 제외한다.
- 검증 명령: `cargo test atuin_import --workspace`, `cargo test import_accepts_atuin_source --workspace`, `cargo test history_import --workspace`, `scripts/run-manifest-checks.sh --mode quick --work-id atuin-import`, 구현 완료 전 `scripts/check.sh --fast`.
- 완료 기준: local Atuin SQLite fixture와 실제 default path contract가 모두 통과하고, 재실행 시 기존 row가 `ignored`로 집계되며, deleted Atuin rows는 import되지 않고, Atuin import 문서가 Hishtory import와 같은 로컬-only 경계를 명확히 설명한다.

## 결정 사항

- 기본 경로는 `~/.local/share/atuin/history.db`로 둔다. 사용자가 `--path`를 지정하면 해당 SQLite 파일을 사용한다.
- importer는 `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX`로 열고, Hishtory importer와 같은 busy timeout/batch insert 패턴을 따른다.
- 필수 컬럼은 `id`, `timestamp`, `duration`, `exit`, `command`, `cwd`, `session`, `hostname`이다. optional 컬럼 `deleted_at`, `author`, `intent`는 `PRAGMA table_info(history)`로 존재 여부를 확인한다.
- `deleted_at` 컬럼이 있으면 `deleted_at is null` row만 import한다. Atuin deletion을 Rustory tombstone으로 변환하지 않는다.
- Rustory entry id는 Atuin `id`를 우선 source key로 삼는 deterministic UUID v5로 만든다. `id`가 비어 있거나 깨진 fixture를 위해 timestamp/duration/exit/command/cwd/session/hostname composite fallback을 둔다.
- `timestamp`는 Atuin nanoseconds 값을 `OffsetDateTime::from_unix_timestamp_nanos`로 변환한다. `duration`은 nanoseconds로 보고 Rustory `duration_ms`로 변환한다.
- Atuin `hostname`은 entry의 원래 host context로 보존하되, Rustory `device_id`는 import를 실행한 현재 Rustory device id를 사용한다. 이렇게 해야 import된 row가 현재 device의 outbound sync 대상으로 push 가능하다.
- invalid UTF-8 TEXT는 Hishtory importer와 같은 lossy conversion 원칙을 따른다. 한 row의 encoding 문제로 전체 import를 중단하지 않는다.
- Atuin `author`/`intent`는 Rustory entry 모델에 직접 대응 필드가 없으므로 MVP에서는 저장하지 않는다. 필요하면 후속 metadata feature로 분리한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `scripts/start-work.sh --work-id atuin-import` | Atuin 로컬 DB source-of-truth를 확인하고 local-only import 범위와 non-goals 고정 |
| C2 | todo | codex | `cargo test import_accepts_atuin_source --workspace` | `HistoryShell::Atuin`, `rr import --shell atuin`, default path/help 문구 추가 |
| C3 | todo | codex | `cargo test atuin_import_current_schema --workspace` | current Atuin `history` schema fixture를 read-only로 import하고 timestamp ns/duration ns/exit/cwd/hostname 변환 검증 |
| C4 | todo | codex | `cargo test atuin_import_schema_variants --workspace` | `deleted_at`/`author`/`intent`가 없는 older schema와 optional column 존재 schema를 모두 지원 |
| C5 | todo | codex | `cargo test atuin_import_idempotency_and_deleted_rows --workspace` | deterministic idempotency, `deleted_at is not null` skip, limit newest ordering 검증 |
| C6 | todo | codex | `cargo test atuin_import_invalid_utf8 --workspace` | SQLite TEXT invalid UTF-8를 lossy 변환해 row 단위 import를 계속 진행 |
| C7 | todo | codex | `cargo test history_import --workspace` | Hishtory/bash/zsh import 회귀 없이 Atuin SQLite import 경로 통합 |
| C8 | todo | codex | `scripts/run-manifest-checks.sh --mode quick --work-id atuin-import && scripts/check.sh --fast` | 빠른 repo gate와 Rust fast gate 통과 |
| C9 | todo | codex | `scripts/check-doc-links.sh && scripts/check-doc-index.sh && scripts/check-manifest-entrypoints.sh` | Atuin migration 문서와 문서 인덱스/manifest 포인터 갱신 |

## 완료/미완료/다음 액션

- 완료: C1. `scripts/start-work.sh --work-id atuin-import`로 todo workspace를 만들고, Atuin local `history.db` schema/default path/write path를 확인했다.
- 미완료: C2-C9. 아직 구현은 시작하지 않았다.
- 다음 액션: `src/history_import.rs`의 Hishtory SQLite importer 패턴을 확장해 Atuin SQLite importer를 추가하고, `src/cli.rs`의 import dispatch/help를 갱신한다. 이어서 fixture 기반 회귀 테스트와 `docs/atuin-migration.md`를 추가한다.
- 검증 증거: `scripts/start-work.sh --work-id atuin-import`, `nl -ba ../atuin/crates/atuin-client/migrations/20210422143411_create_history.sql`, `nl -ba ../atuin/crates/atuin-client/migrations/20230319185725_deleted_at.sql`, `nl -ba ../atuin/crates/atuin-client/migrations/20260224000100_history_author_intent.sql`, `nl -ba ../atuin/crates/atuin-client/src/settings.rs | sed -n '1468,1494p'`, `nl -ba ../atuin/crates/atuin-client/src/database.rs | sed -n '257,320p'`.
