# Spec: dedupe-candidate-semantics

## 배경

- 요청 맥락: 실사용 히스토리에서 같은 command가 날짜와 실행 맥락 차이 때문에 예상보다 많이 남는다.
- 현재 문제/기회: 기존 `--older-than-days`는 삭제 후보뿐 아니라 keeper 선정 범위도 줄여 최신 row와 오래된 keeper가 함께 남을 수 있고, 보수적인 context 기준이 기본이라 별도 옵션 없이는 정리 효과가 작다.

## 계획 스냅샷

- 목표: exact command dedupe를 단순한 기본 동작으로 만들고 age·push 안전조건은 삭제 후보에만 적용한다.
- 범위: dedupe SQL 후보 선정, CLI 기본값·help, 회귀 테스트, quickstart를 변경한다. command 정규화나 셸 의미 추론은 하지 않는다.
- 검증 명령: `cargo test dedupe`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/check.sh --fast`, `scripts/run-manifest-checks.sh --mode quick --work-id dedupe-candidate-semantics`.
- 완료 기준: 최신 keeper가 age/push 경계 밖에 있어도 삭제 가능한 오래된 exact duplicate를 모두 찾고, 기본 command 및 opt-in context 동작이 테스트와 문서에서 일치한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test dedupe` | age·push 경계를 삭제 후보에만 적용하는 SQL과 회귀 테스트 구현 |
| C2 | done | codex | `cargo test cli::tests::dedupe` | exact command 기본값과 CLI help 검증 |
| C3 | done | codex | `scripts/check.sh --fast` | 문서 및 저장소 quick gate 검증 |

## 완료/미완료/다음 액션

- 완료: exact command 기본값, age·push candidate-only 의미, CLI·storage 회귀 테스트, quickstart 반영.
- 미완료: 없음.
- 다음 액션: 구현 change를 고정한 뒤 완료된 todo workspace를 제거한다.
- 검증 증거: `cargo test dedupe`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/check.sh --fast`, `scripts/run-manifest-checks.sh --mode full --work-id dedupe-candidate-semantics`.
