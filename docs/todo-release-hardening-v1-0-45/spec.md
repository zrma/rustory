# Spec: release-hardening-v1-0-45

## 배경

- 요청 맥락: `v1.0.44` Linux asset이 GLIBC 2.39를 요구해 구형 Docker에서 updater 검증에 실패한 긴급 출고 결함을 복구하고 fleet 배포를 안전하게 완료한다.
- 현재 문제/기회: native SSH builder의 GLIBC를 그대로 상속한 asset과, 모든 Linux node에 남은 legacy `rustory-daemon.service` preflight restart loop가 확인됐다. 후자는 `rr uninstall` cleanup 대상에서도 누락돼 있다.

## 계획 스냅샷

- 목표: Linux asset의 최대 GLIBC baseline을 출고 전에 fail-closed로 검증하고, update/uninstall이 legacy systemd user unit을 정리하며, 이를 포함한 `v1.0.45`를 local/5-node fleet에 배포한다.
- 범위: Linux release build gate, self-update/uninstall legacy unit cleanup, 회귀 테스트, package version, GitHub Release, worker-first fleet 배포.
- 검증 명령: `scripts/check-linux-glibc-baseline.sh`, `cargo test uninstall --workspace`, Linux `cargo test self_update --workspace`, full release gates, public asset checksum/GLIBC scan, fleet `rr version`/systemd/sync/cluster health.
- 완료 기준: `v1.0.45` Linux asset의 최대 요구 GLIBC가 2.17 이하이고, local/5-node가 동일 build로 수렴하며 legacy unit 부재, main daemon active, tracker reachable, pending 0, cluster Ready/Healthy다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `readelf --version-info rr-x86_64-unknown-linux-gnu` | 깨진 `v1.0.44` asset을 Zig GLIBC 2.17 build로 긴급 교체하고 public checksum 재검증 |
| C2 | done | codex | fleet `systemctl --user show rustory-daemon.service` | 5개 노드의 legacy preflight restart loop와 uninstall 누락 근거 고정 |
| C3 | done | codex | `scripts/check-script-smoke.sh --work-id release-hardening-v1-0-45` | 빌더 종류와 무관한 최대 GLIBC gate 및 pass/fail fixture 검증 |
| C4 | done | codex | `cargo test uninstall --workspace && cargo test self_update --workspace` | update/uninstall legacy systemd unit cleanup과 회귀 테스트 통과 |
| C5 | todo | codex | full local/remote release gates | `1.0.45` source commit 고정 및 GitHub Actions green 확인 |
| C6 | todo | codex | Zig build + public release download/checksum/GLIBC scan | `v1.0.45` 호환 Linux/macOS asset 발행 |
| C7 | todo | codex | local → node0..node3 → sample-node `rr update` | local/worker/control-plane 순차 배포와 legacy unit 제거 |
| C8 | todo | codex | fleet sync + Kubernetes/ArgoCD health | 최종 fleet/cluster 수렴 확인 및 todo 종료 |

## 완료/미완료/다음 액션

- 완료: C1-C4. `v1.0.44` Linux asset은 최대 `GLIBC_2.17`인 Zig build로 교체했고, fleet-wide legacy systemd restart loop를 확인했다. GLIBC gate와 update/uninstall cleanup을 구현해 macOS/Linux 검증을 통과했다.
- 미완료: C5-C8. `v1.0.45` source 출고, 호환 asset 발행과 fleet 배포가 남았다.
- 다음 액션: full release gate와 원격 Actions로 source commit을 고정한 뒤 Zig daily-driver asset을 발행한다.
- 검증 증거: public `v1.0.44` Linux SHA256 `7cf20aed4a4d4c3e309c1db480ac10241ee3e80c72f938bfeaea928aa4f4303a`; `MAX_GLIBC=GLIBC_2.17`; GLIBC fixture `2.17` pass/실제 `node0 /usr/bin/ls`의 `2.38` reject; macOS uninstall 12/self-update 16 tests; Linux full test 302 passed + Clippy green.
