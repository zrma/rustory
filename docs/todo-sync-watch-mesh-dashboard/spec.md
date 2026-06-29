# Spec: sync-watch-mesh-dashboard

## 배경

- 요청 맥락: `rr sync-status --watch`가 표 중심 화면이라 사용자가 기대한 p2p grid/flow 시각화와 다르다.
- 현재 문제/기회: 현재 로컬 상태 모델로는 전역 peer-to-peer telemetry를 사실처럼 그릴 수 없지만, local-observed mesh map과 traffic/link 패널은 제공할 수 있다.

## 계획 스냅샷

- 목표: `sync-status --watch`를 local-observed mesh dashboard 형태로 바꿔 노드/링크/backlog/traffic 상태를 더 직관적으로 보여준다.
- 범위: `src/cli.rs` watch 렌더링, 관련 테스트, `docs/p2p.md` 설명만 수정한다.
- 검증 명령: `cargo test sync_status --workspace` 및 `scripts/run-manifest-checks.sh --mode full --work-id sync-watch-mesh-dashboard`.
- 완료 기준: watch 프레임이 `Mesh Map`, `Traffic`, `Links` 패널로 렌더링되고 모든 라인이 120컬럼 이하로 유지되며, 검증 명령이 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `cargo test sync_status --workspace` | local-observed mesh dashboard 렌더링 구현 |
| C2 | todo | codex | `scripts/run-manifest-checks.sh --mode full --work-id sync-watch-mesh-dashboard` | 전체 게이트 통과 후 todo 삭제/마감 |

## 완료/미완료/다음 액션

- 완료: C1 구현 초안 작성 및 `cargo test sync_status --workspace` 통과.
- 미완료: C2 전체 게이트와 마감 커밋.
- 다음 액션: 전체 검증 후 todo workspace를 삭제하고 archive token을 남긴다.
- 검증 증거: `cargo test sync_status --workspace`.
