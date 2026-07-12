# Spec: post device retirement audit

## 계획 스냅샷

- 목표: device retirement 출고 전 독립 전수 검토에서 확인된 삭제 경로, authorization, updater, background daemon, release asset 경계를 보완한다.
- 범위: retirement receipt와 tracker binding, self-update/install background lifecycle, managed logs, staged release asset 검증, 관련 문서와 회귀 테스트.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id post-device-retirement-audit`
- 완료 기준: 모든 finding이 회귀 테스트로 닫히고 full gate, 공개 경계 검사, security diff scan, 원격 CI가 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test device_retirement --workspace` | 실제 파생 uninstall 경로를 acceptance receipt에 고정 |
| C2 | done | codex | `cargo test tracker --workspace` | logical device revoke와 canonical binding 일관화 |
| C3 | done | codex | `cargo test self_update::tests --workspace` | update 임시 파일과 background process 정리 보강 |
| C4 | done | codex | `python3 install/test_rustory.py` | installer background process 회귀 보강 |
| C5 | done | codex | `scripts/check-script-smoke.sh --work-id post-device-retirement-audit` | staged release asset identity/arch/GLIBC 재검증 |
| C6 | doing | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id post-device-retirement-audit` | 전체 회귀와 보안 diff scan 후 출고 마감 |

## 완료/미완료/다음 액션

- 완료: C1-C5 구현과 focused 검증.
- 미완료: C6 전체 게이트, security diff scan, 커밋·푸시·원격 CI 확인.
- 다음 액션: 통합 diff를 논리 커밋으로 분리한 뒤 전체 검증과 보안 스캔을 실행한다.
- 검증 증거: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/check-release-gates.sh --manifest-mode full --work-id post-device-retirement-audit`.
