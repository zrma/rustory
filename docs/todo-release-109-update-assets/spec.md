# Spec: release-109-update-assets

## 배경

- 요청 맥락: `rr update`가 GitHub latest release asset을 받았지만 최신 `main`의 `rr doctor --auto-fix`가 포함되지 않은 `1.0.8 / 280bb56310fd` 바이너리를 다시 설치했다.
- 현재 문제/기회: `main`에는 `doctor --auto-fix`와 watch UI 개선이 반영됐지만 package version, tag, release asset이 아직 갱신되지 않아 daily-driver 머신이 self-update로 받을 수 없다.

## 계획 스냅샷

- 목표: `rr update`로 `doctor --auto-fix`가 포함된 새 release asset을 설치할 수 있게 `v1.0.9`를 출고한다.
- 범위: `Cargo.toml`/`Cargo.lock` version bump, 배포 문서 예시 갱신, release asset build/upload, latest update smoke.
- 검증 명령: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/build-release-assets.sh`, `gh release upload v1.0.9 ...`, `rr update --version v1.0.9`.
- 완료 기준: `rr update --version v1.0.9`로 받은 binary가 `version: 1.0.9`, build revision이 `v1.0.9` tag commit, `rr doctor --auto-fix`를 지원한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `cargo test --workspace` | package version을 1.0.9로 올리고 기존 테스트가 통과한다. |
| C2 | todo | codex | `scripts/build-release-assets.sh` | 현재 플랫폼 release asset과 checksum을 생성하고 binary version/revision을 검증한다. |
| C3 | todo | codex | `gh release upload v1.0.9 ...` | GitHub Release `v1.0.9`와 필요한 updater asset/checksum을 게시한다. |
| C4 | todo | codex | `rr update --version v1.0.9 && rr doctor --auto-fix --help` | 설치된 updater 경로에서 새 asset을 받아 `doctor --auto-fix`가 사용 가능함을 검증한다. |

## 완료/미완료/다음 액션

- 완료: 없음.
- 미완료: C1-C4.
- 다음 액션: version bump 후 release asset을 빌드/게시하고 updater smoke를 수행한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-release-109-update-assets`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-release-109-update-assets/open-questions.md`.
