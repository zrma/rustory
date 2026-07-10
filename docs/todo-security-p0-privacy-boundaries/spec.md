# Spec: security-p0-privacy-boundaries

## 배경

- 요청 맥락: Hishtory의 application-level E2EE와 Rustory의 현재 보호 경계를 비교한 결과를 저장소에 남기고, 즉시 필요한 P0 privacy/security 보완을 구현한다.
- 현재 문제/기회: daily-driver P2P payload는 pnet + libp2p Noise로 보호되지만 local SQLite는 평문이며 모든 sync peer를 신뢰한다. 또한 shell hook이 앞 공백을 제거해 기록하고, debug HTTP sync는 원격 평문 URL을 명시적 transport opt-in 없이 사용할 수 있다.
- 하위 호환 원칙: DB schema, P2P protocol, tracker/relay metadata, 기존 peer와의 sync는 변경하지 않는다. 기존 정상 경로는 유지하고, 앞 공백 기록과 원격 평문 debug HTTP만 fail-closed로 바꾼다.

## 계획 스냅샷

- 목표: Rustory의 trust model을 명시하고, 앞 공백 command opt-out과 debug HTTP transport guard를 회귀 테스트와 함께 적용한다.
- 범위: `docs/security.md` 및 문서 인덱스, bash/zsh generated hook, debug `rr serve`/`rr sync` CLI와 HTTP peer URL validation, 관련 테스트/도움말.
- 검증 명령: `scripts/check-todo-readiness.sh docs/todo-security-p0-privacy-boundaries`, `cargo test hook::tests --workspace`, `cargo test http_sync --workspace`, `scripts/check-release-gates.sh --manifest-mode full --work-id security-p0-privacy-boundaries`.
- 완료 기준: C1-C5가 모두 `done`이고 기존 loopback HTTP/HTTPS/P2P 경로는 유지되며, 앞 공백 command와 implicit remote plaintext HTTP는 테스트에서 거부된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `scripts/check-doc-links.sh && scripts/check-doc-index.sh` | 현재 암호화/신뢰/metadata/at-rest/향후 E2EE 경계를 `docs/security.md`에 기록 |
| C2 | done | codex | `cargo test hook::tests --workspace` | bash/zsh hook이 원문 첫 문자가 공백인 command를 기록하지 않도록 보완 |
| C3 | done | codex | `cargo test http_sync --workspace` | loopback HTTP와 HTTPS는 유지하고 원격 평문 HTTP는 명시적 insecure opt-in 없이는 거부 |
| C4 | done | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id security-p0-privacy-boundaries` | 전체 fmt/test/clippy/installer/P2P 회귀 검증 |
| C5 | in_progress | codex | `jj diff --stat && jj status` | 호환성 영향과 change metadata를 정리하고 local working copy 결과를 보고 |

## 호환성 결정

- 기존 SQLite row/schema와 sync cursor는 그대로 읽고 쓴다.
- 기존/구버전 peer와 P2P plain/zstd protocol negotiation은 그대로 유지한다.
- 새 hook은 다음 shell reload부터 앞 공백 command를 의도적인 opt-out으로 처리한다. 이미 저장된 row는 자동 삭제하지 않는다.
- `rr serve`의 기본 loopback HTTP와 `rr sync --peers http://localhost...`는 유지한다.
- `rr sync --peers https://...`는 유지한다.
- non-loopback plaintext HTTP는 `--allow-insecure-http`를 요구한다. 기존 `rr serve --allow-unauthenticated`는 이미 명시적인 unsafe opt-in이므로 호환을 위해 plaintext opt-in도 겸한다.

## 완료/미완료/다음 액션

- 완료: C1-C4. 문서/구현/회귀 테스트를 반영하고 full release gate를 통과했다.
- 미완료: C5의 local change metadata와 todo closure.
- 다음 액션: 구현 change를 설명한 뒤 lessons log에 검증 증거를 남기고 완료 todo를 삭제한다.
- 검증 증거: hook focused tests 9 passed, HTTP sync focused tests 5 passed, generated bash/zsh hook syntax green, zsh smoke는 앞 공백 command를 제외하고 `echo public`만 기록, 실제 CLI는 implicit non-loopback server/client HTTP를 `--allow-insecure-http` 안내와 함께 거부. full release gate는 309 tests, clippy `-D warnings`, installer tests, local P2P smoke까지 통과.
