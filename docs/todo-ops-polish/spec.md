# ops-polish spec

## 목표
- 반복 개발 루프(검토 -> 검증 -> 푸시)를 더 빠르고 일관되게 만든다.
- P2P 동기화의 관측성을 개선한다(특히 pull 쪽).
- 셸 훅 기반 자동 기록에서 민감 커맨드를 쉽게 제외할 수 있게 한다(옵션).

## 범위
### 1) 원커맨드 점검 스크립트 추가
- `scripts/check.sh`를 추가한다.
- 기본 동작은 CI와 동일하게 다음을 순서대로 수행한다.
  - `cargo fmt --all --check`
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `scripts/smoke_p2p_local.sh`
- 옵션:
  - `--no-smoke` 또는 `--fast`: 스모크를 생략한다.

### 2) P2P pull 요약 로그 추가
- `rr p2p-sync`가 peer별로 pull 결과를 1줄로 요약 로그로 출력할 수 있게 한다.
  - 예: `p2p pull summary: <peer>: received=<n> inserted=<n> ignored=<n>`
- 요약 로그는 의미가 있을 때만 출력한다.
  - 권장: `received > 0` 또는 `inserted > 0` 인 경우에만 출력.
- 이를 위해 pull 경로에서 DB insert의 `inserted/ignored` 통계를 집계한다.

### 3) 자동 기록 필터(민감 커맨드 제외)
- `rr record`가 특정 패턴의 커맨드를 기록하지 않도록(= skip) 하는 옵션을 추가한다.
- 설정 소스:
  - env: `RUSTORY_RECORD_IGNORE_REGEX`
  - config.toml: `record_ignore_regex`
  - 우선순위: env > config
- 동작:
  - 패턴이 설정되어 있고 `cmd`에 매칭되면 아무 것도 기록하지 않고 성공으로 종료한다(exit 0).
  - 정규식이 잘못된 경우는 안전을 위해 기록을 스킵하고(exit 0), 가능하면 경고를 출력한다(훅이 stderr를 버리는 환경도 고려).
- 문서에 사용 예시(대표적인 ignore regex)를 추가한다.

## 비목표(이번 작업에서 하지 않음)
- 커맨드 redaction(부분 마스킹) 기능
- multi-line/structured history(세션 단위) 모델링
- 암호화 저장(E2EE) 등 보안 모델 확장

## 완료 조건(DoD)
- `cargo fmt`, `cargo test`, `cargo clippy -D warnings` 통과
- `scripts/smoke_p2p_local.sh` 통과
- `scripts/check.sh`로 위 검증을 1회에 수행 가능
- 관련 문서 갱신(최소 `docs/dev-playbook.md`, `docs/hook.md`)

