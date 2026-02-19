# README Operating Policy

- Audience: Rustory 유지보수자, LLM 에이전트
- Owner: Rustory
- Last Verified: 2026-02-19

이 문서는 AI 에이전트 중심 개발 환경에서 `README.md`, `AGENTS.md`, `docs/*`의 역할 경계를 고정한다.

## 목적

- 암묵 컨텍스트를 줄이고, 문서 책임을 분리한다.
- `README` 장문화로 인한 중복/드리프트를 방지한다.
- 에이전트가 안정적으로 진입점을 찾도록 네비를 단순화한다.

## 역할 분리 (단일 기준)

1. `README.md`
- 역할: 저장소 랜딩 페이지(개요 + 최소 부트스트랩 + 핵심 링크)
- 대상: 사람 + 에이전트의 첫 진입
- 포함: 무엇을 하는 저장소인지, 어디서 시작하는지, 필수 링크
- 제외: 상세 절차, 체크리스트, 게이트 규칙, 장문의 운영 런북

2. `AGENTS.md`
- 역할: 실행 규칙/가드레일/검증 절차
- 대상: 에이전트
- 포함: 작업 워크플로, 금지/승인 규칙, 필수 검증
- 제외: 서비스 소개성 문장, 중복 개요

3. `docs/*`
- 역할: 상세 운영 아티팩트 단일 소스
- 대상: 사람 + 에이전트
- 포함: runbook, 체크리스트, 운영 정책, 회고/교훈, 변경 통제
- 제외: `README`에 이미 요약된 인덱스 정보의 중복 본문

## 단일 기준 문서 매핑

- 네비게이션 시작점(어디로 이동할지): `docs/HANDOFF.md`
- 구현/검증 방법론(How): `docs/EXECUTION_LOOP.md`
- 출고/푸시 strict 게이트 + 옵션/예외: `docs/CHANGE_CONTROL.md`
- todo 워크스페이스 탐색 glob: `docs/REPO_MANIFEST.yaml`의 `maintenance.todo_workspace_glob`
- README/AGENTS/docs 역할 경계: 이 문서(`docs/README_OPERATING_POLICY.md`)

## 무컨텍스트 권장 동선 (forward-only)

1. `docs/HANDOFF.md`
2. `docs/EXECUTION_LOOP.md`
3. `docs/CHANGE_CONTROL.md`
4. `docs/REPO_MANIFEST.yaml`

`docs/README_OPERATING_POLICY.md`는 위 동선에서 문서 역할 경계가 헷갈릴 때만 참조하는 보조 정책 문서다.

편집 원칙:

- 규칙을 변경할 때는 소유 문서만 수정하고, 다른 문서는 링크/참조만 갱신한다.
- 동일 옵션/예외 규칙을 여러 문서에 재서술하지 않는다.

## AI-first README 규칙

1. `README`는 "외부 온보딩 문서"가 아니라 "개인 운영 인덱스"로 유지한다.
2. 상세 내용은 링크로 위임하고 본문 복제를 피한다.
3. 변경이 생기면 `README`는 링크/진입점만 갱신한다.
4. 실행 규칙 변경은 `AGENTS.md` 또는 `docs/*`를 갱신하고 `README`에는 요약 링크만 남긴다.

## 권장 구조 (README)

1. 저장소 한 줄 설명
2. Quick Start (필수 최소 명령만)
3. 작업 시작 링크(`docs/HANDOFF.md`)
4. 운영 규칙 링크(`AGENTS.md`, `docs/OPERATING_MODEL.md`)
5. 모듈별 진입 링크(필요 최소)

## 품질 게이트

`README`를 수정할 때 아래를 확인한다.

1. 상세 절차가 `docs/*`와 중복되지 않는가
2. 실행 규칙이 `AGENTS.md`와 중복되지 않는가
3. 핵심 링크가 최신인가(`scripts/check-doc-links.sh`)
4. README 구조/밀도 가드레일을 통과하는가(`scripts/check-readme-policy.sh`)

`scripts/check-readme-policy.sh` 기준의 기본 강제값(환경변수 override 없을 때):

- `README.md` 첫 줄은 레벨-1 제목(`# ...`)이어야 한다.
- 총 라인 수 `<= 220`, H2 개수 `<= 8`, H3 개수 `<= 6`, 코드 펜스 개수 `<= 6`
- 필수 H2: `Quick Start`, `Agent Navigation`, `Product Docs`, `Development`
- 필수 링크: `docs/HANDOFF.md`, `docs/README_OPERATING_POLICY.md`, `docs/OPERATING_MODEL.md`, `docs/EXECUTION_LOOP.md`, `docs/CHANGE_CONTROL.md`, `docs/IMPROVEMENT_LOOP.md`, `docs/ESCALATION_POLICY.md`, `docs/LESSONS_LOG.md`, `docs/REPO_MANIFEST.yaml`, `AGENTS.md`

## 관련 문서

- `AGENTS.md`
- `docs/HANDOFF.md`
- `docs/OPERATING_MODEL.md`
- `docs/EXECUTION_LOOP.md`
- `docs/SKILL_OPERATING_GUIDE.md`
