# Spec: release-v1-0-58

## 배경

- 요청 맥락: Ctrl+R 검색 품질 개선과 정량 평가 하네스를 다음 daily-driver 릴리즈로 준비한다.
- 현재 문제/기회: `v1.0.57` 이후 검색 개선이 전체 출고 게이트와 플랫폼별 release asset으로 아직 고정되지 않았다.

## 계획 스냅샷

- 목표: package version을 `1.0.58`로 올리고 전체 release gate와 daily-driver asset build를 통과시킨 뒤 외부 게시 직전 상태로 만든다.
- 범위: `Cargo.toml`/`Cargo.lock` 버전 정렬, 전체 release gate, release dry-run, macOS arm64 및 Linux x86_64 asset의 로컬 build를 포함한다. push, tag, GitHub Release, 실제 배포는 명시적 게시 승인 전까지 수행하지 않는다.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-58`.
- 완료 기준: C1~C5가 통과하고 C6~C8만 외부 게시 권한 경계로 남으며, package metadata와 staged asset이 모두 `1.0.58`을 가리킨다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo metadata --no-deps --format-version 1` | package와 lockfile version을 `1.0.58`로 정렬 |
| C2 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-58` | 전체 release gate 통과 |
| C3 | done | codex | `scripts/release-version.sh --version v1.0.58 --profile daily-driver --target-ref @ --gate none --work-id release-v1-0-58 --no-remote-check --dry-run` | release plan과 target 검증 |
| C4 | done | codex | `scripts/release-version.sh --version v1.0.58 --profile daily-driver --target-ref @ --gate none --work-id release-v1-0-58 --skip-upload --allow-dirty` | daily-driver asset 로컬 build와 checksum 검증 |
| C5 | done | codex | `scripts/release-version.sh --version v1.0.58 --profile daily-driver --target-ref @ --gate none --work-id release-v1-0-58 --skip-build --skip-upload --allow-dirty` | staged asset architecture, glibc, embedded version/revision 재검증 |
| C6 | todo | codex | `scripts/finalize-and-push.sh --message "build: prepare v1.0.58 release" --work-id release-v1-0-58` | 승인 후 release target을 `main`에 push하고 원격 revision 검증 |
| C7 | todo | codex | `scripts/release-version.sh --version v1.0.58 --profile daily-driver --target-ref main --gate none --work-id release-v1-0-58 --skip-build` | `v1.0.58` GitHub Release 게시와 updater dry-run 검증 |
| C8 | todo | codex | `scripts/check-todo-closure.sh` | 게시 검증 후 완료 todo 삭제와 closure token 기록 |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3, C4, C5. package metadata와 lockfile이 `1.0.58`로 정렬됐고 전체 release gate, daily-driver dry-run, macOS arm64 및 Linux x86_64 로컬 asset build와 staged metadata 검증이 통과했다.
- 미완료: C6, C7, C8.
- 다음 액션: 명시적 게시 승인 후 `main` push와 GitHub Release 게시를 수행하고 실제 배포를 별도 검증한 뒤 todo를 닫는다.
- 검증 증거: `cargo metadata --no-deps --format-version 1`, `scripts/check-release-gates.sh --manifest-mode full --work-id release-v1-0-58`, `scripts/release-version.sh --version v1.0.58 --profile daily-driver --target-ref @ --gate none --work-id release-v1-0-58 --no-remote-check --dry-run`, `scripts/release-version.sh --version v1.0.58 --profile daily-driver --target-ref @ --gate none --work-id release-v1-0-58 --skip-upload --allow-dirty`, `scripts/release-version.sh --version v1.0.58 --profile daily-driver --target-ref @ --gate none --work-id release-v1-0-58 --skip-build --skip-upload --allow-dirty`.
