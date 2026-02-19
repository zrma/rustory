# Skill Operating Guide

- Audience: Rustory 유지보수자, LLM 에이전트
- Owner: Rustory
- Last Verified: 2026-02-19

이 문서는 전역 스킬과 레포 문서의 책임 경계를 분리해, 전역 오염 없이 재사용 가능한 스킬만 유지하기 위한 기준이다.

## 핵심 원칙

1. 전역 스킬은 `방법론(How)`만 소유한다.
2. 레포 문서는 `사실/현황(What)`만 소유한다.
3. 전역 스킬에는 레포 가드(특정 레포명/경로)와 환경 고정값(엔드포인트/계정/리소스명)을 넣지 않는다.
4. 특정 레포에서만 유효한 규칙은 `docs/` 트리에서 관리한다.

## 무엇을 스킬로 승격할지

아래 조건을 모두 만족하면 전역 스킬 후보로 본다.

1. 2개 이상 프로젝트/상황에서 반복 사용된다.
2. 특정 경로/도메인 URL 없이 설명 가능하다.
3. 어댑터 입력(검증 명령, 문서 타겟, VCS 정책)으로 분리 가능하다.
4. 성공/실패 출력 계약을 정의할 수 있다.

하나라도 불만족이면 레포 문서에 남긴다.

## 전역 스킬 계층

1. 루프 계층: `iteration-loop`
- 진행 신호 해석, 계획-구현-검증-문서화-정리 반복

2. 도메인 유지보수 계층: `rust-project-maintenance` (필요 시 `python/go/terraform-project-maintenance` 병행)
- 언어/도메인별 검증 세트와 실패 보고 규약

3. VCS 계층: `vcs-jj`
- `jj` 우선 버전관리 규칙

4. 환경 계층(선택): `microk8s-cluster-ops`
- 특정 레포가 아닌 운영 환경(MicroK8s) 기준 런북

## 어댑터 패턴

전역 스킬은 아래 값을 프로젝트에서 주입받아 동작한다.

- `SNAPSHOT_CMDS`
- `VERIFY_CMDS`
- `DOC_TARGETS`
- `VCS_RULES`
- `SHIP_GATES`

값이 비어 있으면 프로젝트 handoff 문서를 우선 조회하고, 추정으로 채우지 않는다.

## 레포 문서가 소유하는 범위

다음 항목은 전역 스킬이 아니라 이 저장소 문서가 소유한다.

1. 네비게이션 트리 (`docs/HANDOFF.md`, `docs/REPO_MANIFEST.yaml`)
2. 실제 경로/검증 명령 (`scripts/*`, `cargo ...`)
3. 현재 배포 상태, 게이트 상태, 이행 TODO
4. 실사용 계약과 운영 런북

## 동기화/오염 방지

`codex-gist-sync` 사용 시 전역 오염 방지를 위해 필터를 기본 적용한다.

1. `CODEX_GIST_REQUIRE_PREFIX`로 팀/용도 prefix 제한
2. `CODEX_GIST_INCLUDE_SKILLS`로 허용 목록 제한
3. `CODEX_GIST_EXCLUDE_SKILLS`로 레거시/실험 스킬 제외
4. 필요 시 `CODEX_GIST_SYNC_AGENTS=0`으로 로컬 AGENTS 분리

## 품질 게이트

스킬 변경 시 아래를 최소 확인한다.

1. `SKILL.md` frontmatter 유효성(`---`, `name`, `description`)
2. 루프/도메인 책임 분리(중복 금지)
3. 레포 경로/엔드포인트 하드코딩 부재
4. 스킬 인덱스/문서 최신화

## 참고

- OpenAI Harness Engineering: <https://openai.com/index/harness-engineering/>
- Anthropic Skills Guide: <https://resources.anthropic.com/hubfs/The-Complete-Guide-to-Building-Skill-for-Claude.pdf?hsLang=en>
