# Spec: workflow-action-pin-parity

## 배경

- 요청 맥락: 열린 `docs/todo-*`와 GitHub issue가 없고 최신 `main` CI도 green이라 다음 유지보수 후보를 검토했다.
- 현재 문제/기회: `CI` workflow는 action을 SHA로 pin하고 있지만, `Docs Integrity`와 `Release Gates` workflow에는 아직 `actions/checkout@v6.0.2`, `actions/setup-python@v6.2.0` 태그 참조가 남아 있다.
- 후보 목록:
  - `workflow-action-pin-parity`: 워크플로 action pinning 규칙을 CI 전반에 맞춘다.
  - `secret-scan-doc-parity`: 로컬 `scripts/check.sh --secret-scan`과 CI secret scan의 운영 문서 노출을 재점검한다.
  - `doctor-runtime-config-audit`: `rr doctor` 출력과 `rr init` 템플릿의 runtime 설정 표면을 추가 점검한다.
- 선택: 이번 작업은 CI 운영 drift를 줄이는 `workflow-action-pin-parity`를 진행한다. 사용자 판단이 필요한 기능 정책 변경은 포함하지 않는다.

## 계획 스냅샷

- 목표: GitHub Actions workflow에서 같은 action/version을 태그와 SHA pin 혼합 없이 일관되게 참조한다.
- 범위: `.github/workflows/docs-integrity.yml`, `.github/workflows/release-gates.yml`, 필요 시 이 todo spec과 closure lesson만 수정한다.
- 검증 명령: `scripts/run-manifest-checks.sh --mode quick --work-id workflow-action-pin-parity`, `scripts/check-release-gates.sh --manifest-mode full --work-id workflow-action-pin-parity`.
- 완료 기준: `actions/checkout`과 `actions/setup-python`의 활성 workflow 참조가 검증한 SHA로 pin되고, release gates와 최신 push CI가 green이다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `git ls-remote https://github.com/actions/checkout.git refs/tags/v6.0.2` | `actions/checkout` v6.0.2의 pin SHA를 확인한다. |
| C2 | todo | codex | `git ls-remote https://github.com/actions/setup-python.git refs/tags/v6.2.0` | `actions/setup-python` v6.2.0의 pin SHA를 확인한다. |
| C3 | todo | codex | `rg -n "uses: actions/(checkout|setup-python)@" .github/workflows` | workflow의 action 참조를 검증한 SHA + version comment 형태로 정렬한다. |
| C4 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id workflow-action-pin-parity` | repo release gates를 통과시킨다. |

## 완료/미완료/다음 액션

- 완료: `scripts/start-work.sh --work-id workflow-action-pin-parity`로 todo workspace를 생성하고 초기 readiness/open-questions/quick manifest 게이트를 통과했다.
- 미완료: C1, C2, C3, C4.
- 다음 액션: planning commit을 먼저 푸시한 뒤 workflow pinning을 구현한다.
- 검증 증거: `scripts/start-work.sh --work-id workflow-action-pin-parity`, `git ls-remote https://github.com/actions/checkout.git refs/tags/v6.0.2`, `git ls-remote https://github.com/actions/setup-python.git refs/tags/v6.2.0`.
