# Dev Playbook (Iteration Takeaways)

이 문서는 Rustory를 반복 개발(스펙 -> TDD -> 구현)할 때, "나중에 크게 터지는 종류"의 시행착오를 줄이기 위한 체크리스트/운영 규칙을 정리한다.

## 이번 이터레이션에서 드러난 패턴 (2026-02-07)

### 1) 기능 스펙만 있고 "운영 성질"이 빠져 있었다
- 예: P2P PeerId가 프로세스 재시작마다 바뀌면, 수동 주소(stage1) 재사용/커서 기반 증분 동기화가 깨지거나 불필요한 풀 리싱크가 발생한다.
- 이건 "기능이 된다/안 된다"가 아니라, **지속성(persistence)** / **운영 UX**의 문제다.

### 2) 테스트가 검증하는 건 "테스트에 적은 것"뿐이다
- 예: tracker list의 `user_id`가 `u1` 같은 단순 문자열만 들어간다는 전제에서 테스트하면, 공백/슬래시/유니코드 등 URL 인코딩 이슈가 남을 수 있다.

### 3) 로컬/루프백으로는 네트워크/프로세스 경계 이슈가 잘 안 잡힌다
- 예: NAT/relay/observed address 품질 같은 것은 "다른 머신/다른 네트워크"에서만 드러난다.
- 따라서 unit/integration만으로는 부족하고, 최소한의 e2e/smoke가 필요하다.

## 교훈을 "규칙"으로 바꾸기

### Spec에 반드시 넣을 것(비기능/운영 요구)
- 지속성: 재시작해도 유지돼야 하는 상태(예: identity, 커서, 키 파일)
- 오프라인/부분 장애: 일부 피어/트래커가 죽어도 동작해야 하는지, partial success 정의
- 입력/프로토콜 경계: 인코딩(URL, JSON), 파싱, 호환성(마이그레이션)
- 관측성: 성공/실패/폴백이 로그로 확인 가능한지(최소 로그 포맷/레벨)
- 보안/멤버십: 키/토큰/권한, 파일 퍼미션, 기본 안전값

### "수용 테스트(acceptance)" 최소 1개를 의무화
unit 테스트가 아니라도 된다. 다음 중 하나면 된다.
- integration 테스트(루프백/임시 서버/임시 DB)
- 재시작 테스트(상태 파일/identity를 디스크에 쓰고 다시 로드)
- smoke 시나리오 문서 + 실행 커맨드(사람이 그대로 따라하면 재현되는 수준)
  - 예: `scripts/smoke_p2p_local.sh` (tracker+relay+p2p-serve 2개+p2p-sync 스모크)
  - 예: `scripts/acceptance_docker_macos_linux.sh` (macOS host + Linux container, relay circuit 확인)
  - 예: `scripts/acceptance_docker_two_peer_relay.sh` (분리된 Docker network 2개에서 relay-only 수렴 확인)

### 노드 이탈/재가입은 membership 변경으로 취급한다
- `rr uninstall --yes`는 현재 머신의 로컬 이탈이다. 관리 중인 daemon/hook/autostart/service, 로컬 DB/config/state, tracker 등록을 정리하지만 이미 다른 peer에 전파된 history row를 되돌리지는 않는다.
- 다른 노드의 membership은 strict enrollment fleet에서 `rr device revoke`로 박탈할 수 있다. 로컬 파일 정리는 대상이 사전 opt-in한 managed daemon만 fixed `rr device retire` ticket을 cooperative helper로 실행하며, 임의 command/path 또는 offline/침해 대상의 강제 삭제로 확장하지 않는다. 상세 rollout과 한계는 `docs/security.md`, `docs/p2p.md`, `docs/daemon.md`를 따른다.
- 이미 전파된 row는 원래 `hostname`, `device_id`, command metadata를 유지한다. 따라서 같은 hostname의 과거 row가 남는 것은 정상이다.
- 비정상 신호는 같은 hostname 또는 같은 `device_id`를 가진 peer가 동시에 active로 보이는 경우다. `rr sync-status --with-tracker`와 watch UI는 5분 이내에 보인 active duplicate를 warning으로 표시해야 한다.
- uninstall 뒤 완전히 새로 join하면 새 `identity.key`와 새 PeerId가 생긴다. 같은 머신을 동일 identity로 이어가려면 uninstall 전에 config/key를 보존하는 운영 절차를 명시적으로 선택해야 한다.
- 같은 hostname으로 재가입한 새 peer가 안정화된 뒤 이전 peer가 계속 active duplicate로 보이면, 이전 노드의 daemon이 아직 살아 있거나 다른 환경에서 같은 config/key를 복제한 것이다. 이 경우 history row 삭제가 아니라 node process/config 정리가 우선이다.

### 버그/이슈를 발견하면: 재현 -> 테스트화 -> 수정
1. 최소 재현 절차를 만든다(테스트 or 스모크 커맨드).
2. 그 절차가 "실패하는 것"을 먼저 확인한다.
3. 수정 후 그 절차가 "항상 통과"하도록 회귀를 막는다.

## 테스트 레이어(권장 사다리)
- Unit: 순수 함수/파서/변환 로직
- Integration(loopback): 로컬 프로세스 내에서 transport roundtrip, SQLite schema/쿼리
- Restart: 파일 기반 상태(identity/키/설정) 저장/복구
- E2E smoke: (가능하면) tracker + relay + 2 peer를 띄워 실제 동기화/폴백/업그레이드 관측

### Ctrl+R 검색 품질 게이트

- ranking 변경은 private shell history를 fixture로 복사하지 않고 source의 synthetic corpus로 Hit@1, Hit@3, MRR, Top-3 도달 입력 수를 검증한다.
- 일반 검색 시나리오는 field query 없이 통과해야 하며, 기존 field/negation/quote 문법은 호환 회귀로 별도 확인한다.
- 10만 건 hot path 예산은 release mode의 ignored benchmark로 확인한다. 검증 명령과 현재 예산은 검색 테스트 이름과 활성 `docs/todo-*` spec을 기준으로 한다.

## Definition of Done (네트워크/동기화 계열 기준)
- 표준 로컬 검증 진입점은 `scripts/check.sh`다.
- 현재 검증 명령 선언은 `docs/REPO_MANIFEST.yaml`에서 확인한다.
- 정확한 cargo 명령, smoke 포함 여부, 옵션 해석은 `scripts/check.sh --help`와 스크립트 본문을 직접 확인한다.
- spec에 결정 사항이 반영되고, `open-questions.md`는 비어 있어야 한다.
- 문서(`docs/`)에 사용법/제약이 반영돼야 한다.

## 배포 바이너리 식별
- 사람이 빠르게 확인할 때는 `rr --version`을 사용한다. 출력에는 package version과 build revision이 함께 들어가야 한다.
- 운영 스크립트나 배포 점검에서는 `rr version --json`을 사용해 `version`, `build_revision`, `build_revision_source`, `build_dirty`를 파싱한다.
- `rr doctor --json`도 동일한 `build` 블록을 포함해야 하며, 장애 triage에서 설정/DB 상태와 바이너리 revision을 같이 확인하는 경로로 쓴다.

### 원커맨드 점검(권장)
- CI와 같은 검증은 `scripts/check.sh`에서 시작한다.
- relay/cross-network까지 포함한 고신뢰 네트워크 검증이나 빠른 반복 옵션이 필요하면 `scripts/check.sh --help`에서 현재 옵션을 확인한다.

## 커밋 메시지 규칙(요약)
- 형식: `<type>: <summary>`
- scope 괄호는 사용하지 않는다. 예: `feat(sync): ...` 형태 금지
- 범위/모듈을 드러내고 싶으면 summary에 포함한다.
- 자세한 규칙은 `AGENTS.md`를 따른다.

## 진행 자동화(대화 규칙)
- 반복적으로 "검토 후 다음 진도"를 물어보지 않기 위해, 기본 진행 루프를 문서화한다.
- 사용자가 `진행해줘`, `추천대로 진행` 같은 요청을 하면 `AGENTS.md`의 "기본 진행 루프(검토 -> 다음 진도)"를 적용한다.
  - 즉: 스펙/테스트/검토/검증/커밋/푸시 후 다음 작업으로 자동 진도(결정이 필요할 때만 질문).

## 다음 작업에서 바로 쓸 수 있는 체크리스트(짧은 버전)
- 스펙에 "재시작/오프라인/인코딩/관측성" 섹션이 있는가?
- 새로운 네트워크 입력(쿼리/헤더/주소)이 추가되었는가? 그러면 특수문자/비정상 입력 테스트가 있는가?
- partial failure(일부 실패) 시 동작이 정의됐는가? 로그로 확인 가능한가?
- 최소 1개의 수용 테스트(재시작 또는 e2e)가 추가됐는가?
- relay/direct 선택 로직 변경이면 `scripts/check.sh --acceptance` 또는 `bash scripts/acceptance_docker_two_peer_relay.sh`로 direct가 막힌 relay-only 경계까지 확인했는가?
