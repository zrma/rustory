# AGENTS.md

## Workflow: docs/todo-*

- 계획된 작업은 `docs/todo-<short-work-id>/` 형식의 폴더로 시작한다.
- 필수 파일은 `spec.md`, `open-questions.md`이다.
- 작성/승인/갱신/종료 기준(계획 스냅샷, `C1..Cn`, 체크포인트, todo 삭제)은 `docs/EXECUTION_LOOP.md`를 단일 기준으로 따른다.
- 작업 시작은 `scripts/start-work.sh --work-id <work-id>`를 권장 경로로 사용한다.

## Workflow: Anti-regression

- 비긴급 변경은 코드/문서 수정 전에 `scripts/check-todo-readiness.sh docs/todo-<work-id>`로 준비 상태를 확인한다.
- 질문 카드/닫힘 상태는 `scripts/check-open-questions-schema.sh --require-closed`를 기준으로 관리한다.
- 누락/실패/재작업 대응은 `docs/IMPROVEMENT_LOOP.md`를 단일 기준으로 따른다.

## Workflow: Autonomous execution

- 에이전트는 사용자에게 세부 지시를 요구하지 않고, 저장소 문서/스크립트 기준으로 목표 달성까지 자율적으로 진행한다.
- 사용자 호출은 `docs/ESCALATION_POLICY.md`의 즉시 에스컬레이션 조건 또는 명시적 사용자 판단이 필요한 경우로 제한한다.
- 그 외 진행 상황은 간단히 공유하되, 구현/검증/문서화와 로컬 change 정리를 같은 턴에서 닫는다. 원격 push는 사용자 요청이 명시적으로 권한을 부여한 경우에만 수행한다.

## Workflow: Code-first documentation

- 실행 동작, 옵션, 기본값, 검증 목록은 문서 요약을 신뢰하기 전에 `src/*`, `scripts/*`, `Cargo.toml`, `docs/REPO_MANIFEST.yaml`, CLI help를 직접 확인한다.
- 문서는 네비게이션, 소유 경계, 안전 불변조건, 결정 근거, 검증 증거처럼 코드만으로 드러나지 않는 판단 재료를 소유한다.
- 문서가 구현 현황을 재서술하고 있으면 가능한 한 코드/스크립트/매니페스트 포인터로 줄이고, 중복 본문은 `docs/README_OPERATING_POLICY.md` 기준으로 제거한다.

<!-- agent-harness-baseline:start -->
## Agent Harness Baseline (GPT-5.6)

Baseline ID: `openai-gpt-5.6-2026-07-10`.

- Source of truth: use the `openai-docs` skill and the official [latest model guide](https://developers.openai.com/api/docs/guides/latest-model) plus [prompting best practices](https://developers.openai.com/api/docs/guides/latest-model#prompting-best-practices) before changing OpenAI model, API, prompt, or agent guidance.
- Model target: when the task asks for the current or latest OpenAI baseline, use `gpt-5.6`. This is harness guidance, not proof that the application calls OpenAI; change runtime model strings only at an existing OpenAI integration point.
- Prompt budget: start with the smallest prompt and task-relevant tool set that reliably completes the work. Preserve project-specific constraints, remove redundant generic instructions, and add examples only for an observed failure.
- Request modes: for answer, explain, review, diagnose, or plan requests, inspect and report without implementation. For change, build, or fix requests, make the requested in-scope local changes and run relevant non-destructive validation.
- Permissions: reading, searching, editing in-scope files, and running non-destructive checks are pre-authorized for change tasks. Require confirmation for external writes not explicitly requested, destructive or irreversible actions, purchases or cost, secrets, or material scope expansion.
- Persistence: continue until the requested outcome is complete; do not stop after only analysis, a partial patch, or an intermediate tool success. Stop and escalate only at a real permission, product-decision, or external-state boundary.
- Verification: treat tool and patch success as provisional. Re-read the diff and verify the user-visible or runtime outcome with the narrowest meaningful checks, then broaden only when risk warrants it.
- Output: lead with the conclusion. Include required evidence, material caveats, and the next action; trim introductions, repetition, generic reassurance, and optional background before trimming required content.
- Structure: use a lightweight task-specific plan or output shape. Do not impose a global template or long process narration when the repository already supplies the necessary workflow.
- Modes and orchestration: configure Pro mode in the API or runtime rather than asking the model to “think harder.” Use Programmatic Tool Calling only for bounded reduction stages with explicit schemas, limits, and no approval-sensitive side effects; keep semantic decisions and final validation direct.
- Evaluation: add or retain harness instructions only when repository checks or representative tasks show they improve final-answer completeness, evidence quality, reliability, latency, or cost. Evaluate the final result, not just tool-call count.
- Project overlay: the remaining sections of this file and the linked project docs define domain-specific architecture, tests, safety boundaries, escalation rules, and publish gates. They may specialize this baseline but must not silently weaken its permission or evidence requirements.
<!-- agent-harness-baseline:end -->

## Workflow: Execution loop

- 표준 사이클은 `구현 + 테스트 -> 검수 + 보완 -> 로컬 change 정리 -> 승인된 경우 푸시` 순서로 진행한다.
- 피처별 구현 범위/검증 명령/완료 조건은 `docs/todo-*/spec.md`의 `C1..Cn` 체크리스트를 단일 기준으로 사용한다.
- 방법론(How)과 피처 스펙(What)의 책임 분리는 `docs/EXECUTION_LOOP.md`를 기준으로 유지한다.
- 사용자가 commit/push까지 명시적으로 요청한 턴은 `scripts/finalize-and-push.sh --message "<type>: <summary>" [--work-id <work-id>]` 경로를 기본으로 사용한다.
- 커밋/원격 동기화/strict 게이트의 허용 경계는 `docs/CHANGE_CONTROL.md`를 따르고, 실제 옵션/default/실행 분기는 대상 `scripts/*`와 CLI help를 직접 확인한다.

## Module-specific guidance

- 공통 하네스 인터페이스와 Rustory overlay의 첫 진입점은 `docs/agent-harness.md`다.
- 저장소 단일 네비 진입점은 `docs/HANDOFF.md`를 사용한다.
- 운영/문서 경계의 단일 기준은 `docs/README_OPERATING_POLICY.md`를 따른다. (운영 제약: `docs/OPERATING_MODEL.md`, 스킬 경계: `docs/SKILL_OPERATING_GUIDE.md`)
- 저장소 메타/진입점/검증 명령 선언의 단일 기준은 `docs/REPO_MANIFEST.yaml`이며, 게이트 실행 방식과 예외 경계는 `docs/CHANGE_CONTROL.md`를 따른다.
- 루트 문서 변경 시 문서 품질 게이트(Last Verified/링크/인덱스/README 정책)는 `docs/CHANGE_CONTROL.md`의 `표준 흐름 > 2. 출고 전 검증`을 단일 기준으로 따른다.
- Rust 기본 검증 기준은 `docs/REPO_MANIFEST.yaml`과 실제 `scripts/*`/`cargo` 명령을 우선 확인하고, 실행 방법론은 `docs/EXECUTION_LOOP.md`, 추가 맥락은 `docs/dev-playbook.md`를 따른다.
- 완료된 `docs/todo-*` 잔존 여부는 `scripts/check-todo-closure.sh`로 점검한다.

## 무컨텍스트 다음 순서

- 문서 탐색이 필요한 작업은 `docs/agent-harness.md` -> `docs/HANDOFF.md` -> `docs/EXECUTION_LOOP.md` -> `docs/CHANGE_CONTROL.md` -> `docs/REPO_MANIFEST.yaml` 순서로 진행한다.
