# Spec: rand-major-refresh

## 배경

- 요청 맥락: dependency maintenance 루프에서 직접 의존성 중 `rand 0.8.6 -> 0.10.1`만 남았다.
- 현재 문제/기회: `rand` 0.10은 major/API 변경이므로 lockfile refresh나 다른 HTTP/storage 변경과 섞지 않고 직접 호출부만 좁게 갱신해야 한다.

## 계획 스냅샷

- 목표: `rustory`의 직접 `rand` 의존성을 0.10 계열로 올리고, CLI jitter와 swarm key 생성 호출부를 0.10 API에 맞춘다.
- 범위: `Cargo.toml`, `Cargo.lock`, 직접 `rand` 호출부(`src/cli.rs`, `src/config.rs`)와 작업 증적 문서만 수정한다. libp2p 계열 transitive `rand 0.8` 제거는 이번 범위에서 제외한다.
- 검증 명령: `cargo tree --target all -i rand@0.10.1`, `cargo tree --target all -i rand@0.8.6`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/run-manifest-checks.sh --mode full --repo-key rustory --work-id rand-major-refresh`.
- 완료 기준: 직접 `rand` 요구 버전이 0.10 계열이 되고, 직접 호출부가 컴파일/테스트/클리피/full manifest gate를 통과하며, 남은 transitive `rand 0.8` 경로가 명시된다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo tree --target all -i rand@0.10.1` + `cargo tree --target all -i rand@0.8.6` | 직접/간접 `rand` 사용 경로를 확인하고 범위를 직접 의존성 갱신으로 고정 |
| C2 | done | codex | `cargo test --workspace` | `rand` 요구 버전과 직접 호출부를 0.10 API에 맞게 갱신 |
| C3 | done | codex | `cargo clippy --workspace --all-targets -- -D warnings` | 클리피 기준으로 API 전환 후 경고 없는지 확인 |
| C4 | done | codex | `scripts/run-manifest-checks.sh --mode full --repo-key rustory --work-id rand-major-refresh` | 출고 전 full manifest gate 통과 |
| C5 | todo | codex | `scripts/finalize-and-push.sh --message "build: update rand" --work-id rand-major-refresh` | 작업 단위 커밋/푸시 후 원격 SHA 검증 |

## 완료/미완료/다음 액션

- 완료: C1, C2, C3, C4. 직접 `rand`는 `rand@0.10.1 -> rustory`로 갱신했고, 남은 `rand@0.8.6`은 libp2p 계열 transitive 경로임을 확인했다.
- 미완료: C5.
- 다음 액션: 작업 단위 커밋/푸시를 진행한다.
- 검증 증거: `scripts/start-work.sh --work-id rand-major-refresh`, `cargo tree --target all -i rand@0.10.1`, `cargo tree --target all -i rand@0.8.6`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/run-manifest-checks.sh --mode full --repo-key rustory --work-id rand-major-refresh`.
