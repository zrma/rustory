# Spec: hook-doctor-status

## 배경

- 요청 맥락: 활성 todo가 없어 다음 MVP 온보딩 마일스톤을 검토했고, 최근 `rr doctor`가 `fzf`/DB 상태를 보고하도록 보강된 상태다.
- 현재 문제/기회: `rr doctor`가 현재 셸에 Rustory hook이 실제로 적용됐는지, `RUSTORY_HOOK_DISABLE`로 비활성화됐는지, ctrl+r 검색 limit 값이 유효한지 보여주지 않아 hook 온보딩 실패를 즉시 구분하기 어렵다.

## 계획 스냅샷

- 목표: `rr doctor` 텍스트/JSON에 shell hook 상태를 추가해 hook 설치/비활성화/검색 limit 문제를 한 번에 확인할 수 있게 한다.
- 범위: `rr hook`이 child process에서 볼 수 있는 설치 마커를 export하고, doctor report/문서/테스트를 갱신한다. 실제 셸 rc 파일 자동 수정이나 hook 자동 설치는 범위에서 제외한다.
- 검증 명령: `cargo test doctor_report --workspace`, `cargo test hook_status --workspace`, `cargo test hook_contains_disable_and_ctrl_r_and_rr_filter --workspace`, `scripts/run-manifest-checks.sh --mode quick --work-id hook-doctor-status`.
- 완료 기준: doctor JSON에 hook 섹션이 포함되고, 텍스트 출력이 설치/disable/search_limit 상태를 표시하며, hook 렌더링 테스트와 todo readiness가 통과한다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test doctor_report --workspace` | doctor report에 hook 상태 스키마와 텍스트 출력 추가 |
| C2 | done | codex | `cargo test hook_contains_disable_and_ctrl_r_and_rr_filter --workspace` | bash/zsh hook이 `RUSTORY_HOOK_INSTALLED=1` 마커를 export하도록 갱신 |
| C3 | in_progress | codex | `scripts/run-manifest-checks.sh --mode quick --work-id hook-doctor-status` | quick manifest/todo 게이트 통과 및 문서 반영 |

## 완료/미완료/다음 액션

- 완료: C1, C2.
- 미완료: C3.
- 다음 액션: 기능 change를 출고한 뒤 별도 close change에서 완료 todo를 `docs/LESSONS_LOG.md`에 기록하고 삭제한다.
- 검증 증거: `cargo test doctor_report --workspace`, `cargo test hook_status --workspace`, `cargo test hook_contains_disable_and_ctrl_r_and_rr_filter --workspace`, `cargo fmt --all --check`, `scripts/run-manifest-checks.sh --mode quick --work-id hook-doctor-status`.
