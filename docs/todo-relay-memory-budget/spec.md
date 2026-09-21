# Spec: relay-memory-budget

## 배경

Yamux의 기본 수신 credit와 relay 연결 상한을 동시에 소진하면 소형 relay의
메모리 예산을 초과할 수 있다. 확정된 추가 누수와는 구분하며 부하 상한을 보강한다.

## 계획 스냅샷

- 목표: relay 연결/stream 수신 예산을 줄이고 회로 활동을 관측한다.
- 범위: relay 전용 Yamux 0.13 설정, 연결/예약/회로 기본값, health 누계,
  호환/포화·회복 검증과 운영 문서. 클라이언트 기본 muxer는 유지한다.
- 결정: upstream wrapper에 0.13 생성자만 추가한 작은 vendor patch를 사용한다.
  기존 setter의 0.12 전환을 피하며 프로토콜/stream 구현은 수정하지 않는다.
- 수신 credit: established 32 + 방향별 pending 8, 연결당 4 MiB/16 stream.
  명목 합계 192 MiB이며 전체 RSS 보장이 아니다. 프로세스 오버헤드와 workload도 관측한다.
- 회로 telemetry: 수락/종료 이벤트 누계를 제공한다. upstream 공개 이벤트에는
  circuit ID가 없고 연결 종료 정리가 중복될 수 있어 차이를 정확한 활성 수로 단정하지 않는다.
- 회로 기본값은 8개로 제한해 목적지 집중 시에도 16 stream 내 control 여유를 둔다.
  Yamux 상한 초과는 연결 종료이며, 용량 반환과 새로운 연결의 회복을 검증한다.
- 유지: Yamux wire 호환, pnet/Noise, NAT 환경의 PeerId 제한, 정상 relay 동기화.
- 제외: release/version 변경, live 변경, client 전체 메모리 감사, heap profiler 도입.
- 검증 명령: `cargo test p2p_muxer`, `cargo test -p libp2p-yamux --lib`,
  `scripts/check-release-gates.sh --manifest-mode full --work-id relay-memory-budget`.
- 완료 기준: 기본 클라이언트 호환, 종료한 stream 용량 반환, 초과 시 거부와 재연결,
  실제 relay sync/회로 health 및 canonical gate 통과.
- 이관: `docs/p2p.md`, `vendor/libp2p-yamux/README.rustory.md`.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | done | codex | `cargo test p2p_muxer` | 0.13 제한, 협상 및 포화 후 회복 |
| C2 | done | codex | `scripts/smoke_p2p_local.sh` | 실제 relay sync와 회로 health |
| C3 | in_progress | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id relay-memory-budget` | 문서·privacy·회귀 gate와 packet 이관 |

## 완료/미완료/다음 액션

- 완료: C1, C2. 기본 462/minimal 444개, vendor 2개, 실제 relay sync/health,
  8회로 포화·반환·재접속, 2,000회 회로 churn, 전체 native gate 통과.
- 미완료: C3의 packet 이관·마감 검증.
- 다음 액션: 구현 snapshot을 보존하고 소유 문서로 이관한 뒤 packet 제거.
- 검증 증거: 위 명령과 `cargo test p2p_relay_circuit_churn -- --ignored --nocapture`
  통과. source와 검증 범위 대조 완료; live 장기 메모리 관측은 로컬 테스트 범위 밖이다.
