# Spec: post-v1-0-47-defect-review

## 배경

- 요청 맥락: `v1.0.47` 배포 직후 최신 `main` 전체 구현에서 치명적 버그, 논리 결함, 데이터 손실·보안·운영 문제를 꼼꼼히 재검토하고 확인된 결함을 보완한다.
- 현재 문제/기회: 기존 full gate는 green이지만 2.6만 줄 규모의 CLI/P2P/storage/update/uninstall 경로에는 자동 테스트가 포착하지 못하는 교차 상태·입력 경계가 남을 수 있다. 추측성 리팩터링 없이 재현 가능한 결함만 최소 수정한다.

## 계획 스냅샷

- 목표: current `main=85fed3c7`을 기준으로 자동 검증과 수동 invariant 리뷰를 수행하고, 발견된 결함을 회귀 테스트와 함께 닫는다.
- 범위: `src/`, installer/release/maintenance scripts, CLI user-facing behavior, DB/P2P/sync/update/uninstall/hook 경계, 관련 문서와 테스트. 외부 fleet 재배포와 기존 장기 todo 구현은 제외한다.
- 검증 명령: `cargo audit`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/check-release-gates.sh --manifest-mode full --work-id post-v1-0-47-defect-review`.
- 완료 기준: C1-C6가 모두 `done`이고 자동 검사와 수동 리뷰 finding이 재현/수정/회귀 테스트로 닫히며, 남은 위험과 local VCS 상태가 명시된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `jj status && jj bookmark list --all-remotes` | clean current/remote `main`과 active todo/source inventory 고정 |
| C2 | done | codex | `cargo audit && cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings` | dependency advisory와 전체 자동 검증 실행 |
| C3 | done | codex | `rg -n 'unwrap\(|expect\(|unsafe|TODO|FIXME|Command::new|remove_file|remove_dir_all' src install scripts` | 고위험 panic/process/filesystem/auth/input 경계 정적 sweep |
| C4 | done | codex | `cargo test uninstall --workspace && cargo test self_update --workspace && cargo test p2p --workspace && cargo test storage --workspace` | uninstall/update/P2P/storage invariant 수동 리뷰와 focused 검증 |
| C5 | done | codex | `cargo test --workspace` | 확인된 결함 최소 수정 및 회귀 테스트 추가, 없으면 no-op 근거 기록 |
| C6 | in_progress | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id post-v1-0-47-defect-review` | 전체 gate와 최종 diff/VCS metadata 정리 |

## 완료/미완료/다음 액션

- 완료: C1-C5. `main`, `origin/main`, working copy가 `85fed3c7` 기준으로 일치함을 확인하고 22개 Rust source/26,476 lines와 installer/scripts inventory를 고정했다. panic/unsafe/process/filesystem/auth/input 경계를 정적 sweep하고 uninstall의 config-load fail-closed, keep/remove path 충돌, managed-only file/empty-dir 삭제, daemon stop-before-delete 순서를 재검토했다. update의 HTTPS/checksum/download 상한/runtime 실행 검증/atomic rename과 P2P codec·provenance·cursor invariant도 재확인했다.
- 확인 및 보완한 결함: HTTP pull 응답이 `ureq::Error::BodyExceedsLimit`에 걸리면 기존 adaptive batch 축소가 오류를 식별하지 못하고 동기화가 중단됐다. HTTP pull/push 응답 상한을 명시하고 body-limit 오류를 payload-too-large로 분류했으며, 2개 합산 응답은 상한을 넘고 개별 응답은 통과하는 실제 HTTP 회귀 테스트로 `2 -> 1` 축소 후 정상 수신을 확인했다.
- 미완료: C6.
- 다음 액션: full release gate와 최종 diff/VCS metadata 검증을 수행한다.
- 검증 증거: `cargo audit` 취약점 0건(간접 `paste 1.0.15` unmaintained 허용 경고 1건), locked 전체 테스트 309개 통과, clippy `-D warnings` 통과, 신규 HTTP adaptive 회귀 테스트 통과. `cargo outdated --root-deps-only`의 patch 후보는 동작 결함과 분리해 이번 최소 수정 범위에서 제외했다.
