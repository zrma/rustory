# Swarm Key Fingerprint (PSK)

## 문제
P2P/relay 통신은 PSK(pnet) `swarm.key`에 의존한다.
여러 디바이스가 같은 키를 공유해야 하는데, 운영 중에는 "정말 같은 키인가?"를 빠르게 확인하기 어렵다.

## 목표
- `rr swarm-key` 커맨드를 추가해 다음을 출력한다.
  - resolved key path
  - key fingerprint(키 원문을 노출하지 않는 식별자)

## 범위
- `src/cli.rs`: `swarm-key` 서브커맨드 추가
- 문서 업데이트: `docs/p2p.md` (선택)

## 비목표
- 키 배포/복사 자동화
- 멤버십/ACL

## 결정 사항
- fingerprint는 libp2p `PreSharedKey::fingerprint()`를 사용한다.
- 키 파일이 없으면 기존 정책대로 생성 후 fingerprint를 출력한다.

## 완료 조건
- `cargo fmt/test/clippy -D warnings` 통과
- todo 폴더 삭제
