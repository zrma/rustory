# Spec: doctor-db-status

## 배경

- 요청 맥락: 활성 `docs/todo-*`가 없어 다음 MVP 운영 마일스톤을 선정했다.
- 현재 문제/기회: `rr doctor`는 설정/키/트래커/fzf 상태를 보여주지만 로컬 DB 파일 존재 여부와 저장된 entry 수를 직접 보여주지 않는다. hook/ctrl+r 온보딩 중 DB가 비어 있거나 깨진 상태를 늦게 발견할 수 있다.

## 계획 스냅샷

- 목표: `rr doctor` 텍스트/JSON 출력에 로컬 DB 상태를 추가해 DB 존재 여부, entry 수, peer book/sync peer 수를 즉시 확인할 수 있게 한다.
- 범위: `storage`의 read-only DB inspection, `doctor` 리포트/출력, 관련 테스트, quickstart 문서 보강.
- 검증 명령: `cargo fmt --all --check`; `cargo test doctor_report --workspace`; `cargo test inspect_existing_store --workspace`; `scripts/run-manifest-checks.sh --mode quick --work-id doctor-db-status`.
- 완료 기준: doctor가 DB를 새로 만들지 않고 기존 DB 상태를 보고하며, JSON shape와 문서가 재현 가능한 검증 증거와 함께 갱신된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test inspect_existing_store --workspace` | DB를 생성하지 않는 read-only inspection 경로 추가 |
| C2 | done | codex | `cargo test doctor_report --workspace` | `rr doctor` 텍스트/JSON에 DB status 추가 |
| C3 | in_progress | codex | `scripts/run-manifest-checks.sh --mode quick --work-id doctor-db-status` | quickstart 문서와 todo 체크포인트 갱신 |

## 완료/미완료/다음 액션

- 완료: C1, C2.
- 미완료: C3 마감 커밋 전 quick manifest 및 todo closure.
- 다음 액션: 구현 커밋을 먼저 출고한 뒤 완료 todo를 `docs/LESSONS_LOG.md`에 내재화하고 삭제한다.
- 검증 증거: `cargo fmt --all --check`; `cargo test doctor_report --workspace`; `cargo test inspect_existing_store --workspace`; `cargo test --workspace`; `cargo clippy --workspace --all-targets -- -D warnings`; `scripts/smoke_p2p_local.sh`.
