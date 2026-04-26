# Spec: hook-search-limit-config

## 배경

- 요청 맥락: 활성 todo가 없어 다음 MVP 온보딩/운영 표면을 검토하던 중 hook 검색 limit 설정 경로를 확인했다.
- 현재 문제/기회: `config.toml`에는 `search_limit_default`가 있지만 bash/zsh hook이 항상 `rr search --limit 100000`을 호출해 config fallback을 우회한다. `rr doctor`의 hook 섹션도 env가 없으면 config 값이 아니라 hardcoded 기본값을 보여준다.

## 계획 스냅샷

- 목표: ctrl+r hook 검색 limit이 `RUSTORY_SEARCH_LIMIT` > `config.toml search_limit_default` > 기본값 순서로 일관되게 해석되도록 한다.
- 범위: shell hook의 `rr search` 호출, `rr doctor` hook status의 effective limit 표시, hook/quickstart/P2P 문서의 설정 우선순위 설명.
- 검증 명령: `cargo test hook --workspace`, `cargo test search_limit --workspace`, `cargo test doctor --workspace`, `cargo fmt --all --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/run-manifest-checks.sh --mode quick --work-id hook-search-limit-config`.
- 완료 기준: hook이 `--limit 100000`을 직접 주입하지 않고, doctor/test/docs가 동일 우선순위를 설명하며, 위 검증 명령이 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `scripts/start-work.sh --work-id hook-search-limit-config` | 작업 스냅샷과 질문 카드 닫힘 상태를 초기화한다 |
| C2 | done | codex | `cargo test hook --workspace && cargo test search_limit --workspace && cargo test doctor --workspace` | hook/doctor/search limit resolver를 일관되게 수정하고 회귀 테스트를 추가한다 |
| C3 | in_progress | codex | `cargo fmt --all --check && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && scripts/run-manifest-checks.sh --mode quick --work-id hook-search-limit-config` | 문서와 최종 검증 증적을 갱신한다 |

## 완료/미완료/다음 액션

- 완료: C1, C2. Hook이 `rr search` resolver를 직접 사용하도록 바뀌었고 doctor hook status가 config search limit fallback을 표시한다.
- 미완료: C3.
- 다음 액션: 전체 Rust 검증과 manifest quick 게이트를 실행하고 문서 증적을 마감한다.
- 검증 증거: `scripts/start-work.sh --work-id hook-search-limit-config`, `cargo test hook --workspace`, `cargo test search_limit --workspace`, `cargo test doctor --workspace`.
