## Repository Overlay

- 실행 동작과 option의 source of truth는 `src/`, `Cargo.toml`, `scripts/`,
  `docs/REPO_MANIFEST.yaml`과 `rr --help`다.
- local-first write, shared grid identity와 per-device identity 경계를 보존한다.
- relay circuit을 포함한 실제 multi-peer evidence 없이 P2P readiness를 완료로
  표시하지 않는다.
- background daemon spawn, PID persistence와 startup 확인은 하나의 성공 단위다.
  persistence 실패 시 child를 종료·회수하고 restart 성공을 보고하지 않는다.
- 전체 빠른 gate는 `scripts/check.sh --fast`, generated drift check는
  `python3 .ai-first/check.py`, 출고 경계는 `docs/CHANGE_CONTROL.md`를 따른다.
