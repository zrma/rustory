# Spec: yamux-migration

## 배경

- 요청 맥락: 장기 relay 연결에서 발생하는 mplex 작업 참조 누적을 피하도록 Yamux로 전환한다.
- 현재 문제/기회: 회로 종료 후에도 mplex 쓰기 작업 참조가 유지되는 경로가 재현됐다. 기존 클라이언트를 순차 업데이트할 수 있다.

## 계획 스냅샷

- 목표: Rustory 1.1.0의 새 연결은 양쪽이 지원하면 Yamux를 선택하고, 전환 중 구버전과의 동기화는 유지한다.
- 범위: 클라이언트와 relay의 multiplexer, 협상 관측, 직접/relay 혼합 버전 시험, 버전과 전환 문서.
- 제약: PSK/Noise 인증, PeerId, 저장 데이터/동기화 프로토콜과 연결 quota/timeout을 유지한다. 공개 자료에는 환경 inventory나 raw 로그를 기록하지 않는다.
- 제외: mplex 라이브러리 fork와 무관한 의존성 일괄 갱신. 구버전 지원 제거는 기기 전환 및 프로토콜 관측 후 별도 변경으로 결정한다.
- 버전: 하위 호환 기능 추가 및 mplex 사용 중단 예고로 1.1.0을 사용한다. 패치 번호는 0으로 초기화한다.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id yamux-migration`.
- 완료 기준: C1-C3의 로컬 구현/검증을 닫고 배포 및 장기 관찰의 남은 조건을 전환 문서에 남긴다. 로컬 시험 통과를 운영 전환 완료로 보고하지 않는다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test muxer` | Yamux 우선 협상과 mplex fallback 및 협상 결과 확인 |
| C2 | done | codex | `cargo test p2p_relay` | 혼합 버전과 새 버전의 실제 relay 회로/데이터 전송, 반복 회로 종료 검증 |
| C3 | in_progress | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id yamux-migration` | 1.1.0 버전, 전환 문서, 전체 검증 및 로컬 change 마감 |

## 완료/미완료/다음 액션

- 완료: C1-C2. Yamux 우선 및 legacy 양방향 협상, relay 회로 반복, 구버전 바이너리 혼합 동기화와 전체 native gate 통과.
- 미완료: C3의 로컬 change 및 문서 마감.
- 다음 액션: 검증 결과와 후속 배포 조건을 소유 문서로 이관하고 작업 packet을 닫는다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-yamux-migration`.
