# Spec: import-adapter-audit

## 배경

- 요청 맥락: Atuin/Hishtory importer의 논리 오류와 기존 동작 회귀를 출고 전에 재감사한다.
- 현재 문제/기회: 외부 SQLite DB의 live WAL, 극단적 limit, blank source ID 수렴 경계가 일반 fixture만으로는 보장되지 않는다.

## 계획 스냅샷

- 목표: optional importer의 입력 경계를 보강하고 default/no-default/단일-feature 조합의 회귀가 없음을 증명한다.
- 범위: Atuin/Hishtory SQLite importer, 공통 source ID fallback, 관련 테스트와 검증 기록.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id import-adapter-audit`.
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고 검증 명령이 재현 가능하게 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test history_import --workspace` | importer 집중 회귀 테스트 |
| C2 | done | codex | `cargo test --no-default-features --workspace` | optional adapter 제거 상태의 기존 동작 검증 |
| C3 | done | codex | `cargo clippy --no-default-features --workspace --all-targets -- -D warnings` | core-only 정적 검증 |
| C4 | done | codex | `cargo clippy --no-default-features --features import-hishtory --workspace --all-targets -- -D warnings` 및 `cargo clippy --no-default-features --features import-atuin --workspace --all-targets -- -D warnings` | adapter별 독립 빌드 검증 |
| C5 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id import-adapter-audit` | default/no-feature/단일-feature/installer/P2P 전체 회귀 게이트 |

## 완료/미완료/다음 액션

- 완료: checked SQLite limit 변환, Atuin WAL snapshot non-mutation, blank source ID 수렴 및 feature 조합 검증.
- 미완료: 없음.
- 다음 액션: 마감 change에서 todo를 삭제하고 `LESSONS_LOG` 근거를 보존한다.
- 검증 증거: `scripts/check-release-gates.sh --manifest-mode full --work-id import-adapter-audit` 통과.
