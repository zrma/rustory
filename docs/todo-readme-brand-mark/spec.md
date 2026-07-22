# Spec: readme-brand-mark

## 배경

- 요청 맥락: 승인된 두루마리·실타래 시안을 저장소 소유 SVG로 옮기고 README의 공개 랜딩 경험을 보강한다.
- 현재 문제/기회: 현재 README는 제품과 운영 경계를 설명하지만 프로젝트를 즉시 식별할 시각 표식과 짧은 가치 문장이 없다.

## 계획 스냅샷

- 목표: 두 기록이 붉은 실로 연결된 Rustory 브랜드 마크를 재현 가능한 SVG로 추가하고 README 상단에서 제품 성격을 간결하게 전달한다.
- 범위: `docs/assets/rustory-mark.svg`, `README.md`, 작업 todo와 완료 시 필요한 교훈 로그만 변경한다.
- 검증 명령: `scripts/check-readme-policy.sh`, `scripts/check-doc-links.sh`, `scripts/run-manifest-checks.sh --mode quick --work-id readme-brand-mark`.
- 완료 기준: SVG가 독립적으로 렌더링되고 README 링크·밀도 정책과 저장소 quick gate가 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `xmllint --noout docs/assets/rustory-mark.svg` | 승인 시안을 외부 의존성 없는 SVG로 구현 |
| C2 | done | codex | `scripts/check-readme-policy.sh && scripts/check-doc-links.sh` | README 상단에 브랜드 마크와 간결한 가치 문장 추가 |
| C3 | done | codex | `scripts/run-manifest-checks.sh --mode quick --work-id readme-brand-mark` | 저장소 quick gate와 공개 경계 검증 수행 |
| C4 | in_progress | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id readme-brand-mark` | 전체 출고 게이트와 원격 `main` 반영으로 작업 마감 |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3. SVG와 README 상단 브랜드 블록을 구현하고 360px 렌더링 및 저장소 quick gate를 확인했다.
- 미완료: C4. 전체 출고 게이트와 원격 `main` 반영이 남았다.
- 다음 액션: 전체 출고 게이트를 통과한 구현 커밋을 반영한 뒤 완료 todo를 제거해 마감한다.
- 검증 증거: `xmllint --noout docs/assets/rustory-mark.svg`, `scripts/check-readme-policy.sh`, `scripts/check-doc-links.sh`, `scripts/run-manifest-checks.sh --mode quick --work-id readme-brand-mark`, repository publication boundary, machine-local diff boundary guard, 360px PNG 렌더링 확인.
