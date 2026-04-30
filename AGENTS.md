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
- 그 외 진행 상황은 간단히 공유하되, 구현/검증/문서화/커밋/푸시를 같은 턴에서 닫는 것을 기본값으로 둔다.

## Workflow: OpenAI GPT-5.5

- OpenAI 모델/API/프롬프트/에이전트 기준을 갱신할 때는 `openai-docs` 스킬과 공식 OpenAI developer docs를 먼저 확인한다.
- 최신 OpenAI 기준을 요구받으면 `gpt-5.5`를 baseline으로 보되, 활성 model string과 직접 연결된 prompt/harness 문구만 좁게 갱신한다.
- GPT-5.5용 지침은 outcome-first로 작성한다. 목표, 성공 기준, 허용되는 부작용, 검증 증거, 중단/에스컬레이션 조건, 출력 형태를 먼저 고정한다.
- reasoning/verbosity/Responses API/tool wiring은 현재 저장소가 안전한 설정 지점을 노출할 때만 바꾼다. API surface나 tool handler 변경이 필요하면 blocker로 기록한다.

## Workflow: Execution loop

- 표준 사이클은 `구현 + 테스트 -> 검수 + 보완 -> 커밋 정리 + 푸시` 순서로 진행한다.
- 피처별 구현 범위/검증 명령/완료 조건은 `docs/todo-*/spec.md`의 `C1..Cn` 체크리스트를 단일 기준으로 사용한다.
- 방법론(How)과 피처 스펙(What)의 책임 분리는 `docs/EXECUTION_LOOP.md`를 기준으로 유지한다.
- 구현 요청 턴은 별도 중단 지시가 없으면 `scripts/finalize-and-push.sh --message "<type>: <summary>" [--work-id <work-id>]` 경로를 기본으로 사용한다.
- 커밋/원격 동기화/strict 게이트 옵션 상세는 `docs/CHANGE_CONTROL.md`를 단일 기준으로 따른다.

## Module-specific guidance

- 저장소 단일 네비 진입점은 `docs/HANDOFF.md`를 사용한다.
- 운영/문서 경계의 단일 기준은 `docs/README_OPERATING_POLICY.md`를 따른다. (운영 제약: `docs/OPERATING_MODEL.md`, 스킬 경계: `docs/SKILL_OPERATING_GUIDE.md`)
- 저장소 메타/진입점/검증 명령의 단일 기준은 `docs/REPO_MANIFEST.yaml`, `docs/CHANGE_CONTROL.md`를 따른다.
- 루트 문서 변경 시 문서 품질 게이트(Last Verified/링크/인덱스/README 정책)는 `docs/CHANGE_CONTROL.md`의 `표준 흐름 > 2. 출고 전 검증`을 단일 기준으로 따른다.
- Rust 기본 검증(`cargo fmt/test/clippy`, 필요 시 `scripts/smoke_p2p_local.sh`) 기준은 `docs/EXECUTION_LOOP.md`, `docs/dev-playbook.md`, `docs/REPO_MANIFEST.yaml`을 단일 기준으로 따른다.
- 완료된 `docs/todo-*` 잔존 여부는 `scripts/check-todo-closure.sh`로 점검한다.

## 무컨텍스트 다음 순서

- 문서 탐색이 필요한 작업은 `docs/HANDOFF.md` -> `docs/EXECUTION_LOOP.md` -> `docs/CHANGE_CONTROL.md` -> `docs/REPO_MANIFEST.yaml` 순서로 진행한다.
