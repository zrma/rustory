# Tracker Query Encoding: user_id

## 문제
tracker list endpoint는 `GET /api/v1/peers?user_id=<...>` 형태로 필터링한다.
현재 구현은 다음 문제가 있다.
- client가 `user_id`를 URL 인코딩 없이 그대로 쿼리에 붙인다.
- server는 쿼리 스트링을 percent-decoding 없이 비교한다.

즉 `user_id`에 공백/슬래시/유니코드/`&` 등이 포함되면 필터링이 깨질 수 있다.

## 목표
- client는 `user_id`를 percent-encode 해서 요청한다.
- server는 쿼리 파라미터를 percent-decode 해서 비교한다.
- 특수문자 `user_id`에 대한 회귀 테스트를 추가한다.

## 범위
- `src/tracker.rs` (TrackerClient::list, query 파싱/필터)
- 테스트 업데이트

## 비목표
- 쿼리 파서/라우터 전면 교체(url crate 기반 전환 등)

## 결정 사항
- 최소 의존성으로 `urlencoding` crate를 사용한다(encode/decode).

## 완료 조건
- `cargo fmt/test/clippy -D warnings` 통과
- `user_id = \"u 1/2\"` 같은 값으로도 tracker list 필터가 정상 동작한다(테스트로 보장).
- todo 폴더 삭제

