# Spec: k8s-tracker-image

## 배경

- 요청 맥락: Rustory tracker를 특정 로컬 hostname alias가 아니라 외부 운영 환경의 고정 endpoint 뒤에서 운영하려면 배포 가능한 container image가 필요하다.
- 현재 문제/기회: 저장소에는 Docker acceptance 전용 image만 있고, production tracker pod에서 재사용할 root `Dockerfile`과 build revision 고정 경로가 없다.

## 계획 스냅샷

- 목표: `rr tracker-serve`를 기본 entrypoint로 실행할 수 있는 production container image definition을 추가한다.
- 범위: root `Dockerfile`, `.dockerignore`, 이 작업 추적 문서와 lessons closeout만 포함한다. registry, hostname, secret store, GitOps manifest 같은 운영 환경별 배포 정보는 소비자 환경의 private infra repo가 소유한다.
- 검증 명령: `scripts/run-manifest-checks.sh --mode quick --work-id k8s-tracker-image`, `docker build --platform linux/arm64 --build-arg RUSTORY_BUILD_REVISION=<rev> -t rustory:<tag> .`
- 완료 기준: image가 `rr tracker-serve --bind 0.0.0.0:8850` 기본 실행 경로를 가지며, build arg로 `rr version` revision/dirty 상태를 재현 가능하게 고정할 수 있다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `scripts/run-manifest-checks.sh --mode quick --work-id k8s-tracker-image` | production container image definition과 Docker build context hygiene 추가 |
| C2 | in_progress | codex | `docker build --platform linux/arm64 --build-arg RUSTORY_BUILD_REVISION=<rev> -t rustory:<tag> .` | platform-compatible tracker image build 가능성 검증 |

## 완료/미완료/다음 액션

- 완료: C1.
- 미완료: C2.
- 다음 액션: Docker build/push 검증 후 private infra repo의 GitOps tracker deployment로 이어간다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-k8s-tracker-image`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-k8s-tracker-image/open-questions.md`.
