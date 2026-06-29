# Spec: hishtory-invalid-utf8-import

## 배경

- 요청 맥락: private-node-01 신규 설치에서 `--import-hishtory`가 Hishtory SQLite row의 invalid UTF-8 TEXT 때문에 중단됐다.
- 현재 문제/기회: Hishtory는 과거 shell/locale 상태에 따라 UTF-8이 아닌 byte를 DB TEXT에 보관할 수 있으므로, Rustory import는 단일 손상 row 때문에 전체 이관을 실패시키면 안 된다.

## 계획 스냅샷

- 목표: Hishtory SQLite import가 invalid UTF-8 TEXT 값을 lossily decode해 계속 진행하고, one-line installer 실패도 traceback 없이 진단 가능하게 만든다.
- 범위: Hishtory import row decoding, regression test, installer import failure message, migration/distribution 문서, patch version bump.
- 검증 명령: `cargo test import_hishtory_sqlite_decodes_invalid_utf8_lossily --workspace`, `cargo fmt --all --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `python3 -m py_compile install/rustory.py`, `scripts/check-release-gates.sh --manifest-mode full --work-id hishtory-invalid-utf8-import`.
- 완료 기준: private-node-01와 같은 invalid UTF-8 Hishtory DB row가 import를 중단하지 않고, latest installer로 재시도 가능한 patch release가 출고된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test import_hishtory_sqlite_decodes_invalid_utf8_lossily --workspace` | invalid UTF-8 Hishtory SQLite TEXT row 회귀 테스트 추가 |
| C2 | done | codex | `cargo test --workspace` | Hishtory row decoding을 lossy UTF-8 변환으로 보완하고 기존 import 동작 유지 |
| C3 | done | codex | `python3 -m py_compile install/rustory.py` | installer의 Hishtory import 실패 메시지를 traceback 없이 진단 가능하게 정리 |
| C4 | in_progress | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id hishtory-invalid-utf8-import` | 문서, 버전, release gate를 통과하고 출고 준비 |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3.
- 미완료: C4 closeout.
- 다음 액션: 구현 커밋을 푸시한 뒤 lessons log 기준으로 todo workspace를 별도 closeout 커밋에서 삭제한다.
- 검증 증거: `cargo test import_hishtory_sqlite_decodes_invalid_utf8_lossily --workspace` 통과, `python3 -m py_compile install/rustory.py` 통과, `cargo fmt --all --check` 통과, `cargo test --workspace` 198 passed, `cargo clippy --workspace --all-targets -- -D warnings` 통과, `scripts/check-release-gates.sh --manifest-mode full --work-id hishtory-invalid-utf8-import` 통과.
