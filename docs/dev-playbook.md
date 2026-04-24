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

### 버그/이슈를 발견하면: 재현 -> 테스트화 -> 수정
1. 최소 재현 절차를 만든다(테스트 or 스모크 커맨드).
2. 그 절차가 "실패하는 것"을 먼저 확인한다.
3. 수정 후 그 절차가 "항상 통과"하도록 회귀를 막는다.

## 테스트 레이어(권장 사다리)
- Unit: 순수 함수/파서/변환 로직
- Integration(loopback): 로컬 프로세스 내에서 transport roundtrip, SQLite schema/쿼리
- Restart: 파일 기반 상태(identity/키/설정) 저장/복구
- E2E smoke: (가능하면) tracker + relay + 2 peer를 띄워 실제 동기화/폴백/업그레이드 관측

## Definition of Done (네트워크/동기화 계열 기준)
- `cargo fmt`
- `cargo test`
- `cargo clippy --workspace --all-targets -- -D warnings`
- (권장) `bash ~/.codex/skills/rust-monorepo-maintainer/scripts/workspace_check.sh .`
- spec에 결정 사항이 반영되고, `open-questions.md`는 비어 있어야 한다.
- 문서(`docs/`)에 사용법/제약이 반영돼야 한다.

## 커밋 메시지 규칙(요약)
- 형식: `<type>: <summary>`
- scope 괄호는 사용하지 않는다. 예: `feat(sync): ...` 형태 금지
- 범위/모듈을 드러내고 싶으면 summary에 포함한다.
- 자세한 규칙은 `AGENTS.md`를 따른다.

## 다음 작업에서 바로 쓸 수 있는 체크리스트(짧은 버전)
- 스펙에 "재시작/오프라인/인코딩/관측성" 섹션이 있는가?
- 새로운 네트워크 입력(쿼리/헤더/주소)이 추가되었는가? 그러면 특수문자/비정상 입력 테스트가 있는가?
- partial failure(일부 실패) 시 동작이 정의됐는가? 로그로 확인 가능한가?
- 최소 1개의 수용 테스트(재시작 또는 e2e)가 추가됐는가?
