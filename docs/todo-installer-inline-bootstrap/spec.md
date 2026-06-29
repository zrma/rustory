# Spec: installer-inline-bootstrap

## 배경

- 요청 맥락: 신규 Linux/macOS 머신에서 Rustory grid에 붙으려면 `repo-root.env`와 `swarm.key`를 먼저 복사해야 해서,
  사용자가 기대한 "복사/붙여넣기 한 번" 온보딩이 아니었다.
- 현재 문제/기회: installer가 tracker token은 inline으로 받을 수 있지만 shared `swarm.key`는 파일 경로만 지원한다.
  private archive에는 실제 secret 값을 둘 수 있으므로, public installer는 `swarm.key`를 base64 argument로 받아
  대상 머신에 파일 선배치 없이 쓸 수 있어야 한다.

## 계획 스냅샷

- 목표: `curl ... | python3 - ...` 한 번으로 binary/config/swarm key/hook/daemon/Hishtory import가 끝나게 한다.
- 범위: installer의 inline swarm key 지원, public 문서 placeholder, private archive README one-paste command, release version bump.
- 검증 명령: `python3 -m py_compile install/rustory.py`, installer temp HOME smoke, `scripts/run-manifest-checks.sh --mode quick --work-id installer-inline-bootstrap`.
- 완료 기준: C-체크리스트 항목이 `done` 상태가 되고 검증 명령이 재현 가능하게 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `python3 -m py_compile install/rustory.py` | installer가 `--swarm-key-b64`를 지원하고 `--swarm-key-source`와 동시에 쓰면 실패한다. |
| C2 | done | codex | installer temp HOME smoke | base64 swarm key만으로 `~/.config/rustory/swarm.key`가 설치되고 fingerprint 확인이 된다. |
| C3 | done | codex | `scripts/run-manifest-checks.sh --mode quick --work-id installer-inline-bootstrap` | public 문서는 placeholder만 사용하고 private archive README는 파일 선배치 없는 one-paste command를 제공한다. |
| C4 | todo | codex | release asset version check | `rr` 1.0.8 release asset이 게시되어 신규 설치가 추적 가능한 버전을 받는다. |

## 완료/미완료/다음 액션

- 완료: C1-C3.
- 미완료: C4.
- 다음 액션: commit/push 후 v1.0.8 release asset을 게시하고 latest download 검증을 진행한다.
- 검증 증거: `python3 -m py_compile install/rustory.py`, mutual-exclusion guard smoke, installer temp HOME smoke, `cargo fmt --all --check`, `cargo test --workspace` (201 passed), `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/run-manifest-checks.sh --mode quick --work-id installer-inline-bootstrap`, `scripts/check.sh --fast`.
