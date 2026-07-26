# Spec: release-v1-0-61

## 배경

- 요청 맥락: 보안 경계 보완을 공개 릴리즈하고 Mac, 운영 노드, tracker/relay 런타임에 배포한다.
- 현재 문제/기회: 수정 source는 원격 `main`에 있지만 배포 대상은 이전 릴리즈를 실행 중이며, client와 server 변경을 같은 revision으로 출고해야 한다.

## 계획 스냅샷

- 목표: `v1.0.61` source, daily-driver asset, container image를 동일 release revision으로 발행하고 실제 런타임의 버전·서비스·동기화 health를 확인한다.
- 범위: Cargo version bump, full release/publication gate, `main` push와 동일 SHA CI, macOS arm64·Linux x86_64 asset, container image, worker 우선 순차 배포, GitOps promotion과 배포 후 health 검증.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-61`, publication boundary `all`, 원격 SHA·CI·tag·asset·image identity 검증, 실제 `rr version`과 tracker·relay·동기화·클러스터 health 확인.
- 완료 기준: 원격 `main`, release tag, asset과 image identity가 일치하고 Mac·운영 노드·tracker·relay가 `1.0.61` revision으로 실행되며 sync와 cluster health에 배포 회귀가 없다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-61` | version bump와 full source 검증 |
| C2 | in_progress | codex | `git ls-remote --heads origin main` | 공개 경계 검사 후 `main` push와 동일 SHA CI 확인 |
| C3 | todo | codex | `gh release view v1.0.61 --repo zrma/rustory --json tagName,isDraft,isPrerelease,targetCommitish` | daily-driver asset과 container image 발행·identity 검증 |
| C4 | todo | codex | `rr version` | Mac canary update와 daemon·sync health 확인 |
| C5 | todo | codex | `rr sync-status --json --with-tracker` | worker 우선 운영 노드 배포와 peer sync 확인 |
| C6 | todo | codex | `kubectl -n rustory get deploy,pod` | GitOps image promotion과 tracker·relay·cluster health 확인 |

## 완료/미완료/다음 액션

- 완료: release 범위와 target identity를 고정하고 version bump의 full source gate를 통과했다.
- 미완료: C2, C3, C4, C5, C6.
- 다음 액션: publication boundary를 통과한 source change를 `main`에 push하고 동일 SHA CI를 확인한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-release-v1-0-61`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-release-v1-0-61/open-questions.md`, `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-61`.
