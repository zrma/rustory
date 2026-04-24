# HTTP Retry/Backoff Spec

## 목표
- tracker client(등록/목록)와 HTTP sync(pull/push)가 일시적인 네트워크 문제에 대해 더 견고하게 동작하도록 한다.
- 3회 시도(총 3 attempts) 동안, **connect/read timeout을 지수적으로 증가**시키며 재시도한다.

## 범위
- `src/http_retry.rs`에 공통 retry 헬퍼를 추가한다.
- 적용 대상:
  - `TrackerClient::register`, `TrackerClient::list`
  - `transport::http_pull_batch`, `transport::http_push_batch`
- retry 조건:
  - 네트워크/전송 계열 에러(transport error)
  - HTTP 408, 429, 5xx
  - 그 외 4xx(예: 401)는 재시도하지 않는다.

## 운영 성질
- tracker list는 fallback(peer_book)으로 빨리 넘어갈 수 있도록, 전체 대기 시간이 과도해지지 않게 한다(짧은 base timeout + cap).
- 로그/에러 메시지는 기존처럼 `anyhow::Context`로 URL/요청 맥락을 남긴다.

