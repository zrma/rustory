# Spec: release-v1-0-54

## 배경

- 요청 맥락: runtime lifecycle 논리 감사와 background daemon 보완을 daily-driver fleet에 출고한다.
- 현재 문제/기회: `main`의 수정은 검증됐지만 공개 `v1.0.53` 자산과 실사용 노드는 이전 revision이다.

## 계획 스냅샷

- 목표: `v1.0.54` daily-driver 자산을 공개하고 local Mac과 `node0..3`, `sample-node`에 순차 배포한다.
- 범위: Cargo version, source/release gate, macOS arm64/Linux x86_64 자산, local canary, 5-node rollout, fleet/cluster 검증과 교훈 마감.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-54`.
- 완료 기준: tag/target/assets가 일치하고 전 노드 `1.0.54`, daemon active, tracker reachable, pending 0, Kubernetes Ready 및 ArgoCD Synced/Healthy다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | in_progress | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-54` | version bump와 source gate 검증 |
| C2 | todo | codex | `python3 ~/.agents/skills/public-repo-boundary-guard/scripts/check_publication_boundary.py . --mode all` | 공개 출고 경계와 release 자산 검증 |
| C3 | todo | codex | `rr version && rr sync-status --json --with-tracker` | local Mac canary 배포 |
| C4 | todo | codex | `ssh ts-<node> '$HOME/.local/bin/rr version'` | worker-first 5-node 배포 |
| C5 | todo | codex | `kubectl get nodes && kubectl get applications.argoproj.io -n argocd` | fleet/cluster 최종 검증과 문서 마감 |

## 완료/미완료/다음 액션

- 완료: `v1.0.53` local/fleet와 사전 cluster health를 확인했다.
- 미완료: C1-C5.
- 다음 액션: version bump diff를 검토하고 full release gate를 실행한다.
- 검증 증거: local/5-node `1.0.53` clean build, Linux service active, non-running Pod 0, ArgoCD 19 Synced/Healthy.
