# Spec: ai-first-v1-release

## 배경

- 요청 맥락: 초기 AI-first 파일럿은 정식 릴리스 전의 `0.1.0-dev` commit pin을 사용했다.
- 현재 문제/기회: 공개된 framework `v1.0.0`을 기준으로 선언·lock·생성 문서·검증 계약을 맞춰야 전체 저장소가 같은 안정 버전을 재현할 수 있다.

## 계획 스냅샷

- 목표: AI-first source를 annotated release `v1.0.0`으로 고정하고 repository overlay는 그대로 보존한다.
- 범위: `.ai-first.toml`, `.ai-first.lock`, 생성된 `AGENTS.md`·`docs/agent-harness.md`, interface checker, 이 work packet만 변경한다.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-v1-release`.
- 완료 기준: release tag·source commit·생성 산출물의 정합성과 Rustory 전체 release gate가 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `scripts/check-agent-harness-interface.sh` | AI-first 선언·lock·생성 산출물을 `v1.0.0` release pin으로 갱신 |
| C2 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-v1-release` | 저장소 전체 release gate 통과 |

## 완료/미완료/다음 액션

- 완료: AI-first `v1.0.0` release pin, 독립 interface 검증, 전체 release gate.
- 미완료: 없음.
- 다음 액션: 검증 증거를 교훈 로그에 남기고 work packet을 닫는다.
- 검증 증거: `scripts/check-agent-harness-interface.sh`, `scripts/check-release-gates.sh --manifest-mode full --work-id ai-first-v1-release`.
