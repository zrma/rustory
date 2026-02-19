# Rustory

분산/동기화 기반 셸 히스토리 관리 도구.

## Quick Start

- 빠른 시작: `docs/quickstart.md`
- 로컬 검증(권장): `scripts/check.sh`
- 빠른 검증(smoke 생략): `scripts/check.sh --fast`

## Agent Navigation

- 작업 시작: `docs/HANDOFF.md`
- 문서 역할 경계: `docs/README_OPERATING_POLICY.md`
- 실행 방법론: `docs/EXECUTION_LOOP.md`
- 운영 모델: `docs/OPERATING_MODEL.md`
- 출고 절차: `docs/CHANGE_CONTROL.md`
- 지속 개선: `docs/IMPROVEMENT_LOOP.md`
- 에스컬레이션: `docs/ESCALATION_POLICY.md`
- 교훈 로그: `docs/LESSONS_LOG.md`
- 메타/검증 맵: `docs/REPO_MANIFEST.yaml`
- 에이전트 실행 규칙: `AGENTS.md`

## Product Docs

- P2P 상세/트러블슈팅: `docs/p2p.md`
- 데몬/스케줄러: `docs/daemon.md`
- hook 설정: `docs/hook.md`
- 수용 테스트 문서: `docs/acceptance/docker-macos-linux.md`

## Development

- 테스트: `cargo test --workspace`
- 린트/포맷: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`
- 로컬 P2P 스모크: `scripts/smoke_p2p_local.sh`
- 보안 점검(권장): `scripts/secret_scan.sh`
