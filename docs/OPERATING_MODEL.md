# Operating Model

- Audience: Rustory 유지보수자, LLM 에이전트
- Owner: Rustory
- Last Verified: 2026-02-19

이 문서는 현재 팀 운영 제약(1인 개발 + LLM 적극 활용)을 작업 규칙으로 고정한 기준 문서다.

## 팀 구조

- Maintainer: 1인
  - Rust CLI/P2P/스토리지/릴리즈 운영 및 문서 기준 소유
- LLM 에이전트
  - 구현/검증/문서화/게이트 실행의 1차 실행 도구

## 협업 원칙

- 구두 합의보다 저장소 문서를 단일 진실 원천으로 사용한다.
- 작업 네비게이션은 `docs/HANDOFF.md -> docs/EXECUTION_LOOP.md -> docs/todo-*/spec.md`를 기본 경로로 사용한다.
- 결정 사항은 `spec.md`에 유지하고, 미결 항목은 `open-questions.md`에만 유지한다.
- 동일 규칙/절차를 여러 문서에 중복 작성하지 않는다.

## 문서 역할 분리 (AI-first)

1. `README.md`는 저장소 인덱스(개요 + 최소 시작 링크)만 유지한다.
2. 실행 규칙/가드레일은 `AGENTS.md`가 소유한다.
3. 상세 절차/체크리스트/런북은 `docs/*`가 소유한다.
4. 역할 분리 상세 기준은 `docs/README_OPERATING_POLICY.md`를 단일 기준으로 따른다.

## 버전관리 원칙 (jj 우선)

1. 로컬 변경 정리는 `jj st`, `jj describe -m`, `jj bookmark`를 기본으로 사용한다.
2. 원격 동기화는 `scripts/jj-git-push-safe.sh` 또는 `jj git push`를 기본으로 사용한다.
3. 루트(`.jj`)에서는 `git commit`을 기본 차단하고, 예외 사용 시 `ALLOW_GIT_COMMIT_IN_JJ_ROOT=1` + 즉시 `jj git import`를 사용한다.
4. 출고/공유 직전에는 `docs/CHANGE_CONTROL.md`의 절차를 기준으로 `scripts/check-release-gates.sh --manifest-mode full`를 수행한다.
5. `.gitmodules`가 없으면 submodule 계열 게이트는 자동 skip 처리한다.

## LLM 운영 규칙

1. 작업 시작 시 최신 문서를 먼저 동기화한다.
2. 구현 전 `spec.md` 체크리스트와 검증 명령을 고정한다.
3. 선택지가 있는 논의는 구현 전에 질문 카드(`description`, `options`, `pros/cons`, `recommended`)로 먼저 고정한다.
4. 결정된 항목은 같은 턴에서 `spec/open-questions/대상 문서`에 즉시 반영한다.
5. 동일 게이트 실패가 2회 반복되면 임의 재시도를 중단하고 `docs/ESCALATION_POLICY.md` 기준으로 사람에게 보고한다.
6. `DEBUG_GATES_OVERRIDE` 계열 우회는 사람 승인 없이 사용하지 않는다.
7. `scripts/check-doc-last-verified.sh` 실패 시 관련 문서 `Last Verified`를 갱신하고 재검증 전까지 출고/푸시를 진행하지 않는다.

## 전역 스킬 운영 원칙

1. 전역 스킬은 `방법론(How)`만 소유하고, 레포별 사실/상태/경로는 `docs/`가 소유한다.
2. 전역 스킬에는 레포 가드(특정 레포명/경로)와 환경 고정값(엔드포인트/계정)을 넣지 않는다.
3. 반복 루프는 `iteration-loop`, 언어/도메인 검증은 `*-project-maintenance`로 분리해 중복을 피한다.
4. 스킬 승격/경계/오염 방지 상세 기준은 `docs/SKILL_OPERATING_GUIDE.md`를 단일 기준으로 따른다.

## 회귀 방지/지속 개선 루프

- 운영 기준 문서: `docs/IMPROVEMENT_LOOP.md`
- 사람 개입 기준 문서: `docs/ESCALATION_POLICY.md`
- 누락/실패/재작업이 발생하면 원인과 방지책을 `docs/LESSONS_LOG.md`에 기록한다.
- 같은 유형의 문제가 2회 이상 반복되면 문서 규칙 또는 스크립트 검증으로 승격한다.
- 로그는 최근 항목만 유지하고, 오래된 항목은 주기적으로 아카이브한다.
