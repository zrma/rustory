# Yamux 설정 호환 패치

- 원본: [libp2p-yamux 0.47.0](https://crates.io/crates/libp2p-yamux/0.47.0), MIT.
- `src/lib.rs`의 원본 저작권/라이선스 고지를 유지한다.
- 변경: `Config::bounded_v013` 생성자와 0.13 선택 회귀 테스트만 추가한다.
  기존 setter는 0.12로 전환되므로 relay의 0.13 수신 예산 설정에 사용할 수 없다.
- 프로토콜, 버퍼 처리, stream 수명 관리는 upstream 그대로 유지한다.
- Cargo manifest는 같은 crate에서 복사했다. 별도 미공개 test harness를 요구하는
  compliance target 및 불필요한 async-std dev dependency는 제외했다.
  Rustory에서 실제 stream/relay 통합 테스트를 실행한다.
- upstream이 0.13 상한 설정 API를 공개하면 이 patch를 제거하고 동일한
  협상/stream 포화·회복/실제 relay sync 검증을 수행한다.
- 검증: `cargo test -p libp2p-yamux --lib`, `cargo test p2p_muxer`,
  `scripts/smoke_p2p_local.sh`.
