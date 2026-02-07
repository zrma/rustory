# AGENTS.md

## Workflow: docs/todo-*
- 계획된 작업은 `docs/todo-<short-work-id>/` 형식의 폴더로 시작한다.
- 필수 파일은 `spec.md`, `open-questions.md`이다.
- 먼저 spec을 작성/검토하고, 결정 사항은 `spec.md`에 유지한다.
- 미결 항목은 `open-questions.md`에 기록하고 해결 시 제거한다.
- `open-questions.md`가 비고 spec 승인 후에만 구현을 시작한다.
- 범위가 바뀌면 코드 변경 전후로 `spec.md`를 갱신한다.
- 구현 완료 후 관련 내용을 `docs/`(또는 모듈 문서)로 정리하고 todo 폴더는 삭제한다.

## Development: TDD (테스트 주도)
- 구현은 가능한 한 “테스트 먼저”를 기본으로 한다.
- 순서:
  1) `spec.md`의 요구사항을 작은 단위(함수/모듈/API)로 쪼갠다.
  2) 해당 단위의 기대 동작을 테스트로 먼저 작성한다.
  3) `cargo test`를 실행해 테스트가 **실패하는 것**을 확인한다.
  4) 최소 구현으로 테스트를 통과시킨다.
  5) 필요하면 리팩터링하고 `cargo test`로 회귀를 막는다.
- 구현 전/후 체크:
  - `cargo test`
  - 포맷/린트가 필요해지면 `cargo fmt`, `cargo clippy`도 같이 맞춘다.

## Development: 비기능/수용 테스트
- 스펙에는 기능뿐 아니라 "운영 성질"(재시작 지속성, 오프라인/부분 장애, 인코딩, 관측성)을 반드시 명시한다.
- unit 테스트만으로 부족한 경우, 최소 1개의 수용 테스트(재시작/스모크/e2e)를 추가한다.
- 반복 개발 교훈/체크리스트는 `docs/dev-playbook.md`를 따른다.
