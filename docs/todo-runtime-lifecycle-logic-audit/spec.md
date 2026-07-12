# Spec: runtime lifecycle logic audit

## 계획 스냅샷

- 목표: updater와 installer의 background daemon 부분 성공이 orphan process, stale PID, 잘못된 restart 성공 보고로 남지 않게 한다.
- 범위: `src/self_update.rs`, `install/rustory.py`, 관련 Rust/Python 회귀 테스트와 운영 교훈.
- 검증 명령: `cargo test self_update --workspace && python3 install/test_rustory.py && scripts/check-release-gates.sh --manifest-mode full --work-id runtime-lifecycle-logic-audit`
- 완료 기준: PID 기록 실패와 즉시 종료가 child 정리 및 PID 제거로 끝나고 systemd-user argv가 정확히 한 번만 `--user`를 포함하며 전체 회귀가 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test self_update --workspace` | Rust background daemon 부분 성공 회귀 보강 |
| C2 | done | codex | `python3 install/test_rustory.py` | installer fallback orphan/stale PID/fd 누수 회귀 보강 |
| C3 | done | codex | `cargo test self_update --workspace` | systemd-user 중복 option 제거 |
| C4 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id runtime-lifecycle-logic-audit` | 전체 회귀와 cold reread 마감 |

## 완료/미완료/다음 액션

- 완료: runtime lifecycle 결함을 보완하고 default/all-feature 422 tests, no-default 405 tests, installer 27 tests(3 skipped), clippy, import 기능별 build, 3-peer P2P smoke와 full release gate를 통과했다.
- 미완료: 없음.
- 다음 액션: 완료 todo를 별도 마감 change에서 삭제하고 strict push gate로 원격 반영을 검증한다.
- 검증 증거: PID 기록 실패 시 child reap, 즉시 종료 시 PID 제거, raw fd 회수, `systemctl --user` 단일 scope를 실패 주입 테스트로 고정했다.
