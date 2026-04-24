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

## VCS: 커밋 메시지 컨벤션
- 형식: `<type>: <summary>`
- scope 괄호는 사용하지 않는다. 예: `feat(sync): ...` 형태 금지
- 범위/모듈을 드러내고 싶으면 summary에 포함한다.
  - 예: `feat: sync push cursor persistence`
  - 예: `perf: p2p batch size tuning`
- 커밋(=change) 메시지는 `jj describe -m "<message>"`로 작성/수정한다.

## Security: secrets/PII
- 커밋 메시지/코드/문서에 시크릿(토큰/키/패스워드/쿠키)과 개인정보(이메일/전화번호 등)를 넣지 않는다.
- 로컬 사전 점검(권장): `scripts/secret_scan.sh`
- CI는 PR/push에서 TruffleHog로 secret scan을 수행한다(실제 유출 감지 시 실패).
- 만약 유출이 의심되면 “이미 노출된 것으로 간주”하고 즉시 revoke/rotate 후, 필요하면 히스토리 정리까지 진행한다.

## Execution: 기본 진행 루프(검토 -> 다음 진도)
- 사용자가 `진행해줘`, `추천대로 진행`, `순서대로 진행`, `검토 후 진행` 류의 요청을 하면, 아래 루프를 **묻지 않고** 반복한다.
  1) (필요 시) `docs/todo-*`에 `spec.md`/`open-questions.md` 작성/갱신
  2) `open-questions.md`가 비었는지 확인 후 구현 시작
  3) TDD로 구현 + 최소 1개 수용 테스트(재시작/스모크/e2e) 추가 또는 갱신
  4) 구현 후 자체 검토: 스펙 부합, 버그/논리 오류, 보안 문제, 리팩터링 포인트 점검
  5) 검증: `cargo fmt`, `cargo test`, `cargo clippy --workspace --all-targets -- -D warnings`
     - 보안(권장): `scripts/secret_scan.sh`
  6) (해당 시) 스모크: `scripts/smoke_p2p_local.sh`
  7) `jj describe -m`로 메시지 작성 후 `main` 이동 + `jj git push`
  8) 다음 추천 작업으로 자동 진도(결정이 필요한 경우에만 질문)
- 사용자가 `잠깐`, `여기서 멈춰` 같은 중단 의사를 표현하면 즉시 멈추고 다음 행동을 확인한다.
