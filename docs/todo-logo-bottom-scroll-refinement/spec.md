# Spec: logo-bottom-scroll-refinement

## 배경

- 요청 맥락: README 로고의 양피지 밑단을 승인된 참고 이미지처럼 뒤로 말리는 구조로 다듬는다.
- 현재 문제/기회: 기존 밑단의 넓고 완만한 측면 곡선은 종이가 뒤로 말린 형태보다 중간에서 잘린 형태로 보였다.
- 승인된 형상: 긴 앞면 하단선이 바깥쪽의 둥근 말림까지 이어지고, 짧은 뒷면 연결부가 그 뒤에 드러난다.

## 계획 스냅샷

- 목표: 두 양피지의 밑단을 긴 앞면 하단선, 바깥쪽 원형 말림, 짧은 뒷면 연결부로 구성해 승인된 형태와 맞춘다.
- 범위: `docs/assets/rustory-mark.svg`의 하단 말림 형상과 작업 종료 문서만 변경한다.
- 검증 명령: `xmllint --noout docs/assets/rustory-mark.svg`, `scripts/check-readme-policy.sh`, `scripts/check-doc-links.sh`, `scripts/run-manifest-checks.sh --mode quick --work-id logo-bottom-scroll-refinement`.
- 완료 기준: SVG 문법과 README 문서 정책 검사를 통과하고, 실제 렌더링에서 좌우 밑단의 앞면·뒷면·말림 구조가 의도대로 보인다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `xmllint --noout docs/assets/rustory-mark.svg` | 좌우 밑단 형상을 대칭 가능한 SVG 경로로 구현 |
| C2 | done | codex | `scripts/check-readme-policy.sh && scripts/check-doc-links.sh` | 실제 렌더링을 확인하고 README 연동 정책 검증 |
| C3 | in_progress | codex | `scripts/run-manifest-checks.sh --mode full` | todo 종료, 출고 게이트, 원격 CI 확인 |

## 완료/미완료/다음 액션

- 완료: C1, C2.
- 미완료: C3.
- 다음 액션: todo를 종료하고 전체 출고 게이트를 통과한 변경을 원격 `main`에 게시한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-logo-bottom-scroll-refinement`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-logo-bottom-scroll-refinement/open-questions.md`.
