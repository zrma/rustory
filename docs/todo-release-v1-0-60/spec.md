# Spec: release-v1-0-60

## 배경

- 요청 맥락: dedupe 후보 의미와 기본 exact-command 그룹 개선을 공개 릴리즈하고 Mac 및 운영 노드에 배포한다.
- 현재 문제/기회: 로컬 구현은 검증됐지만 source push, 동일 SHA CI, release asset, 실제 런타임 배포와 동기화 health는 아직 출고 증거로 닫히지 않았다.

## 계획 스냅샷

- 목표: `v1.0.60` source와 daily-driver asset을 동일 release revision으로 발행하고 Mac 및 운영 노드에서 실제 버전·서비스·동기화 health를 확인한다.
- 범위: Cargo version bump, full release/publication gate, `main` push, 동일 SHA CI, macOS arm64·Linux x86_64 asset, worker 우선 순차 배포와 배포 후 health 검증.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-60`, publication boundary `all`, 원격 SHA·CI·tag·asset checksum 검증, 로컬/원격 `rr version`과 동기화·서비스·클러스터 health 확인.
- 완료 기준: 원격 `main`과 release tag/asset identity가 일치하고 Mac 및 운영 노드가 모두 `1.0.60`으로 실행되며 sync와 cluster health에 배포 회귀가 없다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-60` | source 변경과 full gate 검증 |
| C2 | in_progress | codex | `git ls-remote --heads origin main` | 공개 경계 검사 후 `main` push와 동일 SHA CI 확인 |
| C3 | todo | codex | `gh release view v1.0.60 --repo zrma/rustory --json tagName,isDraft,isPrerelease,targetCommitish` | daily-driver release와 asset checksum·build identity 검증 |
| C4 | todo | codex | `rr version` | Mac canary update와 daemon·sync health 확인 |
| C5 | todo | codex | `rr sync-status --json --with-tracker` | worker 우선 운영 노드 배포와 cluster health 확인 |

## 완료/미완료/다음 액션

- 완료: C1.
- 미완료: C2, C3, C4, C5.
- 다음 액션: publication boundary를 실행하고 `main` push 및 동일 SHA CI를 확인한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-release-v1-0-60`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-release-v1-0-60/open-questions.md`, `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-60`.
