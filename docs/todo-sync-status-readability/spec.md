# Spec: sync-status-readability

## 배경

- 요청 맥락: `rr mesh --watch`와 `rr sync-status --watch`가 sequence cursor를 row 수처럼 표시해 실제 저장량과 동기화 위치를 혼동시킨다.
- 현재 문제/기회: `head 6.5M rows`, `direct`, `sent`는 `AUTOINCREMENT` gap이 큰 DB에서 650만 row가 저장된 것처럼 보인다. JSON 하위 호환성을 유지하면서 cursor, 저장 row, pending의 단위를 명시해야 한다.

## 계획 스냅샷

- 목표: status text/JSON/watch UI에서 sequence cursor와 실제 저장 row 수를 구분하고, peer 방향과 pending 단위를 좁은 터미널에서도 읽기 쉽게 표시한다.
- 범위: `LocalStore` row count 조회, `SyncStatusReport` additive JSON 필드, text 출력, sync/mesh watch 레이블·범례, 관련 문서·회귀 테스트와 patch release 출고.
- 검증 명령: `cargo test sync_status --workspace && cargo test watch_tui --workspace && scripts/check-release-gates.sh --manifest-mode full --work-id sync-status-readability`.
- 완료 기준: 기존 `local_head` JSON 필드가 유지되고 `local_row_count`가 추가되며, 모든 사람용 출력에서 cursor를 row count로 부르지 않고 full gate·공개 자산·local/5-node 배포 검증이 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test sync_status --workspace` | 실제 저장 row 수를 report에 추가하고 text/JSON 하위 호환성을 검증 |
| C2 | done | codex | `cargo test watch_tui --workspace` | sync/mesh watch의 cursor·row·pending 레이블과 좁은 폭 렌더링 개선 |
| C3 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id sync-status-readability` | 전체 구현 cold review와 full release gate 수행 |
| C4 | todo | codex | `gh release view v1.0.53 --json tagName,targetCommitish,isDraft,isPrerelease` | patch release 자산과 공개 metadata 검증 |
| C5 | todo | codex | `$HOME/.local/bin/rr version && $HOME/.local/bin/rr sync-status --json --with-tracker` | 로컬 Mac canary 및 k8s 5개 노드 순차 배포 검증 |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3. `local_row_count` additive JSON 필드, text 출력, sync/mesh watch cursor·row 레이블과 실제 650만 cursor/26만 row DB 렌더를 확인했고 full gate를 통과했다.
- 미완료: C4, C5.
- 다음 액션: source commit을 push하고 `v1.0.53` 공개 자산·local/fleet 배포를 검증한다.
- 검증 증거: `cargo test --workspace` 356 passed, `cargo test --no-default-features --workspace` 339 passed, Clippy `-D warnings`, installer 10 passed/1 skipped, `scripts/smoke_p2p_local.sh`, `scripts/check-release-gates.sh --manifest-mode full --work-id sync-status-readability`.
