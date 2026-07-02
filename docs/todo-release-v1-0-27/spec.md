# Spec: release-v1-0-27

## 배경

- 요청 맥락: 보안 스캔 대응과 회귀 검증이 끝난 현재 `main`을 daily-driver 배포용 패치 릴리즈로 출고한다.
- 현재 문제/기회: `v1.0.26` 이후 보안 hardening/게이트 smoke 수정이 포함되어 있으므로 새 release asset과 설치 대상 업데이트가 필요하다.

## 계획 스냅샷

- 목표: `Cargo.toml`/`Cargo.lock`을 `1.0.27`로 올리고 `v1.0.27` GitHub release asset을 배포한다.
- 범위: 버전 bump, release gate, GitHub release upload, 로컬 Mac 및 k8s 노드 5대의 `rr update` 적용 확인.
- 검증 명령: `scripts/check.sh --fast`, `scripts/release-version.sh --version v1.0.27 --profile daily-driver --work-id release-v1-0-27`.
- 완료 기준: `rr version`이 배포 대상에서 `1.0.27`을 보고하고 release todo가 마감 기록과 함께 삭제된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `scripts/check.sh --fast` | 버전 bump 후 fast regression 확인 |
| C2 | todo | codex | `scripts/release-version.sh --version v1.0.27 --profile daily-driver --work-id release-v1-0-27` | `v1.0.27` release asset 생성/업로드 |
| C3 | todo | codex | `rr version` | 로컬 Mac 및 k8s 노드 5대에 `rr update` 적용 확인 |

## 완료/미완료/다음 액션

- 완료: 없음.
- 미완료: C1, C2, C3.
- 다음 액션: fast regression 확인 후 버전 bump 커밋을 푸시하고 release/upload/deploy를 진행한다.
- 검증 증거: `scripts/check.sh --fast`, `scripts/release-version.sh --version v1.0.27 --profile daily-driver --work-id release-v1-0-27`, 배포 대상별 `rr version`.
