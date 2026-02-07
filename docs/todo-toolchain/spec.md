# Toolchain Standardization

## 문제
- Rust 버전/컴포넌트(rustfmt/clippy)가 개발자 환경마다 달라지면,
  - `edition = "2024"` 호환성,
  - fmt/clippy 결과,
  - CI 재현성
  에서 흔들릴 수 있다.

## 목표
- 프로젝트 루트에 `rust-toolchain.toml`을 추가해 툴체인을 고정한다.
- `rustfmt`, `clippy` 컴포넌트를 명시한다.

## 범위
- `rust-toolchain.toml` 추가

## 비목표
- CI 워크플로(GitHub Actions 등) 추가
- rustfmt/clippy 세부 룰 튜닝(`rustfmt.toml`, `clippy.toml`)은 필요해질 때 별도 작업으로 분리

## 결정 사항
- 채널: `1.89.0` (현재 개발 환경 기준)
- 컴포넌트: `rustfmt`, `clippy`
- 프로필: `minimal`

## 완료 조건
- `cargo fmt/test/clippy -D warnings` 통과
- todo 폴더 삭제
