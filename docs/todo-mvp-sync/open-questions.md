# Open Questions

1) 인증 방식
- user_id 발급/로그인 방식은?
- 토큰 기반? 단순 공유 시크릿?

2) 보안/E2EE
- MVP에서 평문 저장으로 갈지?
- 추후 E2EE 적용 시 키 관리 방식은?

3) 동기화 범위
- 피어 다운로드 시 by device_id 필터를 둘지?
- 최대 페이지 크기/페이징 기준 결정
- Mesh 모드에서 피어 발견/주소 교환 방식은?

4) 시계 불일치
- 클라이언트 timestamp 신뢰 수준
- 정렬 tie-breaker 정책
- entry_id 생성 규칙(UUIDv4/v7 등) 결정

5) 삭제/정리 정책
- tombstone 도입 시점
- 자동 prune/dedup 정책 필요 여부

6) UI
- 기본 fzf 한정(최근 N) 기준값
- full-history 탐색 옵션 여부
- 최근 N 값을 설정으로 노출할지?

7) 기술 스택
- Rust 기준 어떤 프레임워크/라이브러리를 쓸지 (예: axum, sqlx)
