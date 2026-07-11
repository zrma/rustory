# Spec: managed-daemon-log-cleanup

## 배경

- 요청 맥락: 과거 daemon 오류 반복으로 생성된 대용량 로그를 수작업이 아닌 제품 경로로 정리한다.
- 현재 문제/기회: launchd와 Linux background fallback 로그에 상한이 없어 오류 반복 시 디스크를 계속 소비할 수 있다.

## 계획 스냅샷

- 목표: Rustory 관리 daemon 로그에 안전한 상한과 fleet에서 호출 가능한 정리 명령을 제공한다.
- 범위: macOS launchd 로그, Linux background fallback 로그, daemon 주기 정리, CLI/doctor 자동화, 문서와 테스트.
- 검증 명령: `cargo test managed_logs`, `cargo test logs_cleanup_parses_and_ignores_config_load_errors`, `scripts/check-release-gates.sh --manifest-mode full --work-id managed-daemon-log-cleanup`.
- 완료 기준: 64 MiB 초과 일반 파일만 정리되고 symlink/비관리 로그는 보존되며 실제 macOS 잔여 로그가 `rr logs cleanup`으로 정리된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test managed_logs` | 관리 경로, 상한 정리, symlink 거부 구현 |
| C2 | done | codex | `cargo test logs_cleanup_parses_and_ignores_config_load_errors` | `rr logs cleanup`과 `doctor --auto-fix` 연결 |
| C3 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id managed-daemon-log-cleanup` | 문서, 실제 로그 정리, 전체 회귀 검증 |

## 완료/미완료/다음 액션

- 완료: 관리 경로 자동 정리, CLI/doctor 경로, symlink/hard link/디렉터리/소유권 guard, 문서와 실제 로그 정리.
- 미완료: 없음.
- 다음 액션: 검증된 구현 change를 마감하고 todo workspace를 삭제한다.
- 검증 증거: `cargo test managed_logs` (7 passed), Linux target `cargo check`, 격리 `doctor --auto-fix` smoke, `rr logs cleanup`으로 macOS stderr `7,751,801,990 -> 0 bytes`, `scripts/check-release-gates.sh --manifest-mode full --work-id managed-daemon-log-cleanup` (default 354/core-only 337 tests, clippy, installer, P2P smoke).
