# Rustory Documentation

이 문서는 인덱스 전용이다. 실행 순서/옵션 규칙의 경계는 `docs/HANDOFF.md`, `docs/EXECUTION_LOOP.md`, `docs/CHANGE_CONTROL.md`, `docs/REPO_MANIFEST.yaml`을 따르고, 실제 동작은 관련 코드/스크립트/CLI help를 직접 확인한다.

## 문서 맵 (링크 전용)

### 1) 코어 네비게이션

- [공통 에이전트 하네스 인터페이스](agent-harness.md)
- [Rustory 작업 네비게이션](HANDOFF.md)
- [실행 루프](EXECUTION_LOOP.md)
- [출고 절차](CHANGE_CONTROL.md)
- [저장소 메타 매니페스트](REPO_MANIFEST.yaml)

### 2) 운영 정책/협업 가드레일

- [에이전트 실행 규칙](../AGENTS.md)
- [운영 모델](OPERATING_MODEL.md)
- [README 운영 정책](README_OPERATING_POLICY.md)
- [스킬 운영 가이드](SKILL_OPERATING_GUIDE.md)
- [장기 유지보수 축](MAINTENANCE_PILLARS.md)
- [보안 및 프라이버시 모델](security.md)
- [지속 개선 루프](IMPROVEMENT_LOOP.md)
- [에스컬레이션 정책](ESCALATION_POLICY.md)
- [교훈 로그](LESSONS_LOG.md)
- [교훈 아카이브](LESSONS_ARCHIVE.md)

### 3) 제품/운영 문서

- [Quick Start](quickstart.md)
- [Distribution](distribution.md)
- [Hishtory Migration Runbook](hishtory-migration.md)
- [Atuin Migration Runbook](atuin-migration.md)
- [P2P 가이드](p2p.md)
- [Daemon 가이드](daemon.md)
- [Hook 가이드](hook.md)
- [개발 플레이북](dev-playbook.md)
- [MVP 결정 메모](mvp.md)
- [수용 테스트 인덱스](acceptance/README.md)

문서 진입점(추가/이동/삭제) 변경 시 `docs/README.md` 인덱스와 `docs/REPO_MANIFEST.yaml` entrypoint를 같은 턴에서 함께 갱신한다.
상세 절차/예외 경계는 `docs/CHANGE_CONTROL.md`를 따르고, 실제 검증 명령과 실행 동작은 `docs/REPO_MANIFEST.yaml`, `scripts/*`, CLI help를 직접 확인한다.
