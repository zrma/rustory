# Change Control

- Audience: Rustory 유지보수자, 릴리즈 담당자, LLM 에이전트
- Owner: Rustory
- Last Verified: 2026-02-19

이 문서는 1인 개발 + LLM 에이전트 중심 워크플로를 안전하게 운영하기 위한 출고 절차를 정의한다.

## 문서 진입 순서 (무컨텍스트)

- 기본 진입 순서: `docs/HANDOFF.md` -> `docs/EXECUTION_LOOP.md` -> `docs/CHANGE_CONTROL.md`
- `README.md`는 링크 인덱스 탐색이 필요할 때만 참고 문서로 사용한다.
- 문서 진입점/검증 명령의 선언형 단일 기준은 `docs/REPO_MANIFEST.yaml`이다.
- 네비게이션 진입점 변경 시 `docs/README.md` 인덱스와 `docs/REPO_MANIFEST.yaml` entrypoint를 같은 턴에서 동기화한다.
- 네비게이션 문서를 추가/이동/삭제하면 `docs/REPO_MANIFEST.yaml`을 같은 턴에서 갱신하고 `scripts/check-manifest-entrypoints.sh`를 재실행한다.

## 기본 원칙

- 개발 중(`dirty` 상태)에는 생산성을 우선한다.
- 배포/공유 직전에는 strict 검증으로 정합성을 확보한다.
- 고위험/고비용/파괴적 작업 판단은 `docs/ESCALATION_POLICY.md`를 따른다.
- 문서 네비게이션 기본 경로와 구현 단계(ready/open-questions/quick manifest) 기준은 `docs/EXECUTION_LOOP.md`를 단일 기준으로 따른다.
- todo 워크스페이스 탐색 glob 단일 기준은 `docs/REPO_MANIFEST.yaml`의 `maintenance.todo_workspace_glob`을 따른다.
- `docs/todo-*` staged 변경(`spec.md`, `open-questions.md`, todo 삭제 증거 포함)은 `lefthook pre-commit`에서 `scripts/check-todo-readiness.sh` + `scripts/check-todo-closure.sh`를 먼저 통과해야 한다.
- 출고/푸시 직전 strict 게이트 상세는 이 문서를 단일 기준으로 유지한다.

## work-id/manifest 모드 기준 요약

- 공통 탐색 기준: `maintenance.todo_workspace_glob`(기본 `docs/todo-*`)
- `--work-id` 미지정 + todo `1개`: 해당 work-id 자동 선택
- `--work-id` 미지정 + todo `2개 이상`: 자동 단일 선택 없이 전체 readiness 점검
- `--work-id` 명시: `docs/todo-<work-id>`를 직접 검증하며, 디렉터리가 없으면 현재 diff(또는 clean tree일 때 `HEAD^..HEAD`)의 단일 `docs/todo-*` 삭제 증거와 일치할 때만 마감 커밋으로 허용
- `scripts/check-release-gates.sh`, `scripts/finalize-and-push.sh`: todo `0개`면 기본 실패(단일 `docs/todo-*` 삭제 마감 커밋은 자동 허용)
- `scripts/check-push-gates.sh --mode strict`: todo `0개`면 todo readiness만 생략하고 나머지 push 게이트는 계속 수행
- `scripts/check-push-gates.sh --mode quick`: `scripts/check-branch-hygiene.sh`만 빠르게 점검한다. strict 대체 경로가 아니며, 출고/공유 직전에는 strict 모드를 사용한다.
- `--manifest-mode quick`(`check-release-gates`, `finalize-and-push`): 디버그 전용이며 `--allow-quick-manifest` + `DEBUG_GATES_OVERRIDE=1` + non-CI가 아니면 차단

## 표준 흐름

1. 로컬 개발
필요한 모듈에서 자유롭게 수정/검증한다. `jj` rebase/squash/split/force push는 필요 시 사용한다.
비긴급 변경의 계획 스냅샷/질문 카드/ready 기준, 질문 카드 닫힘 상태(`현재 미결 항목 없음.`), 완료 todo 위생 기준, quick manifest 사용 규칙은 `docs/EXECUTION_LOOP.md`의 `표준 사이클 > 1. 구현 + 테스트`를 단일 기준으로 따른다.
권장 시작/기본 검증 경로(`scripts/start-work.sh --work-id <work-id>`, `scripts/run-manifest-checks.sh --mode quick --work-id <work-id>`)도 같은 기준 문서를 따른다.

2. 출고 전 검증
`scripts/check-release-gates.sh --manifest-mode full [--work-id <work-id>]`를 우선 실행한다.
`--work-id`를 생략하면 `maintenance.todo_workspace_glob`(기본 `docs/todo-*`)을 자동 감지하며, `1개`면 해당 work-id를 자동 선택하고, `2개 이상`이면 전체 readiness를 자동 점검하며, `0개`면 기본 실패한다. 단, 현재 diff 또는 로컬 변경이 없을 때의 직전 커밋(`HEAD^..HEAD`)에 단일 `docs/todo-*` 삭제가 감지되면 마감 커밋으로 자동 허용한다. `--work-id`를 명시한 경우에도 해당 work-id가 삭제 증거(`현재 diff` 또는 로컬 변경이 없을 때의 `HEAD^..HEAD`)와 일치할 때만 마감 커밋으로 허용한다. (그 외 예외: `--allow-missing-work-id` + `DEBUG_GATES_OVERRIDE=1` + non-CI)
이 게이트는 아래 순서의 검증을 내부에서 실행한다.
- `scripts/check-todo-readiness.sh docs/todo-<work-id>` (work-id가 있을 때, 자동 감지 규칙 포함)
- `scripts/check-prod-preflight.sh` (submodule이 없으면 자동 skip)
- `scripts/check-branch-hygiene.sh`
- `scripts/check-jj-conflicts.sh`
- `scripts/check-open-questions-schema.sh --require-closed` (`--work-id` 지정 시 `docs/todo-<work-id>/open-questions.md`만 검사, 미지정 시 감지된 `docs/todo-*`의 `open-questions.md`만 검사)
- `scripts/check-submodules.sh --strict` (submodule이 없으면 자동 skip, strict 모드에서는 원격 fetch를 비대화형으로 수행해 인증 프롬프트 대기를 차단)
- `scripts/check-doc-last-verified.sh`
- `scripts/check-doc-links.sh`
- `scripts/check-doc-index.sh` (최상위 `docs/*.md` + 중첩 디렉터리 `README.md` + 중첩 문서의 동일 디렉터리 인덱스 링크를 검사, `maintenance.todo_workspace_glob`은 제외)
- `scripts/check-readme-policy.sh` (README 라인/H2/H3/code fence 상한 + 필수 H2/링크 존재를 검사; 기본 상한값: lines=220, H2=8, H3=6, code fences=6)
- `scripts/check-manifest-entrypoints.sh` (`docs/REPO_MANIFEST.yaml`의 top-level canonical 포인터 + `repositories[rustory].entrypoints` 필수 항목 + `maintenance.*` 필수 키를 검증; `python3` + `yaml` 패키지 필요)
- `scripts/check-script-smoke.sh`
- `scripts/check-todo-closure.sh` (완료된 `docs/todo-*` 삭제 증거가 있을 때 `docs/LESSONS_LOG.md`/`docs/LESSONS_ARCHIVE.md`의 `todo-<work-id>` 식별자 참조 포함)
- `scripts/check-lessons-log-enforcement.sh --worktree` (`docs/LESSONS_LOG.md` 단독 변경 차단)
- `scripts/check-push-gates.sh --mode strict`
- `scripts/run-manifest-checks.sh --mode <quick|full> --repo-key <repo-key>`
- release 게이트의 `--manifest-mode quick`은 디버그 전용이며 `--allow-quick-manifest` + `DEBUG_GATES_OVERRIDE=1`(non-CI)이 아니면 차단된다.
- `scripts/run-manifest-checks.sh --mode quick`은 `scripts/check-*` 게이트만 실행하고, manifest의 일반 로컬 검증(예: `cargo fmt/test/clippy`)은 skip될 수 있다. 전체 검증이 필요하면 `--mode full`을 사용한다.
- `scripts/check-jj-root-git-commit.sh`는 `lefthook commit-msg` 강제용 hook-only guard라 standalone manifest 실행에서는 skip 로그가 정상이다.
게이트 실패 시에는 실패한 개별 명령을 같은 옵션으로 단독 재실행해 원인을 좁힌 뒤, 필요한 모듈별 필수 검증(`docs/HANDOFF.md` 기준)을 추가로 수행한다.

3. 최종 커밋/푸시
기본 자동 경로는 `scripts/finalize-and-push.sh --message "<type>: <summary>" [--work-id <work-id>]`를 사용해, 게이트/커밋/푸시/SHA 검증까지 한 번에 닫는다.
`--message`는 `<type>: <summary>` 형식(`feat|fix|perf|refactor|docs|test|build|ci|chore|revert`)만 허용하며 scope 괄호(`feat(scope):`)는 차단된다.
`--work-id` 미지정 시 `maintenance.todo_workspace_glob` 자동 감지 규칙(`1개` 자동 선택, `2개 이상` 전체 readiness 점검, `0개` 기본 실패 + 단일 `docs/todo-*` 삭제 마감 커밋 자동 허용)을 따른다.
`--work-id`를 명시하면 해당 work-id가 삭제 증거(`현재 diff` 또는 로컬 변경이 없을 때의 `HEAD^..HEAD`)와 일치할 때만 마감 커밋으로 진행할 수 있다.
`--remote <name>`를 지정하면 push 대상과 SHA 검증 대상이 동일 remote로 고정되며, 실제 실행에서는 `git remote get-url <remote>` 선검증으로 remote 오타/미설정 상태를 `jj describe` 이전에 차단한다.
원격 브랜치가 없는 첫 push는 remote SHA 조회를 재시도한 뒤 검증하며, 조회 성공 후 SHA mismatch는 기존과 동일하게 차단한다.
`--manifest-mode quick`은 디버그 전용이며 `--allow-quick-manifest`와 `DEBUG_GATES_OVERRIDE=1`(non-CI 환경)을 함께 주지 않으면 차단된다.
`scripts/finalize-and-push.sh`는 기본적으로 `@` non-empty를 요구한다. 빈 작업트리에서 점검이 필요할 때만 디버그 환경에서 `DEBUG_GATES_OVERRIDE=1` + `--allow-empty-at` 조합을 사용한다. (CI 환경 불가)
수동 경로가 필요하면 `jj describe -m`, `jj bookmark move`, `scripts/jj-git-push-safe.sh`, `git ls-remote --heads origin <bookmark>` 순서로 수행한다.

추가 강제:
- `lefthook` `pre-push`에서 `scripts/check-release-gates.sh --manifest-mode full`와 `scripts/check-lessons-log-range.sh --remote origin --bookmark main`를 실행해 release/push 게이트 및 교훈 로그 range coupling을 재확인한다.
- `scripts/jj-git-push-safe.sh`도 push 직전에 `check-release-gates -> check-jj-conflicts --bookmark <target> -> check-lessons-log-range` 순서의 동일 강제를 수행한다.
- `scripts/jj-git-push-safe.sh` 기본 모드는 `PUSH_GATES_MODE=strict`이며, non-strict 우회는 `ALLOW_NON_STRICT_PUSH_GATES=1` + `DEBUG_GATES_OVERRIDE=1` + non-CI 조합이 아니면 차단된다.

4. 사후 점검(조건부)
누락/실패/재작업이 있었으면 `docs/IMPROVEMENT_LOOP.md`를 따라 `docs/LESSONS_LOG.md`에 기록한다.

## main 승격 후 정리 체크리스트

- 릴리즈 기준 커밋을 `main` 북마크로 먼저 고정한다.
  - 예시: `jj bookmark move main --to <target-rev>`
- 원격 `main` 푸시 후 실제 반영 SHA를 즉시 확인한다.
  - 예시: `scripts/jj-git-push-safe.sh --bookmark main` + `git ls-remote --heads origin main`
- 중간 작업용 북마크/브랜치(`rewrite/*`, `backup/*`)는 같은 턴에서 정리한다.
  - 예시: `jj bookmark delete <temp-bookmark>...`
- 정리 직후 상태를 교차 검수한다.
  - `jj bookmark list`에 `main`만 남았는지 확인
  - `scripts/check-branch-hygiene.sh`로 임시 브랜치 + unbookmarked non-empty `jj head` 잔존 여부 확인
  - `jj st`, `git status --short`가 clean인지 확인
- 승격 과정에서 재작업/혼선이 있었으면 같은 턴에서 `LESSONS_LOG`에 원인/대응/검증을 기록한다.

## 1인 개발 모드 권장 규칙

- CI를 항상 강제 게이트로 쓰지 않고, 출고 직전에 수동 strict 검증을 수행한다.
- 작업 도중 생성된 고아 draft change는 마무리 시 정리한다.
예시: `jj st` 확인 후 불필요 change에 `jj abandon <change-id>`

## 참고 명령

```bash
# 비긴급 구현 착수 권장 단일 경로(초기화 + 초기 게이트)
scripts/start-work.sh --work-id <work-id>

# manifest 선언 체크를 quick 모드로 실행
scripts/run-manifest-checks.sh --mode quick --work-id <work-id>

# 출고/공유 직전 통합 게이트 실행
scripts/check-release-gates.sh --manifest-mode full [--work-id <work-id>]

# push 전 통합 게이트 실행
scripts/check-push-gates.sh --mode strict [--work-id <work-id>]

# 로컬 빠른 위생 점검(브랜치 위생만) - strict 대체 불가
scripts/check-push-gates.sh --mode quick

# 구현 턴 종료 자동 경로 (게이트 + 커밋 + 푸시 + SHA 검증)
scripts/finalize-and-push.sh --message "<type>: <summary>" [--work-id <work-id>]

# jj push 안전 경로 (기본 main)
scripts/jj-git-push-safe.sh

# jj push 안전 경로 (게이트 모드/작업 ID 지정)
PUSH_GATES_MODE=strict PUSH_GATES_WORK_ID=<work-id> scripts/jj-git-push-safe.sh

# 원격 push 범위 기준 교훈 로그 coupling 체크
scripts/check-lessons-log-range.sh --remote origin --bookmark main
```
