# README Operating Policy

- Audience: Rustory 유지보수자, LLM 에이전트
- Owner: Rustory
- Last Verified: 2026-09-13

## 목적

공개 제품 사용자는 설치와 사용 경로를 먼저 찾고, 기여자와 AI는 필요한 작업 지침으로
이동할 수 있어야 한다. README에 내부 운영 인덱스를 반복하면 제품 안내가 밀리고
같은 규칙을 여러 곳에서 갱신하게 된다.

## 문서 역할

- `README.md`: 제품 개요, 최소 Quick Start, 제품 문서와 개발 시작 링크.
- `docs/README.md`: 제품 사용 문서를 먼저 안내하는 전체 인덱스. 개발·운영 정책도 찾아갈 수 있다.
- `AGENTS.md`: 공통 AI 실행 계약과 Rustory overlay.
- `docs/HANDOFF.md`: 변경·유지보수 요청에 맞는 탐색 시작점.
- `docs/EXECUTION_LOOP.md`: 구현·검증 방법론.
- `docs/CHANGE_CONTROL.md`: 출고·게시의 검증 및 권한 경계.
- `docs/REPO_MANIFEST.yaml`: 진입점과 검증 명령의 선언.

## 편집 원칙

- README는 제품 사용자에게 필요한 설명과 설치 예시를 제공한다. 내부 checklist,
  전체 검증 명령 목록, agent 운영 절차와 과거 완료 기록은 해당 소유 문서로 연결한다.
- 실행 동작·옵션·기본값은 코드, 스크립트와 CLI help가 소유한다. 사용 예시는 이 기준과
  대조하되 구현 상세를 모두 복제하지 않는다.
- 개인정보·grid identity·데이터 보존처럼 사용자의 선택에 영향을 주는 제약은 설명한다.
- 제품 문서 경로는 클릭 가능한 링크로 제공한다. 개발자는 Handoff, AI는 AGENTS에서
  필요한 실행 문서로 이동하므로 README에 별도의 Agent Navigation 목록을 요구하지 않는다.
- 작업별 acceptance는 활성 spec이 소유한다. 완료 시 결정 이유와 한계는 적절한 문서로
  이관하고, 결과는 검증한 범위까지만 기록한다.

## 검증과 변경 소유

README 구조·밀도와 필수 시작 링크는 `scripts/check-readme-policy.sh`로 확인한다.
문서 경로·인덱스·manifest 정합성은 기존 문서 검사를 따른다. 제품 사용 설명의 의미와
내용 이관이 적절한지는 원래 요구사항 및 diff와 대조해 검토한다.

README 역할을 바꿀 때 이 정책과 검사 대상도 함께 맞춘다. 내부 운영 문구를 README에
다시 넣는 방식으로 검사 실패를 해소하지 않는다. 실제 제품·스크립트 검증과 출고 경계는
유지한다. 전체 순서와 예외는 [Change Control](CHANGE_CONTROL.md)을 따른다.

## 관련 문서

- [작업 시작](HANDOFF.md)
- [실행 루프](EXECUTION_LOOP.md)
- [문서 인덱스](README.md)
