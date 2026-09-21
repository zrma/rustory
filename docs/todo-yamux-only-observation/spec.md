# Spec: yamux-only-observation

## 배경

- 요청 맥락: 지원 대상 peer의 Yamux 전환 후 mplex 하위 호환을 종료한다.
- 현재 문제/기회: fallback이 남으면 구형 multiplexer의 참조 누적 경로가 계속 사용되며, 집계된 연결 수만으로 사용 프로토콜을 확인할 수 없다.

## 계획 스냅샷

- 목표: 직접/중계 연결 모두 Yamux만 허용하고 relay 프로세스별 협상 누계를 주기적으로 제공한다.
- 범위: muxer 설정/의존성, relay health 통계, 협상 및 실제 relay 회귀 검증, 운영 문서.
- 검증 명령: `scripts/check-release-gates.sh --manifest-mode full --work-id yamux-only-observation`.
- 완료 기준: mplex 의존성과 협상 경로 제거, 이전 Yamux 지원 peer와 연결 성공, mplex-only 협상 양방향 거부, 협상 누계 검증, 실제 relay 동기화 성공.

## 제약과 제외 범위

- 지원 대상은 Yamux를 지원하는 peer다. mplex-only peer와의 연결 실패는 의도된 호환성 종료다.
- PSK/Noise, identity, 데이터와 요청 프로토콜, relay quota를 보존한다. 기기 revoke와 공유 키 폐기는 별개이며 프로토콜 차단을 기기 권한 검증으로 간주하지 않는다.
- 누계는 transport 협상 완료 수다. 현재 활성 연결 수나 mplex 시도 횟수로 해석하지 않는다. 프로세스 재시작 시 초기화한다.
- 업데이트 후 readiness 강화는 설계만 제시한다. 새 릴리스 발행과 운영 서비스 중지는 이 로컬 구현의 완료 기준이 아니다.

## C-체크리스트

| ID | 상태 | Owner | Verify command | 작업 항목 |
| --- | --- | --- | --- | --- |
| C1 | todo | codex | `cargo test p2p_muxer::tests` | Yamux 전용 협상, 이전 전환 peer 호환과 legacy 거부 검증 |
| C2 | todo | codex | `cargo test p2p_relay` | 프로세스별 relay 협상 누계 및 실제 중계 검증 |
| C3 | todo | codex | `scripts/check-release-gates.sh --manifest-mode full --work-id yamux-only-observation` | 문서 이관 및 전체 검증 |

## 완료/미완료/다음 액션

- 완료: 없음.
- 미완료: C1, C2, C3.
- 다음 액션: Yamux 전용 설정과 누계 수집 경계를 구현한다.
- 검증 증거: `scripts/check-todo-readiness.sh docs/todo-yamux-only-observation`, `scripts/check-open-questions-schema.sh --require-closed docs/todo-yamux-only-observation/open-questions.md`.
