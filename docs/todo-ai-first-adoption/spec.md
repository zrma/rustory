# Spec: ai-first-adoption

## 배경

- 요청 맥락: public AI-first core를 immutable commit으로 pin하고 Rustory operating
  contract를 repository-owned overlay로 보존한다.
- 현재 문제/기회: 기존 harness는 공통 규칙과 model-specific baseline을 복제하므로
  source provenance와 update drift를 저장소 자체에서 검증하기 어렵다.

## 계획 스냅샷

- 목표: versioned core/profile, overlay, lock와 standalone checker를 canonical gate에
  연결한다.
- 범위: harness/config/overlay/check와 manifest navigation만 변경한다.
- 검증 명령: `scripts/run-manifest-checks.sh --mode quick --work-id ai-first-adoption`,
  `scripts/check.sh --fast`.
- 완료 기준: immutable source/digest, generated interface, native fast gate가 모두
  통과하고 제품/P2P/release behavior가 변경되지 않는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `python3 .ai-first/check.py` | immutable source와 generated drift 검증 |
| C2 | done | codex | `scripts/check-agent-harness-interface.sh` | AI-first interface와 Rustory overlay 보존 |
| C3 | done | codex | `scripts/run-manifest-checks.sh --mode quick --work-id ai-first-adoption` | repository contract 검증 |
| C4 | in_progress | codex | `scripts/check.sh --fast` | immutable adoption change 위에서 native gate 재검증 |

## 완료/미완료/다음 액션

- 완료: C1-C3.
- 미완료: C4 immutable change closeout.
- 다음 액션: adoption change를 고정한 뒤 native fast gate를 재확인하고 완료 todo를
  교훈 로그로 이관해 삭제한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-ai-first-adoption`,
  `scripts/check-open-questions-schema.sh --require-closed docs/todo-ai-first-adoption/open-questions.md`.
