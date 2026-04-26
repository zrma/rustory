# Spec: hook-disable-bool

## 배경

- 요청 맥락: 활성 todo가 없어 다음 MVP 온보딩/doctor 표면을 검토하던 중 hook disable env 해석이 다른 boolean env와 달랐다.
- 현재 문제/기회: `RUSTORY_ASYNC_UPLOAD` 같은 boolean env는 `0/false/no/off`를 false로 해석하지만, shell hook과 doctor의 `RUSTORY_HOOK_DISABLE`은 값이 존재하기만 하면 비활성으로 판단한다. 사용자가 임시 override로 `RUSTORY_HOOK_DISABLE=0`을 둔 경우 기록/검색 hook이 계속 꺼지는 혼선을 줄일 수 있다.

## 계획 스냅샷

- 목표: `RUSTORY_HOOK_DISABLE`의 true/false 해석을 다른 boolean env와 일관되게 만들고 doctor 출력도 실제 hook 동작과 맞춘다.
- 범위: bash/zsh hook 템플릿, doctor hook status, 관련 테스트와 hook 문서만 수정한다. config 파일 기반 hook disable 옵션은 새로 추가하지 않는다.
- 검증 명령: `cargo test hook_status --workspace`, `cargo test hook::tests --workspace`, `cargo fmt --all --check`, `scripts/run-manifest-checks.sh --mode quick --work-id hook-disable-bool`.
- 완료 기준: `RUSTORY_HOOK_DISABLE=0/false/no/off`는 hook을 끄지 않고, true 값은 기존처럼 끄며, 알 수 없는 값은 안전하게 비활성 처리되고 문서/테스트에 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test hook_status --workspace` | doctor hook status가 `RUSTORY_HOOK_DISABLE` boolean 값을 일관되게 해석한다. |
| C2 | done | codex | `cargo test hook::tests --workspace` | bash/zsh hook 템플릿이 false 값을 disable로 보지 않는 helper를 포함한다. |
| C3 | done | codex | `cargo fmt --all --check` + `scripts/run-manifest-checks.sh --mode quick --work-id hook-disable-bool` | 관련 문서와 todo 체크포인트를 갱신하고 기본 게이트를 통과한다. |
| C4 | todo | codex | `scripts/finalize-and-push.sh --message "docs: close hook disable todo" --work-id hook-disable-bool` | 구현 커밋 후 todo 삭제와 교훈 로그를 별도 마감 커밋으로 닫는다. |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3.
- 미완료: C4.
- 다음 액션: 구현 커밋을 finalize/push 한 뒤 C4에서 todo 삭제와 교훈 로그를 별도 마감 커밋으로 닫는다.
- 검증 증거: `cargo test hook_status --workspace`, `cargo test hook::tests --workspace`, `cargo fmt --all --check`, `cargo build`, doctor smoke(`RUSTORY_HOOK_DISABLE=0` -> `disabled=false`, `RUSTORY_HOOK_DISABLE=maybe` -> `disabled=true` + warning), `target/debug/rr hook --shell bash | bash -n`, `target/debug/rr hook --shell zsh | zsh -n`, `scripts/run-manifest-checks.sh --mode quick --work-id hook-disable-bool`, `scripts/finalize-and-push.sh --message "fix: parse hook disable as boolean" --work-id hook-disable-bool` 실패 확인(closure gate: 모든 C가 done인데 todo가 남아 있었음).
