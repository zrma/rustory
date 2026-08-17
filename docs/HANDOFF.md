# Rustory Handoff

- Audience: Rustory 유지보수자, LLM 에이전트
- Owner: Rustory
- Last Verified: 2026-08-17

이 문서는 `rustory` 루트에서 작업 시작 시 사용하는 단일 네비게이션 문서다.

## 무컨텍스트 읽기 순서 (forward-only)

1. `docs/HANDOFF.md` (현재 문서): 어디로 이동할지 결정
2. `docs/EXECUTION_LOOP.md`: 구현/검증 방법론 확인
3. `docs/CHANGE_CONTROL.md`: 출고/푸시 strict 게이트 확인
4. `docs/REPO_MANIFEST.yaml`: 진입점/검증 명령 선언과 동기화 확인

`docs/README.md`는 인덱스 전용 문서이며, 실행 순서/옵션 규칙의 단일 기준 문서는 아니다.

## Source-of-truth 빠른 기준

- 실행 동작/옵션/default/검증 목록은 `src/*`, `scripts/*`, `Cargo.toml`, `docs/REPO_MANIFEST.yaml`, CLI help를 직접 확인한다.
- 문서는 현재 구현을 재서술하지 않고, 어디를 확인할지와 어떤 결정/안전 경계가 있는지만 안내한다.
- 문서 역할 경계가 애매하면 `docs/README_OPERATING_POLICY.md`를 먼저 확인한다.

## 기본 실행 경로 (3단계 + 경계 확인)

- 시작/라우팅: `docs/HANDOFF.md`
- 문서 역할 경계 확인: `docs/README_OPERATING_POLICY.md`
- 구현/검증 방법론: `docs/EXECUTION_LOOP.md` -> `docs/todo-<work-id>/spec.md`
- 출고/푸시 상세 절차: `docs/CHANGE_CONTROL.md`
- 단일 소유 원칙:
  - 이 문서는 "어디로 이동할지"만 안내한다.
  - 실행 방법론과 출고 예외 경계는 `docs/EXECUTION_LOOP.md`, `docs/CHANGE_CONTROL.md`가 안내하고, 실제 명령 옵션/default/분기는 대상 스크립트와 CLI help가 소유한다.

## 최소 시작 루트 (1~2단계)

- 구현/검증 작업: `docs/HANDOFF.md -> docs/EXECUTION_LOOP.md`
- 출고/푸시 점검: `docs/HANDOFF.md -> docs/EXECUTION_LOOP.md -> docs/CHANGE_CONTROL.md`
- 문서 역할 경계가 헷갈릴 때만 `docs/README_OPERATING_POLICY.md`를 추가로 확인한다.

## 실행 전제 (최초 1회)

- 권장: `uv run lefthook install`
- 게이트 스크립트 실행 전제:
  - `python3`(또는 python 3.x 호환) 사용 가능
  - `yaml` 패키지(import `yaml`) 사용 가능
- 빠른 확인: `python3 -c "import yaml; print('ok')"`

## 상황별 시작점 (무컨텍스트 권장)

- 구현 착수/기본 검증: `docs/EXECUTION_LOOP.md`의 표준 사이클을 따른다. (`scripts/start-work.sh --work-id <work-id>` 경로 권장)
- 출고/공유 직전 검증: `docs/CHANGE_CONTROL.md`의 `표준 흐름 > 2. 출고 전 검증`을 따른다.
- 구현 턴 종료(게이트+커밋+푸시+원격 SHA 검증): `docs/CHANGE_CONTROL.md`의 `표준 흐름 > 3. 최종 커밋/푸시`를 따른다.
- 회고/교훈 반영: `docs/IMPROVEMENT_LOOP.md` -> `docs/LESSONS_LOG.md`

## work-id 라우팅 기준

- `todo` 워크스페이스 탐색 glob은 `docs/REPO_MANIFEST.yaml`의 `maintenance.todo_workspace_glob`에서 확인한다.
- 작업 디렉터리 이름은 `docs/todo-<short-work-id>/`를 사용한다.
- `<short-work-id>`는 소문자 kebab-case로 작성한다. 예: `llm-agent-stability-hardening`
- 구현/검증 명령에는 동일한 `<work-id>`를 재사용한다.
- 시작 전에 활성 todo를 먼저 확인해 사람이 읽을 대상과 runner에 전달할 `--work-id`를 분리해서 판단한다.
  - 예시: `find docs -maxdepth 1 -mindepth 1 -type d -name 'todo-*' | sort`
  - 단일 후보가 있어도 `spec.md`를 확인한 뒤 실제 수정/마감 대상은 가능하면 `--work-id`로 명시한다.
  - 후보가 여러 개이면 `spec.md`를 보고 우선순위를 먼저 기록하고, 실제 수정/마감 대상은 `--work-id`로 고정한다.
  - 후보가 없으면 새 작업인지, 완료 todo 삭제 마감인지부터 구분한다.
- `scripts/start-work.sh`, `scripts/check-release-gates.sh`, `scripts/check-push-gates.sh`, `scripts/finalize-and-push.sh`는 위 manifest 값을 공통 탐색 기준으로 사용한다.
- 새 작업 생성은 `scripts/start-work.sh --work-id <work-id>`로 시작하고, 완료 todo 삭제 마감은 `docs/CHANGE_CONTROL.md`의 closed work-id 허용 경계를 확인한다.
- `--work-id` 자동 감지, 마감 커밋 예외, 디버그 우회 옵션의 실제 동작은 각 runner 스크립트와 `--help`를 직접 확인한다.

## 빠른 라우팅

- 에이전트 실행 규칙/가드레일: `AGENTS.md`
- 전체 문서 인덱스(단일 기준): `docs/README.md`
- `docs/README.md`는 인덱스 전용이다. 실행 방법론과 출고 경계는 `docs/EXECUTION_LOOP.md`, `docs/CHANGE_CONTROL.md`를 따르고, 실제 옵션/분기는 대상 스크립트와 CLI help를 직접 확인한다.
- 운영 제약/문서 경계: `docs/OPERATING_MODEL.md`, `docs/MAINTENANCE_PILLARS.md`, `docs/README_OPERATING_POLICY.md`, `docs/SKILL_OPERATING_GUIDE.md`
- 구현/출고/개선 경로: `docs/EXECUTION_LOOP.md`, `docs/CHANGE_CONTROL.md`, `docs/IMPROVEMENT_LOOP.md`, `docs/ESCALATION_POLICY.md`
- 저장소 메타/검증 명령: `docs/REPO_MANIFEST.yaml`
- Rustory 실사용 문서: `docs/quickstart.md`, `docs/distribution.md`, `docs/p2p.md`, `docs/daemon.md`, `docs/hook.md`, `docs/dev-playbook.md`

## 문서 유지 규칙

- 정책/절차/검증 명령이 바뀌면 같은 턴에서 관련 문서와 `Last Verified`를 함께 갱신한다.
- 루트 문서 변경 후 문서 품질 게이트(Last Verified/링크/인덱스/README 정책) 명령 목록과 실행 순서는 `docs/CHANGE_CONTROL.md`의 `표준 흐름 > 2. 출고 전 검증`을 단일 기준으로 따른다.
- 네비게이션 진입점(문서 추가/이동/삭제) 변경 시 `docs/README.md` 인덱스와 `docs/REPO_MANIFEST.yaml` entrypoint를 같은 턴에서 갱신하고 `scripts/check-manifest-entrypoints.sh`를 재실행한다.

## 모듈별 필수 검증

- 구현 착수/기본 게이트/출고 strict 게이트/구현 턴 종료 절차는 `docs/EXECUTION_LOOP.md`, `docs/CHANGE_CONTROL.md`를 단일 기준으로 따른다.
- 검증 명령 선언은 `docs/REPO_MANIFEST.yaml` (`repositories[*].checks`)에서 확인하고, 실제 필터링/치환/실행 순서는 runner 스크립트를 직접 확인한다.
- Rust CLI/P2P 성격의 추가 검증/수용 테스트 기준은 `docs/dev-playbook.md`, `docs/acceptance/README.md`를 따른다.
