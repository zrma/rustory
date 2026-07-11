# Spec: release-v1-0-52

## 배경

- 요청 맥락: managed daemon log 자동 정리를 재감사한 뒤 `v1.0.52`로 공개 출고하고 Mac 및 k8s 5개 노드에 배포한다.
- 현재 문제/기회: `v1.0.51`은 오류 반복 원인은 고쳤지만 기존·미래 대형 로그를 제품이 자동 정리하는 기능은 아직 fleet에 배포되지 않았다.

## 계획 스냅샷

- 목표: managed log 정리의 활성 FD·파일 안전성·플랫폼 경계를 확인하고 `v1.0.52` 자산과 fleet runtime까지 검증한다.
- 범위: append writer 회귀 보완, package version, public main/tag/release, local Mac canary, `node0..3`와 `sample-node` 순차 배포, cluster health.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-52`, publication boundary `--mode all`, release asset checksum/GLIBC, local/fleet `rr version`·`rr logs cleanup`·sync status, cluster healthcheck.
- 완료 기준: source/CI/release 자산이 green이고 local 및 5개 노드가 `1.0.52`, daemon active, tracker reachable, pending 0이며 관리 로그 정책이 각 플랫폼에서 동작한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test managed_logs --workspace` | managed log cold review와 append FD 회귀 보완 |
| C2 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-52` | `1.0.52` version 및 source 출고 gate |
| C3 | in_progress | codex | `gh release view v1.0.52 --json tagName,targetCommitish,assets` | public main/tag/release 자산과 GLIBC 검증 |
| C4 | todo | codex | `rr version && rr logs cleanup && rr sync-status --json --with-tracker` | local Mac canary 검증 |
| C5 | todo | codex | `ssh <node> 'rr version; rr logs cleanup; rr sync-status --json --with-tracker'` | worker-first 5-node 배포와 cluster health 검증 |

## 완료/미완료/다음 액션

- 완료: v1.0.51 live baseline 수집, managed log cold review, append FD 회귀 포함 8건, `1.0.52` version, publication `mode=all`, full release gate.
- 미완료: C3-C5.
- 다음 액션: main push와 remote CI 확인 후 daily-driver release 자산을 게시한다.
- 검증 증거: full gate default 355/core-only 338 tests, clippy, installer, P2P smoke; repository/local publication boundary `mode=all` passed.
