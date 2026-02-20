# Spec: docs-integrity-fetch-depth-fix

## 배경

- 요청 맥락: `72e4422` 이후 `Docs Integrity` GitHub Actions가 연속 실패하고 있어 즉시 복구가 필요하다.
- 현재 문제/기회: `actions/checkout` 기본 shallow clone(fetch-depth=1)에서 `check-doc-last-verified.sh`가 문서 최신 변경일을 HEAD 시점으로 오판해 false failure가 발생한다.

## 계획 스냅샷

- 목표: `Docs Integrity` 워크플로가 full git history를 기반으로 `Last Verified` 검증을 수행하도록 CI 설정을 수정한다.
- 범위:
  - `.github/workflows/docs-integrity.yml`의 checkout depth 설정
  - 재현 검증( shallow/full clone 비교 )
- 검증 명령:
  - `scripts/run-manifest-checks.sh --mode quick --work-id docs-integrity-fetch-depth-fix`
  - `git clone --depth=1 ... && scripts/check-doc-last-verified.sh` (실패 재현)
  - `git clone ... && scripts/check-doc-last-verified.sh` (정상 통과 확인)
- 완료 기준:
  - 워크플로에 `fetch-depth: 0`이 반영됨
  - 로컬 재현으로 shallow/full clone 차이를 확인했고, 수정 의도가 명확함
  - quick 게이트 및 finalize/push가 통과함

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `git clone --depth=1 ... && scripts/check-doc-last-verified.sh` | shallow clone에서 false failure 재현 근거를 확보한다 |
| C2 | in_progress | codex | `scripts/finalize-and-push.sh --message "ci: use full fetch depth in docs integrity workflow" --work-id docs-integrity-fetch-depth-fix` | workflow에 `fetch-depth: 0` 적용 및 커밋/푸시까지 완료한다 |

## 완료/미완료/다음 액션

- 완료: C1.
- 미완료: C2.
- 다음 액션: `scripts/finalize-and-push.sh --message "ci: use full fetch depth in docs integrity workflow" --work-id docs-integrity-fetch-depth-fix` 실행으로 출고를 마무리한다.
- 검증 증거:
  - `scripts/check-todo-readiness.sh docs/todo-docs-integrity-fetch-depth-fix`
  - `scripts/check-open-questions-schema.sh --require-closed docs/todo-docs-integrity-fetch-depth-fix/open-questions.md`
  - `git clone --depth=1 https://github.com/zrma/rustory.git <tmp> && scripts/check-doc-last-verified.sh` => fail
  - `git clone https://github.com/zrma/rustory.git <tmp> && scripts/check-doc-last-verified.sh` => pass
  - `scripts/run-manifest-checks.sh --mode quick --work-id docs-integrity-fetch-depth-fix`
