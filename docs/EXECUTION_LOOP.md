# Execution Loop

- Audience: Rustory 유지보수자, LLM 에이전트
- Owner: Rustory
- Last Verified: 2026-02-19

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

## 표준 사이클

1. 구현 + 테스트
- `spec.md`의 `C1..Cn` 항목을 기준으로 구현한다.
- 권장 시작 경로는 `scripts/start-work.sh --work-id <work-id>`로, todo 초기화(`spec.md`, `open-questions.md`)와 초기 게이트(readiness/open-questions/manifest quick)를 단일 명령으로 실행한다.
- `work-id` 탐색/선정 규칙은 `docs/HANDOFF.md`의 `work-id 규칙`을 단일 기준으로 따르며, 실제 todo 탐색 glob은 `docs/REPO_MANIFEST.yaml`의 `maintenance.todo_workspace_glob`을 사용한다.
- 비긴급 변경은 구현 착수 전에 `scripts/check-todo-readiness.sh docs/todo-<work-id>`를 실행해 `spec/open-questions` 준비 상태를 확인한다.
- `docs/todo-*` 관련 staged 변경(`spec.md`, `open-questions.md`, todo 삭제 증거 포함)은 `lefthook pre-commit`에서 `scripts/check-todo-readiness.sh`, `scripts/check-todo-closure.sh`를 선검증한다.
- readiness 게이트는 계획 스냅샷 필수 필드(`목표`, `범위`, `검증 명령`, `완료 기준`) 존재 + placeholder 금지, 체크포인트 섹션(`완료/미완료/다음 액션` + 검증 증거) 존재, `C1..Cn` 체크리스트(`C1` 시작/연속성/상태값/`Verify command` 유효성)를 함께 검사한다.
- 질문 카드 스키마/닫힘 상태는 `scripts/check-open-questions-schema.sh --require-closed`로 확인해 `open-questions.md` 형식 누락과 미해결 질문 잔존을 함께 차단한다. 닫힘 상태 본문은 정확히 `현재 미결 항목 없음.`이어야 한다.
- 구현 중에는 `scripts/check-todo-closure.sh`로 완료된 `docs/todo-*` 잔존 여부를 점검한다.
- 완료된 작업은 `docs/todo-*`를 삭제하고 정식 문서/`docs/LESSONS_LOG.md`에만 내재화한다. (`docs/archive-*` 루트 폴더 생성 금지)
- `todo` 삭제가 포함된 마감 커밋에서는 `todo-<work-id>` 식별자를 `docs/LESSONS_LOG.md` 또는 `docs/LESSONS_ARCHIVE.md`에 남겨 후속 게이트에서 추적 가능하게 유지한다.
- 구현 중 기본 검증 세트는 `scripts/run-manifest-checks.sh --mode quick --work-id <work-id>`로 실행한다.
- `scripts/run-manifest-checks.sh --mode quick`은 `scripts/check-*` 게이트만 실행하고, manifest의 일반 로컬 검증(예: `cargo fmt/test/clippy`)은 skip될 수 있다. 전체 검증이 필요하면 `--mode full` 또는 `docs/CHANGE_CONTROL.md`의 출고 게이트를 사용한다.
- `scripts/run-manifest-checks.sh`는 `<work-id>`, `<repo-key>` 치환 이후에도 미해결 placeholder(`<...>`)가 남아 있으면 경고 스킵 대신 실패 처리한다. (quick 모드에서 스킵된 비대상 명령은 제외)
- `scripts/run-manifest-checks.sh --mode full`은 `<work-id>` placeholder 체크를 해석할 컨텍스트(work-id 또는 docs/todo-*)가 없으면 실패한다. (quick 모드만 경고 후 skip)
- 운영 게이트 스크립트 변경 시에는 `scripts/check-script-smoke.sh`를 함께 실행해 회귀를 조기 감지한다. (`--work-id`는 다중 todo 환경에서 명시적으로 고정이 필요할 때만 전달)
- 각 항목의 `Verify command`를 우선 실행하고, Rust 기본 검증(`cargo fmt --all --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`)을 통과시킨다.
- 네트워크/P2P 변경에는 `scripts/smoke_p2p_local.sh`를 추가 실행한다.

2. 검수 + 보완
- 피처 규모와 변경 위험도에 맞는 독립 리뷰를 수행하고 교차 검증한다.
- 지적 사항을 반영한 뒤 관련 테스트를 재실행한다.

3. 커밋 정리 + 푸시
- 구현 요청 턴의 완료 정의는 `원격 푸시 + 원격 SHA 검증`까지이며, 기본 경로는 `scripts/finalize-and-push.sh --message "<type>: <summary>" [--work-id <work-id>]`를 사용한다.
- `scripts/finalize-and-push.sh`는 기본적으로 `@` non-empty를 요구한다. 빈 작업트리에서 점검이 필요하면 디버그 환경에서만 `DEBUG_GATES_OVERRIDE=1` + `--allow-empty-at` 조합을 사용한다.
- `jj st`/`jj diff`로 변경 상태를 확인하고 `jj describe -m "<type>: <summary>"`로 메시지를 정리한다.
- strict 게이트/푸시 안전 경로/디버그 우회 옵션/`--work-id` 자동 감지 및 마감 커밋 예외 규칙은 `docs/CHANGE_CONTROL.md`를 단일 기준으로 따른다.
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
