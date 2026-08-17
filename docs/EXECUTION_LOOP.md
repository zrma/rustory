# Execution Loop

- Audience: Rustory 유지보수자, LLM 에이전트
- Owner: Rustory
- Last Verified: 2026-08-17

이 문서는 구현 작업의 공통 실행 방법론(How)을 고정한다.
피처별 구현 내용(What)은 각 작업의 `docs/todo-*/spec.md`에서 관리한다.

## 문서화 품질 조건

1. 단일 기준 문서 유지
- 동일한 규칙/절차는 한 문서만 소유한다.
- 다른 문서는 규칙을 복제하지 않고 소유 문서 링크로 참조한다.

2. 네비게이션 우선
- 작업 시작 경로는 `docs/HANDOFF.md -> docs/EXECUTION_LOOP.md -> docs/todo-*/spec.md`를 기본으로 유지한다.
- 출고/푸시 경로는 `docs/HANDOFF.md -> docs/EXECUTION_LOOP.md -> docs/CHANGE_CONTROL.md`를 기본으로 유지한다.

3. 최신성 관리
- 정책/절차/검증 명령이 변경되면 같은 턴에서 관련 문서를 즉시 갱신한다.
- 문서 변경 시 `Last Verified`를 갱신한다.
- 문서 품질 게이트(Last Verified/링크/인덱스/README 정책) 명령 목록과 실행 순서는 `docs/CHANGE_CONTROL.md`의 `표준 흐름 > 2. 출고 전 검증`을 단일 기준으로 따른다.

4. 상태 가시성
- 진행 상태의 단일 기준은 `docs/todo-*/spec.md`의 `C1..Cn` 체크리스트와 `완료/미완료/다음 액션` 체크포인트다.
- 핵심 명령/예외 규칙은 소유 문서 링크로만 참조하고 중복 본문을 만들지 않는다.

5. 코드 우선 판단
- 코드/스크립트/매니페스트에 이미 인코딩된 동작, 옵션, 기본값, 테스트 목록은 문서가 재서술하지 않는다.
- 문서는 새 진입점, 안전 불변조건, 소유 경계, 결정 근거, 검증 증거처럼 코드만으로 드러나지 않는 판단 재료를 남긴다.
- 에이전트는 문서가 현재 구현을 요약하길 기다리지 않고 관련 코드/스크립트/CLI help를 직접 확인한다.

## AI-first project overlay

공통 stewardship, 권한, 지속 실행과 검증 기준은 루트 `AGENTS.md`의 `AI-first Core Contract`를,
OpenAI 모델·프롬프트 지침은 `Capability Profile: openai-agent-guidance`를 단일 기준으로 사용한다.
이 문서는 Rustory의 실행 순서와 증거 게이트만 소유하며 공통 기준을 복제하지 않는다.

- Rustory 작업은 `docs/todo-*/spec.md`의 목표와 수용 기준, 실제 코드/스크립트/CLI help, `docs/REPO_MANIFEST.yaml`의 검증 명령으로 범위를 좁힌다.
- 활성 OpenAI 통합 지점이 확인되지 않으면 runtime model, reasoning, Responses API, Pro mode, PTC, tool handler, schema를 추정해 추가하지 않는다.
- historical docs, examples, tests, eval baseline, provider 비교, fallback 경로는 명시 요청이 없으면 그대로 둔다.

## 표준 사이클

1. 구현 + 테스트
- `spec.md`의 `C1..Cn` 항목을 기준으로 구현한다.
- 권장 시작 경로는 `scripts/start-work.sh --work-id <work-id>`로, todo 초기화(`spec.md`, `open-questions.md`)와 초기 게이트(readiness/open-questions/manifest quick)를 단일 명령으로 실행한다.
- 작업 대상 라우팅은 `docs/HANDOFF.md`의 `work-id 라우팅 기준`을 따르며, 실제 todo 탐색 glob과 runner 자동 해석은 `docs/REPO_MANIFEST.yaml`, 대상 스크립트, CLI help를 확인한다.
- 비긴급 변경은 구현 착수 전에 `scripts/check-todo-readiness.sh docs/todo-<work-id>`를 실행해 `spec/open-questions` 준비 상태를 확인한다.
- `docs/todo-*` 관련 staged 변경(`spec.md`, `open-questions.md`, todo 삭제 증거 포함)은 `lefthook pre-commit`에서 `scripts/check-todo-readiness.sh`, `scripts/check-todo-closure.sh`를 선검증한다.
- readiness 게이트의 필수 필드, placeholder 차단, 체크포인트, `C1..Cn` 형식은 `scripts/check-todo-readiness.sh`가 소유한다.
- 질문 카드 스키마/닫힘 상태는 `scripts/check-open-questions-schema.sh --require-closed`로 확인한다. 닫힘 상태의 현재 canonical 문구는 스크립트와 `open-questions.md` 템플릿을 직접 확인한다.
- 구현 중에는 `scripts/check-todo-closure.sh`로 완료된 `docs/todo-*` 잔존 여부를 점검한다.
- 완료된 작업은 `docs/todo-*`를 삭제하고 정식 문서/`docs/LESSONS_LOG.md`에만 내재화한다. (`docs/archive-*` 루트 폴더 생성 금지)
- `todo` 삭제가 포함된 마감 커밋에서는 `todo-<work-id>` 식별자를 `docs/LESSONS_LOG.md` 또는 `docs/LESSONS_ARCHIVE.md`에 남겨 후속 게이트에서 추적 가능하게 유지한다.
- 구현 중 기본 검증 세트는 `scripts/run-manifest-checks.sh --mode quick --work-id <work-id>`로 실행한다.
- quick/full 모드별 필터링, placeholder 처리, repo-key 해석은 `scripts/run-manifest-checks.sh --help`와 스크립트 본문이 소유한다.
- 전체 로컬 검증이 필요하면 `docs/REPO_MANIFEST.yaml`의 현재 check 목록을 기준으로 `--mode full` 또는 `docs/CHANGE_CONTROL.md`의 출고 게이트를 사용한다.
- 운영 게이트 스크립트 변경 시에는 `scripts/check-script-smoke.sh`를 함께 실행해 회귀를 조기 감지한다. (`--work-id`는 다중 todo 환경에서 명시적으로 고정이 필요할 때만 전달)
- 각 항목의 `Verify command`를 우선 실행하고, Rust/P2P 검증 명령은 `docs/REPO_MANIFEST.yaml`, `scripts/check.sh`, `docs/dev-playbook.md`에서 현재 기준을 확인한다.

2. 검수 + 보완
- 피처 규모와 변경 위험도에 맞는 독립 리뷰를 수행하고 교차 검증한다.
- 지적 사항을 반영한 뒤 관련 테스트를 재실행한다.

3. 로컬 change 정리 + 승인된 푸시
- 사용자가 commit/push까지 명시적으로 요청한 턴의 완료 정의는 `원격 푸시 + 원격 SHA 검증`까지이며, 기본 경로는 `scripts/finalize-and-push.sh --message "<type>: <summary>" [--work-id <work-id>]`를 사용한다.
- 원격 쓰기 권한이 없는 구현 요청은 `jj st`/`jj diff`, 관련 검증, 문서 동기화, 필요한 로컬 change description까지 닫고 bookmark 이동이나 push 없이 보고한다.
- `scripts/finalize-and-push.sh`는 기본적으로 `@` non-empty를 요구한다. 빈 작업트리에서 점검이 필요하면 디버그 환경에서만 `DEBUG_GATES_OVERRIDE=1` + `--allow-empty-at` 조합을 사용한다.
- `jj st`/`jj diff`로 변경 상태를 확인하고, Codex-authored change description은 `scripts/finalize-and-push.sh` 또는 `~/.codex/skills/vcs-jj/scripts/describe_with_attribution.sh`로 trailer까지 함께 정리한다.
- strict 게이트/푸시 안전 경로/디버그 우회의 허용 경계와 마감 커밋 예외 정책은 `docs/CHANGE_CONTROL.md`를 따른다. `--work-id` 자동 감지, 옵션/default, 실제 분기는 runner 스크립트와 CLI help를 직접 확인한다.
- 루트 저장소(`.jj` 존재)에서 `git commit` 예외 사용/`jj git import` 동기화 규칙, 교훈 로그 coupling 강제 규칙도 `docs/CHANGE_CONTROL.md`를 단일 기준으로 따른다.

## 검증 증적 기록 규칙

- `C1..Cn` 상태를 변경할 때는 같은 턴에서 `spec.md`의 `완료/미완료/다음 액션` 체크포인트를 갱신한다.
- 체크포인트에는 최소 1개 이상의 실행 명령(또는 산출물 식별자)을 남겨 후속 에이전트가 재검증 경로를 추적할 수 있게 한다.

## 책임 분리 원칙

- `spec.md`: 피처별 구현 범위, 결정 사항, 체크리스트 상태, 검증 명령, 완료 기준.
- `open-questions.md`: 미결 질문만 기록(해결 후 즉시 제거).
- `docs/EXECUTION_LOOP.md`(이 문서): 모든 피처에 공통 적용되는 실행 방법론.

## 관련 문서

- 작업 시작 네비게이션: `docs/HANDOFF.md`
- 출고/배포 절차: `docs/CHANGE_CONTROL.md`
- 회귀 방지 루프: `docs/IMPROVEMENT_LOOP.md`
- 운영 제약/협업 원칙: `docs/OPERATING_MODEL.md`
- 외부 참고: [Harness Engineering (OpenAI)](https://openai.com/index/harness-engineering/)
