# Spec: systemd user bus fallback 중복 daemon 방지

## 배경

- `rr update`가 systemd user unit을 발견했지만 SSH 세션에 user bus가 없으면 background fallback을 강제로 시작했다.
- 구형 daemon에는 `RUSTORY_DAEMON_MANAGER=background` marker가 없으므로 PID 파일 검증에서 소유권을 거부한 뒤에도 새 daemon이 기동되어 동일 사용자의 `rr daemon`이 둘이 되었다.
- 실제 외부 Linux 3대에서 중복을 확인했고, 경로·argv·UID를 검증한 뒤 구형 process만 종료하여 각 1개로 복구했다.

## 계획 스냅샷

- 목표: self-update는 user bus 부재만을 근거로 새 background daemon을 만들지 않는다.
- 범위: Linux post-update daemon 재시작 정책, 회귀 테스트, 교훈 로그, patch release와 fleet 재배포.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id systemd-user-bus-fallback`.
- 호환성: installer가 최초 설치 시 background daemon을 시작하는 동작과 유효한 private PID 증거가 있는 기존 background daemon 재시작은 유지한다.
- 완료 기준: marker 없는 실행 중 PID는 건드리지 않고 새 daemon도 만들지 않으며, marker-managed daemon은 업데이트 뒤 정확히 1개로 복구된다.
- migration 기준: private PID가 가리키는 marker-managed daemon이 실제 실행 중일 때만 동일 사용자·동일 설치 바이너리·관리형 argv·기본 DB와 일치하는 marker 없는 중복 process를 정리한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test self_update::tests::background_restart_requires_private_pid_evidence` | 강제 background 시작 경로 제거 및 정책 회귀 테스트 |
| C2 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id systemd-user-bus-fallback` | 전체 source/release gate 통과 |
| C3 | todo | codex | `rr version && rr doctor && rr sync-status` | v1.0.55 local/fleet 배포와 단일 daemon 검증 |
| C4 | done | codex | `cargo test self_update::tests::legacy_background_migration_requires_owned_manager_and_no_marker` | 구형 orphan daemon child의 제한된 자동 migration 정리 |

## 완료/미완료/다음 액션

- 완료: 재현, 강제 background 시작 경로 제거, 정책 테스트와 full release gate, v1.0.55 fleet 배포에서 legacy orphan child 재현, 소유권 anchor 기반 migration과 Linux Zig build 검증.
- 미완료: C3.
- 다음 액션: legacy orphan migration을 보완한 v1.0.56을 출고하고 실제 3개 background 노드에서 자동 정리를 검증한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-systemd-user-bus-fallback`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-systemd-user-bus-fallback/open-questions.md`.
