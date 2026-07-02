# Spec: security-hardening-followup

## 배경

- 요청 맥락: Codex Security scan으로 Rustory의 실사용 배포 경로와 네트워크 경계를 점검한다.
- 현재 문제/기회: release/update download path, tracker register surface, HTTP token handling은 daily-driver 배포에서 직접 노출되는 trust boundary다.

## 계획 스냅샷

- 목표: Codex Security scan에서 발견한 실질 개선점을 코드로 보완하고, 검증/리포트/푸시까지 닫는다.
- 범위: self-update/installer download integrity, tracker register input bounds, bearer token comparison hardening, 관련 테스트와 scan artifact.
- 검증 명령: `cargo fmt --all --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `python3 -m py_compile install/rustory.py`, `cargo audit`, `scripts/run-manifest-checks.sh --mode quick --work-id security-hardening-followup`.
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고 Codex Security final report가 생성된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test --workspace` | release/update download integrity와 tracker hardening 구현 |
| C2 | done | codex | `python3 -m py_compile install/rustory.py` | one-shot installer download integrity guard 구현 |
| C3 | todo | codex | `scripts/run-manifest-checks.sh --mode quick --work-id security-hardening-followup` | repo gate와 Codex Security final report 정리 |

## 완료/미완료/다음 액션

- 완료: C1, C2. `rr update`/installer custom URL은 기본 HTTPS 또는 loopback/pinned/explicit opt-in만 허용하고, tracker register는 addr/meta/peer 수 상한과 constant-time token comparison을 적용했다. `libp2p-dns` 제거로 `hickory-proto` advisory 경로를 끊고 `/dns4`/`/dns6` multiaddr는 dial 전에 IP multiaddr로 정규화한다.
- 미완료: C3. Codex Security final report artifact와 repo gate closeout이 남아 있다.
- 다음 액션: security hardening change를 finalize/push한 뒤, 완료 todo를 `LESSONS_LOG`로 이관하고 삭제한다.
- 검증 증거: `cargo fmt --all --check`, `cargo test --workspace` (245 passed), `cargo clippy --workspace --all-targets -- -D warnings`, `python3 -m py_compile install/rustory.py`, `cargo audit`, `cargo tree -i hickory-proto` (package ID not found).
