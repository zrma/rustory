# Change Control

- Audience: Rustory 유지보수자, 릴리즈 담당자, LLM 에이전트
- Owner: Rustory
- Last Verified: 2026-07-12

이 문서는 1인 개발 + LLM 에이전트 중심 워크플로를 안전하게 운영하기 위한 출고 절차를 정의한다.

## 문서 진입 순서 (무컨텍스트)

- 기본 진입 순서: `docs/HANDOFF.md` -> `docs/EXECUTION_LOOP.md` -> `docs/CHANGE_CONTROL.md`
- `README.md`는 링크 인덱스 탐색이 필요할 때만 참고 문서로 사용한다.
- 문서 진입점/검증 명령의 선언형 단일 기준은 `docs/REPO_MANIFEST.yaml`이다.
- 게이트 내부 실행 순서와 옵션 파싱은 `scripts/check-release-gates.sh`, `scripts/check-push-gates.sh`, `scripts/finalize-and-push.sh`, `scripts/run-manifest-checks.sh`를 직접 확인한다.
- 네비게이션 진입점 변경 시 `docs/README.md` 인덱스와 `docs/REPO_MANIFEST.yaml` entrypoint를 같은 턴에서 동기화한다.
- 네비게이션 문서를 추가/이동/삭제하면 `docs/REPO_MANIFEST.yaml`을 같은 턴에서 갱신하고 `scripts/check-manifest-entrypoints.sh`를 재실행한다.

## 기본 원칙

- 개발 중(`dirty` 상태)에는 생산성을 우선한다.
- 배포/공유 직전에는 strict 검증으로 정합성을 확보한다.
- 고위험/고비용/파괴적 작업 판단은 `docs/ESCALATION_POLICY.md`를 따른다.
- 문서 네비게이션 기본 경로와 구현 단계(ready/open-questions/quick manifest) 기준은 `docs/EXECUTION_LOOP.md`를 단일 기준으로 따른다.
- todo 워크스페이스 탐색 glob 단일 기준은 `docs/REPO_MANIFEST.yaml`의 `maintenance.todo_workspace_glob`을 따른다.
- 실행 동작, 옵션 default, 스크립트 내부 분기는 문서가 재서술하지 않고 해당 스크립트와 CLI help를 직접 확인한다.
- `docs/todo-*` staged 변경(`spec.md`, `open-questions.md`, todo 삭제 증거 포함)은 `lefthook pre-commit`에서 `scripts/check-todo-readiness.sh` + `scripts/check-todo-closure.sh`를 먼저 통과해야 한다.
- 출고/푸시 직전 strict 게이트의 정책 경계는 이 문서를 기준으로 유지하고, 실제 실행 순서/옵션/default는 runner 스크립트가 소유한다.
- 공개 출고 증거는 repository-owned 검사 결과로 제한한다. 개인 배포 inventory, hostname, 실제 endpoint/IP, fleet 규모, rollout revision/checksum, 머신 로컬 경로는 commit·tag·Release 본문에 기록하지 않는다.
- published history 정리 후에는 현재 tree만 검사하지 않고 `publication boundary --mode all`, 원격 `main`·전체 tag target, GitHub Release와 source archive를 함께 재검증한다.

## work-id/manifest 모드 기준 요약

- todo workspace 위치는 `docs/REPO_MANIFEST.yaml`의 `maintenance.todo_workspace_glob`에서 확인한다.
- release/finalize 계열 게이트는 명시 또는 자동 해석 가능한 `work-id`를 요구한다. 완료 todo 삭제 마감처럼 `work-id` 디렉터리가 없는 경우에는 삭제 증거와 일치할 때만 예외로 허용한다.
- active todo가 여러 개이면 실제 수정/마감 대상은 `--work-id`로 고정한다. 게이트는 나머지 active todo의 readiness/open-questions 위생도 함께 확인해야 한다.
- push 계열 게이트는 출고 직전 위생 확인을 담당하며, no-active-todo 상황에서도 todo readiness 외 항목은 계속 확인할 수 있어야 한다.
- quick manifest/release 경로는 디버그 전용이다. 허용 조건과 옵션 해석은 runner 스크립트와 `--help`가 소유하며, 이 문서는 사람 승인 없는 우회를 금지한다는 경계만 소유한다.

## 표준 흐름

1. 로컬 개발
필요한 모듈에서 자유롭게 수정/검증한다. `jj` rebase/squash/split/force push는 필요 시 사용한다.
비긴급 변경의 계획 스냅샷/질문 카드/ready 기준, 질문 카드 닫힘 상태, 완료 todo 위생 기준, quick manifest 사용 규칙은 `docs/EXECUTION_LOOP.md`의 `표준 사이클 > 1. 구현 + 테스트`를 단일 기준으로 따른다.
권장 시작/기본 검증 경로(`scripts/start-work.sh --work-id <work-id>`, `scripts/run-manifest-checks.sh --mode quick --work-id <work-id>`)도 같은 기준 문서를 따른다.

2. 출고 전 검증
`scripts/check-release-gates.sh --manifest-mode full [--work-id <work-id>]`를 우선 실행한다.
`--work-id`는 명시하거나 active todo 상태에서 runner가 해석하게 둔다. todo 삭제 마감 커밋, CI의 no-active-todo 예외, 다중 todo 처리처럼 구현 세부가 필요한 경우에는 runner 스크립트와 `--help`를 직접 확인한다.
이 게이트가 실제로 어떤 스크립트와 manifest check를 어떤 순서로 실행하는지는 `scripts/check-release-gates.sh`, `scripts/check-push-gates.sh`, `scripts/run-manifest-checks.sh`, `docs/REPO_MANIFEST.yaml`을 직접 확인한다. 이 문서는 출고 전 검증의 의도와 예외 경계만 소유한다.

검증 축은 다음 범주로 해석한다.
- todo readiness/open-questions/closure 위생
- branch/jj/submodule/preflight 위생
- 문서 Last Verified/link/index/README/manifest entrypoint 위생
- script smoke와 lessons-log coupling
- manifest mode에 따른 Rust/스모크 검증

`--manifest-mode quick`은 디버그 전용이며 `--allow-quick-manifest` + `DEBUG_GATES_OVERRIDE=1`(non-CI)이 아니면 차단된다. 전체 검증이 필요하면 `--manifest-mode full`을 사용한다.
게이트 실패 시에는 실패한 개별 명령을 같은 옵션으로 단독 재실행해 원인을 좁힌 뒤, 필요한 모듈별 필수 검증(`docs/HANDOFF.md` 기준)을 추가로 수행한다.

3. 최종 커밋/푸시
기본 자동 경로는 `scripts/finalize-and-push.sh --message "<type>: <summary>" [--work-id <work-id>]`를 사용해, 게이트/커밋/푸시/SHA 검증까지 한 번에 닫는다.
`--message`는 `<type>: <summary>` 형식(`feat|fix|perf|refactor|docs|test|build|ci|chore|revert`)만 허용하며 scope 괄호(`feat(scope):`)는 차단된다.
`--work-id` 자동 감지, 다중 todo, 삭제 마감 커밋 예외는 runner 동작을 직접 확인한다.
`--remote <name>`를 지정하면 push 대상과 SHA 검증 대상이 동일 remote로 고정되며, 실제 실행에서는 `git remote get-url <remote>` 선검증으로 remote 오타/미설정 상태를 `jj describe` 이전에 차단한다.
원격 브랜치가 없는 첫 push는 remote SHA 조회를 재시도한 뒤 검증하며, 조회 성공 후 SHA mismatch는 기존과 동일하게 차단한다.
`--manifest-mode quick`은 디버그 전용이며 `--allow-quick-manifest`와 `DEBUG_GATES_OVERRIDE=1`(non-CI 환경)을 함께 주지 않으면 차단된다.
`scripts/finalize-and-push.sh`는 기본적으로 `@` non-empty를 요구한다. 빈 작업트리에서 점검이 필요할 때만 디버그 환경에서 `DEBUG_GATES_OVERRIDE=1` + `--allow-empty-at` 조합을 사용한다. (CI 환경 불가)
수동 경로가 필요하면 `~/.codex/skills/vcs-jj/scripts/describe_with_attribution.sh`, `jj bookmark move`, `scripts/jj-git-push-safe.sh`, `git ls-remote --heads origin <bookmark>` 순서로 수행한다. Codex-authored change description은 `Co-authored-by` trailer가 마지막 non-empty line에 정확히 1회 있어야 한다.

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
