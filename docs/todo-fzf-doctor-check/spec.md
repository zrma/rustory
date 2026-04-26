# Spec: fzf-doctor-check

## 배경

- 요청 맥락: 활성 `docs/todo-*`가 없어 다음 작업 후보를 검토했고, MVP 핵심 UX인 ctrl+r 검색 의존성 점검을 운영성 개선으로 선정했다.
- 현재 문제/기회: `rr search`/hook ctrl+r은 외부 실행 파일 `fzf`가 필요하지만, 현재 `rr doctor`와 온보딩 문서는 설치 누락을 사전에 드러내지 못한다.

## 계획 스냅샷

- 목표: `rr doctor`가 `fzf` 설치/PATH 상태를 텍스트와 JSON 모두에서 보고하게 하여 ctrl+r 검색 실패를 온보딩 단계에서 조기 발견한다.
- 범위: `src/cli.rs`의 doctor report/출력/테스트, `docs/quickstart.md`와 `docs/hook.md`의 fzf 의존성 안내, todo 체크포인트 갱신.
- 검증 명령: `cargo fmt --all --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/run-manifest-checks.sh --mode quick --work-id fzf-doctor-check`.
- 완료 기준: doctor 텍스트/JSON이 `fzf` 상태를 안정적으로 노출하고, 문서가 ctrl+r 검색의 `fzf` 의존성을 안내하며, C-체크리스트 검증 명령이 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test --workspace` | `rr doctor` 텍스트/JSON에 `fzf` 가용성 보고를 추가하고 회귀 테스트를 보강한다. |
| C2 | done | codex | `scripts/check-doc-links.sh` | quickstart/hook 문서에 ctrl+r 검색의 `fzf` 의존성과 doctor 확인 경로를 반영한다. |
| C3 | todo | codex | `scripts/finalize-and-push.sh --message "docs: close fzf doctor todo" --work-id fzf-doctor-check` | 구현 커밋 푸시 후 별도 마감 change에서 todo 삭제와 Lessons 기록을 완료한다. |

## 완료/미완료/다음 액션

- 완료: C1, C2.
- 미완료: C3.
- 다음 액션: 구현 커밋을 푸시한 뒤 todo 삭제와 Lessons 기록을 별도 마감 change로 진행한다.
- 검증 증거: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/check-doc-links.sh`, `scripts/run-manifest-checks.sh --mode quick --work-id fzf-doctor-check`, `cargo run --quiet -- doctor`.
