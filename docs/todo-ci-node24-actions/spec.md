# Spec: ci-node24-actions

## 배경

- 요청 맥락: 최신 `main` GitHub Actions 실행에서 Node.js 20 기반 액션 deprecation annotation이 발생했다.
- 현재 문제/기회: `actions/checkout@v4`, `actions/setup-python@v5`, 기존 `Swatinem/rust-cache` pin이 Node.js 20 런타임을 사용하므로 2026-06-02 기본 Node 24 전환 전에 워크플로 액션을 갱신한다.

## 계획 스냅샷

- 목표: CI/Docs Integrity/Release Gates 워크플로의 Node 기반 액션을 Node 24 지원 버전으로 올리고 최신 `main` Actions가 annotation 없이 통과하게 한다.
- 범위: `.github/workflows/ci.yml`, `.github/workflows/docs-integrity.yml`, `.github/workflows/release-gates.yml`의 액션 참조와 이 todo 문서만 변경한다.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --dry-run --work-id ci-node24-actions`, `gh run list --branch main --limit 3 --json databaseId,headSha,name,status,conclusion,url`.
- 완료 기준: 로컬 release gates가 통과하고, 커밋/푸시 후 최신 `main`의 Docs Integrity/CI/Release Gates가 모두 success이며 Node.js 20 deprecation annotation이 재발하지 않는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `rg -n "actions/(checkout|setup-python)|rust-cache" .github/workflows` | Node 20 기반 액션 참조를 Node 24 지원 버전으로 갱신 |
| C2 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --dry-run --work-id ci-node24-actions` | 로컬 release gates로 워크플로 변경과 todo 상태 검증 |
| C3 | todo | codex | `gh run list --branch main --limit 3 --json databaseId,headSha,name,status,conclusion,url` | 푸시 후 최신 `main` GitHub Actions 성공과 annotation 해소 확인 |

## 완료/미완료/다음 액션

- 완료: C1, C2. Node 24 지원 태그 확인(`actions/checkout` v6.0.2, `actions/setup-python` v6.2.0, `Swatinem/rust-cache` v2.9.1의 `action.yml`이 `runs.using: node24`).
- 미완료: C3.
- 다음 액션: 첫 커밋을 푸시하고 최신 `main` GitHub Actions 결과와 annotation 해소 여부를 확인한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-ci-node24-actions`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-ci-node24-actions/open-questions.md`, `rg -n "actions/(checkout|setup-python)|rust-cache" .github/workflows`, `scripts/check-release-gates.sh --manifest-mode full --dry-run --work-id ci-node24-actions`.
