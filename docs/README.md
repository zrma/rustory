# Rustory Documentation

이 문서는 인덱스 전용이다. 실행 순서/옵션 규칙의 단일 기준은 `docs/HANDOFF.md`, `docs/EXECUTION_LOOP.md`, `docs/CHANGE_CONTROL.md`, `docs/REPO_MANIFEST.yaml`을 따른다.

## 문서 맵 (링크 전용)

### 1) 코어 네비게이션

- [Rustory 작업 네비게이션](HANDOFF.md)
- [실행 루프](EXECUTION_LOOP.md)
- [출고 절차](CHANGE_CONTROL.md)
- [저장소 메타 매니페스트](REPO_MANIFEST.yaml)

### 2) 운영 정책/협업 가드레일

- [에이전트 실행 규칙](../AGENTS.md)
- [운영 모델](OPERATING_MODEL.md)
- [README 운영 정책](README_OPERATING_POLICY.md)
- [스킬 운영 가이드](SKILL_OPERATING_GUIDE.md)
- [지속 개선 루프](IMPROVEMENT_LOOP.md)
- [에스컬레이션 정책](ESCALATION_POLICY.md)
- [교훈 로그](LESSONS_LOG.md)
- [교훈 아카이브](LESSONS_ARCHIVE.md)

### 3) 제품/운영 문서

- [Quick Start](quickstart.md)
- [P2P 가이드](p2p.md)
- [Daemon 가이드](daemon.md)
- [Hook 가이드](hook.md)
- [개발 플레이북](dev-playbook.md)
- [MVP 메모](mvp.md)
- [수용 테스트 인덱스](acceptance/README.md)

문서 진입점(추가/이동/삭제) 변경 시 `docs/README.md` 인덱스와 `docs/REPO_MANIFEST.yaml` entrypoint를 같은 턴에서 함께 갱신한다.
상세 절차/예외/검증 명령은 `docs/CHANGE_CONTROL.md`의 `문서 진입 순서 (무컨텍스트)`를 단일 기준으로 따른다.
