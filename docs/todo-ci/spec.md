# CI (GitHub Actions)

## 문제
- 로컬에서는 `cargo fmt/test/clippy`를 돌리더라도, PR/푸시 시 자동 검증이 없으면 회귀를 놓치기 쉽다.
- 네트워크/스모크 계열은 unit 테스트만으로 놓칠 수 있어 최소 1개의 수용 테스트가 필요하다.

## 목표
- GitHub Actions CI를 추가한다.
- 최소 체크:
  - `cargo fmt --all --check`
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
- (권장) 로컬 P2P 스모크 스크립트(`scripts/smoke_p2p_local.sh`)를 CI에서 1회 실행한다.

## 범위
- `.github/workflows/ci.yml` 추가

## 비목표
- 릴리즈/배포 자동화
- 커버리지/벤치마크

## 결정 사항
- 러스트 툴체인은 `rust-toolchain.toml`(1.89.0)을 따른다.
- OS: ubuntu-latest

## 완료 조건
- CI 워크플로 파일이 추가되고, 로컬에서 동일 커맨드가 모두 통과한다.
- todo 폴더 삭제
