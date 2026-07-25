# Spec: release-v1-0-59

## 배경

- 요청 맥락: command dedupe 강화와 P2P 실패 피어 격리 수정본을 `v1.0.59`로 출고하고 지원 대상에 배포한다.
- 현재 문제/기회: 부분 동기화의 안전성과 배포 패리티를 코드 테스트만으로 닫지 않고 릴리즈 자산과 실제 런타임까지 검증해야 한다.

## 계획 스냅샷

- 목표: 동일 source revision의 `v1.0.59` 자산을 발행하고 지원 대상 런타임을 새 버전으로 수렴시킨다.
- 범위: 버전 메타데이터, 전체 릴리즈 게이트, 공개 경계 검사, 태그·자산 검증, 순차 배포와 최종 상태 검증.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-59`.
- 완료 기준: source revision·tag·release asset의 빌드 정체성이 일치하고 지원 대상의 버전 및 서비스 상태가 검증된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-59` | 전체 코드·문서·스크립트 출고 게이트 수행 |
| C2 | todo | codex | `scripts/check-publication-boundary.py` | 공개 경계와 private inventory 유출 여부 검사 |
| C3 | todo | codex | `scripts/release-version.sh --version v1.0.59 --profile daily-driver --target-ref main --gate none --work-id release-v1-0-59` | 동일 revision의 태그와 macOS/Linux 자산 발행 |
| C4 | todo | codex | `rr --version` | 지원 대상 순차 배포와 서비스 상태 검증 |

## 완료/미완료/다음 액션

- 완료: 없음.
- 미완료: C1, C2, C3, C4.
- 다음 액션: 전체 출고 게이트와 공개 경계 검사를 통과한 source revision을 push한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-release-v1-0-59`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-release-v1-0-59/open-questions.md`, Docker relay acceptance 2종.
