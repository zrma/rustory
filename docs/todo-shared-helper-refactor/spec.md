# Spec: shared-helper-refactor

## 배경

- 요청 맥락: 중복된 import SQLite 변환과 retry backoff 계산을 공통 helper로 정리하고 안전하게 출고한다.
- 현재 문제/기회: Atuin/Hishtory 및 HTTP/P2P 경로에 같은 구현과 같은 회귀 테스트가 각각 존재해 수정 지점이 분산돼 있었다.

## 계획 스냅샷

- 목표: 동작과 공개 API를 바꾸지 않고 중복 helper를 단일 구현과 단일 단위 테스트로 통합한다.
- 범위: `history_import`의 SQLite 값 변환 helper, HTTP/P2P retry duration helper, 관련 module 선언과 테스트만 포함한다.
- 검증 명령: `scripts/check.sh --fast`, `cargo test --no-default-features --workspace`, import feature별 `cargo check`, publication/push gate.
- 완료 기준: 기본 및 feature-minimal 검증이 통과하고 `main`과 `origin/main`이 같은 새 commit을 가리킨다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `rg -n 'exponential_duration|row_lossy_(opt_)?string' src --glob '*.rs'` | 중복 helper를 `src/retry.rs`와 `src/history_import/sqlite.rs`로 통합 |
| C2 | done | codex | `scripts/check.sh --fast` | 기본, no-default-features, import feature별 회귀 검증 수행 |
| C3 | todo | codex | `git ls-remote --heads origin main` | publication boundary와 full gate 통과 후 `main` push 및 원격 SHA 확인 |

## 완료/미완료/다음 액션

- 완료: C1, C2. 공통 helper 추출과 로컬 회귀 검증을 완료했다.
- 미완료: C3 원격 push 및 SHA 확인.
- 다음 액션: `scripts/finalize-and-push.sh --message "refactor: consolidate import and retry helpers" --work-id shared-helper-refactor`를 실행하고 원격 상태를 재조회한다.
- 검증 증거: `scripts/check.sh --fast`에서 Rust 423 tests, Python installer 27 tests, clippy, P2P smoke 통과. `cargo test --no-default-features --workspace` 405 tests 통과. `import-hishtory`, `import-atuin` 단독 check 통과. repository 및 machine-local publication boundary `push` 모드 통과.
