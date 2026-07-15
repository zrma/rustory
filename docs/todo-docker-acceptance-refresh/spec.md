# Spec: docker-acceptance-refresh

## 배경

- 요청 맥락: 로컬 Docker daemon을 사용할 수 있으므로 현재 `main`의 relay acceptance를 실제 Docker 환경에서 다시 검증한다.
- 현재 문제/기회: 기본 CI와 빠른 로컬 게이트만으로는 macOS/Linux 간 relay fallback과 분리된 두 peer의 relay-only 수렴을 증명할 수 없다.

## 계획 스냅샷

- 목표: 현재 `main`에서 Docker 기반 macOS/Linux relay fallback과 two-peer relay-only 동기화가 모두 통과하는지 확인한다.
- 범위: Docker daemon preflight, `scripts/check.sh --acceptance` 실행, 저장소 원인 실패가 확인될 때의 최소 수정, 공개 경계 점검을 포함한다.
- 검증 명령: `scripts/check.sh --acceptance`.
- 완료 기준: 기본 검사와 두 Docker acceptance가 모두 통과하고 공개 가능한 기록에 환경 식별자 없이 검증 판정과 명령만 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `docker info >/dev/null` | Docker daemon preflight 확인 |
| C2 | done | codex | `scripts/check.sh --acceptance` | macOS/Linux relay fallback acceptance 통과 |
| C3 | done | codex | `scripts/check.sh --acceptance` | 분리된 two-peer relay-only 수렴 acceptance 통과 |
| C4 | done | codex | `python3 scripts/check-publication-boundary.py` | 공개 가능한 tracked artifact 경계 확인 |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3, C4. 호스트 config 격리 누락을 수정한 뒤 기본 검사와 두 Docker relay acceptance, 공개 경계 검사가 통과했다.
- 미완료: 없음.
- 다음 액션: 완료 todo를 마감하고 다음 릴리즈 준비 change를 시작한다.
- 검증 증거: `docker info >/dev/null`, `bash scripts/acceptance_docker_macos_linux.sh`, `scripts/check.sh --acceptance`, `python3 scripts/check-publication-boundary.py`.
