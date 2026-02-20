# Spec: doctor-runtime-status-report

## 배경

- 최근 `rr record` 훅 경로에 `RUSTORY_ASYNC_UPLOAD*`, `RUSTORY_AUTO_PRUNE*` 설정이 추가됐지만 `rr doctor`에서 유효 설정/marker 상태를 바로 확인하기 어렵다.
- 운영 중에는 "왜 자동 업로드/보관이 실행되지 않았는지"를 빠르게 구분해야 하므로, doctor 출력에 런타임 상태 요약이 필요하다.

## 계획 스냅샷

- 목표: `rr doctor`만으로 async upload/auto prune 설정 해석 결과와 다음 실행 가능 시점을 점검할 수 있게 한다.
- 범위:
  - `src/cli.rs` doctor 출력 확장
  - doctor 보조 파서/요약 함수 및 단위 테스트 추가
  - 관련 사용자 문서(`docs/p2p.md`, 필요 시 `docs/quickstart.md`) 업데이트
- 검증 명령:
  - `scripts/run-manifest-checks.sh --mode quick --work-id doctor-runtime-status-report`
  - `cargo fmt --all --check`
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
- 완료 기준:
  - doctor 출력에 async upload/auto prune 유효 설정 + marker 기반 상태가 노출된다.
  - 잘못된 env 값은 doctor에서 `invalid:`로 표시되어 원인 파악이 가능하다.
  - C1..C3이 `done`이고 검증 명령이 재현 가능하게 남는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test --workspace` | doctor runtime status 요약 로직/출력 구현 |
| C2 | done | codex | `cargo test --workspace` | async upload/auto prune doctor 요약 단위 테스트 추가 |
| C3 | in_progress | codex | `scripts/run-manifest-checks.sh --mode quick --work-id doctor-runtime-status-report` | 문서 반영 + quick manifest 게이트 통과 |

## 완료/미완료/다음 액션

- 완료: C1, C2.
- 미완료: C3(마감 커밋에서 todo 정리 예정).
- 다음 액션: 기능 커밋을 먼저 완료한 뒤, 마감 커밋에서 todo 워크스페이스를 정리하고 `LESSONS_ARCHIVE`에 식별자를 남긴다.
- 검증 증거:
  - `cargo fmt --all --check`
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `scripts/run-manifest-checks.sh --mode quick --work-id doctor-runtime-status-report`
