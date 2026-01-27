# AGENTS.md

## Workflow: docs/todo-*
- 계획된 작업은 `docs/todo-<short-work-id>/` 형식의 폴더로 시작한다.
- 필수 파일은 `spec.md`, `open-questions.md`이다.
- 먼저 spec을 작성/검토하고, 결정 사항은 `spec.md`에 유지한다.
- 미결 항목은 `open-questions.md`에 기록하고 해결 시 제거한다.
- `open-questions.md`가 비고 spec 승인 후에만 구현을 시작한다.
- 범위가 바뀌면 코드 변경 전후로 `spec.md`를 갱신한다.
- 구현 완료 후 관련 내용을 `docs/`(또는 모듈 문서)로 정리하고 todo 폴더는 삭제한다.
