# Spec: installer-hook-import

## 배경

- 요청 맥락: 신규 머신에서 one-line installer로 Rustory 설치, tracker init, shell hook 설치, Hishtory import/전환까지 끝내고 싶다.
- 현재 문제/기회: 기존 installer는 binary 설치와 `rr init`까지만 수행해 `Ctrl-R` hook 전환과 Hishtory import/cleanup이 수작업으로 남았다.

## 계획 스냅샷

- 목표: one-line installer가 binary 설치, tracker init, shell hook 설치, Hishtory import, Hishtory hook 제거까지 수행한다.
- 범위: `install/rustory.py`, public docs, private archive README 갱신. Rust binary 동작 변경은 포함하지 않는다.
- 검증 명령: `python3 -m py_compile install/rustory.py`, installer temp rc/import smoke, `cargo fmt --all --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/run-manifest-checks.sh --mode quick --work-id installer-hook-import`.
- 완료 기준: installer 옵션이 idempotent하고 token을 출력하지 않으며, Hishtory DB/디렉터리는 보존하고 startup files의 Hishtory hook 라인만 제거한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `python3 -m py_compile install/rustory.py` | installer hook/import/Hishtory cleanup 옵션 구현 |
| C2 | done | codex | `python3 install/rustory.py --help` | public docs와 CLI help에 onboarding 옵션 반영 |
| C3 | in_progress | codex | `scripts/run-manifest-checks.sh --mode quick --work-id installer-hook-import` | repo gate 통과 및 push |

## 완료/미완료/다음 액션

- 완료: installer `--install-hook`, `--import-hishtory`, `--keep-hishtory-hooks` 구현. temp HOME smoke에서 Rustory hook block idempotency와 Hishtory hook 제거 확인. public docs와 private archive README 갱신.
- 미완료: 구현 커밋 push와 별도 closeout 커밋.
- 다음 액션: 구현 커밋을 push한 뒤 LESSONS_LOG를 남기고 별도 closeout 커밋에서 todo workspace를 삭제한다.
- 검증 증거: `python3 -m py_compile install/rustory.py`, `python3 install/rustory.py --help`, temp HOME installer smoke, `cargo fmt --all --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/run-manifest-checks.sh --mode quick --work-id installer-hook-import`.
