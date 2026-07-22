# Spec: readme-tagline-copy

## 배경

- 요청 맥락: README 로고 아래 문구를 운율과 의미 대구가 살아 있는 문장으로 갱신한다.
- 현재 문제/기회: 기존 문구는 `기록`과 `히스토리`가 의미상 겹쳐 local-first 저장과 P2P 연결의 대비가 약하다.

## 계획 스냅샷

- 목표: README 태그라인을 `로컬에 남기고, P2P로 잇는다.`로 교체한다.
- 범위: `README.md`, 작업 todo, 완료 시 필요한 교훈 로그만 변경한다.
- 검증 명령: `scripts/check-readme-policy.sh`, `scripts/check-doc-links.sh`, `scripts/run-manifest-checks.sh --mode quick --work-id readme-tagline-copy`.
- 완료 기준: 태그라인이 정확히 반영되고 README·문서·공개 경계 검사와 전체 출고 게이트가 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `rg -n '로컬에 남기고, P2P로 잇는다\.' README.md` | 승인된 태그라인을 README에 반영 |
| C2 | done | codex | `scripts/check-readme-policy.sh && scripts/check-doc-links.sh` | README 정책과 문서 링크 검증 |
| C3 | in_progress | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id readme-tagline-copy` | 전체 출고 게이트와 원격 `main` 반영으로 작업 마감 |

## 완료/미완료/다음 액션

- 완료: C1, C2. 승인된 태그라인과 집중 README 검증을 반영했다.
- 미완료: C3. 전체 출고 게이트와 원격 `main` 반영이 남았다.
- 다음 액션: 구현 커밋을 전체 게이트로 검증해 푸시한 뒤 todo를 닫는다.
- 검증 증거: `rg -n '로컬에 남기고, P2P로 잇는다\.' README.md`, `scripts/check-readme-policy.sh`, `scripts/check-doc-links.sh`, `scripts/run-manifest-checks.sh --mode quick --work-id readme-tagline-copy`.
