# Spec: installer-self-update

## 배경

- 요청 맥락: Rustory를 daily-driver로 쓰기 위해 source build/copy 중심 배포를 줄이고,
  hishtory처럼 단순한 install/init/update UX를 제공한다.
- 현재 문제/기회: 신규 머신 참여는 `rr init`와 수동 binary 배포를 조합해야 하며,
  release asset과 self-update 경로가 없어 운영 중 버전 교체가 번거롭다.

## 계획 스냅샷

- 목표: 공개 Rustory repo에는 private endpoint/registry/token을 넣지 않으면서,
  GitHub Releases asset 기반 installer, `rr init --token --tracker`, `rr update` self-update,
  release asset 생성 스크립트를 제공한다.
- 범위:
  - `rr init`에 `--token`/`--tracker` alias 추가.
  - `rr update`가 현재 플랫폼용 release asset과 `.sha256`을 받아 검증 후 binary를 교체.
  - `install/rustory.py`가 `curl | python3` 온보딩과 optional init을 수행.
  - `scripts/build-release-assets.sh`가 updater/installer가 기대하는 raw executable asset과 checksum을 생성.
  - public docs에는 private tracker 주소나 token을 hardcode하지 않는다.
- 검증 명령:
  - `python3 -m py_compile install/rustory.py`
  - `install/rustory.py --help`
  - `scripts/build-release-assets.sh --help`
  - `cargo fmt --all --check`
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `scripts/check-release-gates.sh --manifest-mode full --work-id installer-self-update`
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고, 위 검증 명령이 재현 가능하게 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test --workspace` | `rr init --token/--tracker` alias와 `rr update` self-update 구현 |
| C2 | done | codex | `python3 -m py_compile install/rustory.py` | Python stdlib installer 구현 |
| C3 | done | codex | `scripts/build-release-assets.sh --help` | release asset/checksum 생성 스크립트 구현 |
| C4 | in_progress | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id installer-self-update` | 구현 커밋 push 후 완료 todo 삭제와 lesson 이관 |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3. `rr init --token/--tracker`, `rr update`, Python installer, release asset builder, public distribution docs를 구현했다.
- 미완료: C4. 구현 커밋 출고 후 별도 closeout 커밋에서 todo 삭제와 lesson 이관을 수행한다.
- 다음 액션: `scripts/finalize-and-push.sh --message "feat: add installer and self update" --work-id installer-self-update`로 구현 커밋을 먼저 출고한다.
- 검증 증거: `python3 -m py_compile install/rustory.py`; `install/rustory.py --help`; `scripts/build-release-assets.sh --help`; `cargo fmt --check`; `cargo test --workspace` (192 passed); `cargo clippy --workspace --all-targets -- -D warnings`; `scripts/build-release-assets.sh --dist-dir /private/tmp/rustory-dist`; `python3 install/rustory.py --asset-url file:///private/tmp/rustory-dist/rr-aarch64-apple-darwin --checksum-url file:///private/tmp/rustory-dist/rr-aarch64-apple-darwin.sha256 --bin-dir /private/tmp/rustory-install-test`; `/private/tmp/rustory-dist/rr-aarch64-apple-darwin update --dry-run --asset-url https://example.invalid/rr-aarch64-apple-darwin --sha256 0000000000000000000000000000000000000000000000000000000000000000 --install-path /private/tmp/rustory-update-test/rr`; `scripts/check-release-gates.sh --manifest-mode full --work-id installer-self-update`.
