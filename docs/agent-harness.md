# Agent Harness

## Interface

- Structure ID: `agent-harness-v1`.
- Baseline ID: `openai-gpt-5.6-2026-07-10`.
- Convergence stage: `bridge`.
- Target stage: `canonical`.
- Canonical check: `scripts/check-agent-harness-interface.sh`.

`AGENTS.md`가 공통 GPT-5.6 계약을 소유하고, 이 문서는 Rustory overlay와 기존 운영 문서로 가는 canonical 진입점이다.

## Project Objective

Rustory를 local-first 셸 히스토리 daily driver로 유지한다. 기록은 각 디바이스의 SQLite에 먼저 남고, tracker와 relay를 통한 P2P grid 동기화가 서로 다른 NAT 환경에서도 검증 가능해야 한다.

## Source Of Truth

- 실행 동작과 옵션: `src/`, `Cargo.toml`, `scripts/`, `rr --help`.
- 진입점과 검증 선언: `docs/REPO_MANIFEST.yaml`.
- 현재 작업 계약: 활성 `docs/todo-*/spec.md`와 `open-questions.md`.
- 탐색 순서: `docs/HANDOFF.md`; 문서 역할 경계: `docs/README_OPERATING_POLICY.md`.

## Autonomy And Permissions

- 목표와 검증 경로가 명확한 로컬·가역 작업은 추가 승인 없이 구현, 검증, 문서화, local change 정리까지 진행한다.
- 외부 write, secret, 비용, 파괴적 작업, 제품 방향 변경, 승인되지 않은 원격 변경은 에스컬레이션한다.
- tracker token, swarm key, identity key와 실제 private endpoint는 저장소에 기록하지 않는다.

## Execution Loop

1. `jj status`와 활성 todo를 확인한다.
2. `docs/HANDOFF.md`에서 task-relevant SSOT만 연다.
3. 비사소한 작업은 `scripts/start-work.sh --work-id <work-id>`로 범위와 acceptance evidence를 고정한다.
4. 가장 작은 논리 단위로 구현하고 focused check를 즉시 실행한다.
5. 실패는 같은 루프에서 수정하고 재검증한다.
6. durable knowledge만 관련 문서와 lessons loop에 반영한다.
7. 하나의 목적을 가진 `jj` change로 닫고, 다음 작업은 새 empty change에서 시작한다.

## Verification And Evidence

- Harness interface: `scripts/check-agent-harness-interface.sh`.
- 빠른 기본 게이트: `scripts/check.sh --fast`.
- 문서 변경: `docs/CHANGE_CONTROL.md`가 지정한 Last Verified, 링크, 인덱스, README, manifest 게이트.
- P2P/배포/릴리즈 변경: 해당 todo acceptance와 `scripts/check-release-gates.sh`; direct-only 성공을 relay readiness로 간주하지 않는다.
- 최종 증거에는 실행한 명령, 결과, 남은 리스크, local/remote bookmark 상태를 포함한다.

## Escalation

`docs/ESCALATION_POLICY.md`를 단일 기준으로 사용한다. 제품 결정, 실제 secret/계정, 비용·운영 리스크, 파괴적 변경, published history rewrite, 승인되지 않은 push가 필요할 때만 사용자에게 최소 판단을 요청한다.

## VCS And Publish

- 로컬 VCS는 `jj`를 사용하고 change description은 `<type>: <summary>`와 Codex trailer 규칙을 따른다.
- 기존 사용자 변경을 보존하고 harness 작업을 별도 change로 유지한다.
- 검증된 마일스톤만 로컬 `main`으로 전진시킨다.
- push 권한이 주어진 경우 `docs/CHANGE_CONTROL.md`와 `scripts/finalize-and-push.sh`를 따르고, 원격 commit과 CI를 확인한다.

## Harness Evaluation And Improvement

대표 작업에서 완료성, evidence 품질, 회귀율, 지연, 비용을 평가한다. 반복 실패는 `docs/IMPROVEMENT_LOOP.md`에 따라 문서, 스크립트, 테스트 또는 `docs/LESSONS_LOG.md`로 기계화하고 단순한 대화 기억에 의존하지 않는다.

## Convergence

- `bridge`: 이 문서가 공통 인터페이스를 제공하고 기존 상세 문서를 연결한다.
- `normalized`: 중복된 autonomy, execution, verification, escalation, VCS 정책을 이 문서의 동일 섹션으로 이동한다.
- `canonical`: 프로젝트별 차이는 `Project Overlay`에만 두고 기존 정책 문서는 domain runbook 또는 호환 링크로 축소한다.
- 단계 전환은 현재 저장소의 Structure ID, 섹션 순서, canonical check 결과로 검증하며 다른 저장소의 이름·개수·로컬 경로·공개 여부를 전제하지 않는다.

## Project Overlay

- local-first write, shared grid identity, per-device identity 경계를 보존한다.
- relay circuit을 포함한 실제 multi-peer evidence 없이 P2P readiness를 완료로 표시하지 않는다.
- 코드/CLI/manifest가 소유하는 동작을 문서에 중복 복사하지 않는다.

## Related Documents

- Navigation: `docs/HANDOFF.md`, `docs/README.md`.
- Execution and change control: `docs/EXECUTION_LOOP.md`, `docs/CHANGE_CONTROL.md`.
- Operating and escalation: `docs/OPERATING_MODEL.md`, `docs/ESCALATION_POLICY.md`.
- Improvement: `docs/IMPROVEMENT_LOOP.md`, `docs/LESSONS_LOG.md`.
