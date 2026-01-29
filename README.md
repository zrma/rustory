# Rustory

분산/동기화 기반 셸 히스토리 관리 도구 (MVP 설계 단계)

## 목표 (MVP)
- append-only 기반 히스토리 수집
- entry_id 기반 dedup 동기화
- fzf 기반 로컬 검색 UI
- bash/zsh 후킹

## 현재 상태
- 설계 문서: `docs/todo-mvp-sync/`
- PoC 구현 진행 중:
  - SQLite 스토리지(ingest_seq cursor, dedup)
  - pull 기반 sync 코어 루프
  - HTTP(디버그) `serve`/`sync`

## 다음 단계
- hook으로 엔트리 수집(로컬 insert)
- ctrl+r + fzf UI
- P2P(tracker/relay) transport로 이식

## 개발
- 테스트: `cargo test`
- 로컬 실행: `cargo run --bin rr -- --help`
