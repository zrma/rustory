# Spec: implementation-defect-audit

## 배경

- 요청 맥락: 최신 로컬 revision 기준으로 전체 구현의 치명적 버그, 논리 결함, 보완 필요 지점을 검토하고 확인된 문제를 수정한다.
- 현재 문제/기회: update/uninstall, P2P/HTTP sync, storage, daemon, terminal UI, release 경계는 일상 데이터와 fleet 운영에 직접 영향을 주므로 독립 리뷰와 회귀 검증이 필요하다.

## 계획 스냅샷

- 목표: 최신 로컬 revision의 전체 runtime surface를 위험 기반으로 재검토하고, 재현 가능한 결함을 최소 변경과 회귀 테스트로 닫는다.
- 범위: `src/` 전체와 installer/release/gate script. 신규 제품 기능과 활성 `atuin-import` 구현은 제외한다.
- 검증 명령: focused regression test, `scripts/check.sh --fast`, `scripts/smoke_p2p_local.sh`, `scripts/check-release-gates.sh --manifest-mode full --work-id implementation-defect-audit`.
- 완료 기준: 확인된 결함에 테스트가 추가되고, 전체 local gate가 통과하며 남은 운영/설계 리스크가 구현 버그와 분리되어 기록된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `scripts/check.sh --fast` | 변경 전 310 tests/build 기준선 확보 |
| C2 | done | codex | `jj diff -r main..@ --stat` | update/uninstall, sync/security, CLI/storage/watch 독립 리뷰 |
| C3 | done | codex | `cargo test --workspace` | 확인된 결함 수정 및 재현 테스트 추가 |
| C4 | done | codex | `scripts/smoke_p2p_local.sh` | P2P/sync runtime smoke 재검증 |
| C5 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id implementation-defect-audit` | 전체 local manifest gate 검증 |
| C6 | done | codex | `jj diff && jj status` | 독립 diff 재검수와 local change 정리 |

## 완료/미완료/다음 액션

- 완료: C1-C6. 위험 경계 전수 리뷰, 확인된 결함 수정, 회귀 테스트와 full gate 검증을 완료했다.
- 미완료: 없음. 비정상/악성 binary나 대용량 응답 전제의 방어 심화 후보는 제품 결함과 분리해 후속 후보로 남긴다.
- 다음 액션: 구현 change를 설명한 뒤 별도 child change에서 완료 todo를 `LESSONS_LOG`로 승격하고 삭제한다.
- 검증 증거: `cargo test --workspace` 331 passed, clippy `-D warnings`, installer tests 10 passed/1 Linux-only skip, local 3-peer P2P smoke, Zig Linux x86_64 max `GLIBC_2.17`, `cargo audit` 취약점 0건(`paste` unmaintained 경고 1건), 독립 cold-read no must-fix.
