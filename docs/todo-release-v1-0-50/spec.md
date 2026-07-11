# Spec: release-v1-0-50

## 배경

- 요청 맥락: macOS loopback direct dial timeout warning flood 보완을 즉시 patch release로 출고한다.
- 현재 문제/기회: `v1.0.49` 기능은 정상이나 운영 로그가 비정상적으로 증가할 수 있어 fleet 교체가 필요하다.

## 계획 스냅샷

- 목표: `v1.0.50` 자산을 게시하고 Mac, 내부 5개 노드, 외부 x86 피어에 재배포한다.
- 범위: package version, release source/tag/assets, fleet와 runtime log 검증.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-50`.
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고 검증 명령이 재현 가능하게 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `rg 'version = "1.0.50"' Cargo.toml Cargo.lock` | package와 lock version 일치 |
| C2 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-50` | 전체 출고 게이트 통과 |
| C3 | todo | codex | `gh run list --branch main` | 원격 CI 3종 성공 |
| C4 | todo | codex | `scripts/check-linux-glibc-baseline.sh dist/release-v1.0.50/rr-x86_64-unknown-linux-gnu 2.17` | 공개 자산과 GLIBC 검증 |
| C5 | todo | codex | `rr version && rr sync-status --json --with-tracker` | fleet 및 runtime log 검증 |

## 완료/미완료/다음 액션

- 완료: C1.
- 미완료: C2-C5.
- 다음 액션: 전체 gate와 원격 CI 후 공개 자산 게시 및 순차 배포.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-release-v1-0-50`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-release-v1-0-50/open-questions.md`.
