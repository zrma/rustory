## Project Overlay

- 무컨텍스트 시작점은 `docs/HANDOFF.md`, 문서 역할 경계는
  `docs/README_OPERATING_POLICY.md`다.
- 실행 방법론은 `docs/EXECUTION_LOOP.md`, push/release strict gate는
  `docs/CHANGE_CONTROL.md`, escalation은 `docs/ESCALATION_POLICY.md`가 소유한다.
- tracker token, swarm key, identity key, 실제 private endpoint와 fleet topology는
  public tracked artifact에 기록하지 않는다.
- Rust updater와 Python installer의 fallback 동작을 함께 검증하고 direct-only
  성공을 relay readiness로 간주하지 않는다.
- 반복 실패는 `docs/IMPROVEMENT_LOOP.md`에 따라 가장 가까운 script, test 또는
  `docs/LESSONS_LOG.md`로 기계화한다.

## Related Documents

- Navigation: `docs/HANDOFF.md`, `docs/README.md`.
- Execution and change control: `docs/EXECUTION_LOOP.md`, `docs/CHANGE_CONTROL.md`.
- Operating and escalation: `docs/OPERATING_MODEL.md`, `docs/ESCALATION_POLICY.md`.
- Improvement: `docs/IMPROVEMENT_LOOP.md`, `docs/LESSONS_LOG.md`.
- Active work: none.
- Declared checks: `docs/REPO_MANIFEST.yaml`.
