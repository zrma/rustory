# Rustory MVP Spec (append + dedup + fzf)

## 배경/동기
- atuin: 기능이 많고 무거워서 운영/관리 부담이 큼
- hishtory: 동기화 안정성이 떨어져 신뢰가 어렵다고 판단
- 목표: **안정적인 동기화** + **가벼운 클라이언트/서버**

## 핵심 목표
- 안정적 동기화 (오프라인/재시도/중복 제거에 강함)
- 가벼운 시스템 (최소 기능만 제공)
- ctrl+r 기반의 빠른 fzf 검색 UX
- CLI 커맨드: `rr`
- 바이너리 구성: 단일 바이너리
- 로컬 DB: SQLite
- 라이브러리 우선 활용 (핵심 로직만 직접 구현)

## 운영/스케일 가정
- 단일 사용자, 다중 디바이스(약 8~10대)
- 디바이스당 10만+ 엔트리, 전체 수십만~100만 엔트리 규모
- 디바이스는 수시로 오프라인/온라인 전환
- 느슨한 동기화 허용(글로벌 순서 보장 X), 최종 일관성 지향

## 목표
- 각 디바이스에서 히스토리를 append-only로 수집
- entry_id 기반 dedup 동기화
- fzf 기반 로컬 검색 UI 제공
- bash/zsh 훅으로 자동 수집

## 범위 (MVP)
- 피어: 단순 저장/조회 API
- 클라이언트: 로컬 큐 + 피어 동기화 + fzf UI
- 삭제/정리: MVP에서는 제외 (후속)
- 정확한 글로벌 순서: 보장하지 않음 (timestamp 정렬)

## 배포 모드 (PoC/MVP)
- Hybrid P2P 모드(유일)
  - 각 디바이스: peer(서버+클라이언트) + 로컬 DB
  - k8s 서버: tracker(서비스 디스커버리) + relay(중계)
  - tracker/relay는 “항상 온라인”이 아니어도 된다
    - 다운 시 신규 피어 발견/중계가 막혀 sync가 지연될 수 있음
    - 각 디바이스 로컬 DB가 append-only 소스 오브 트루스이므로 데이터 유실은 없음
  - dedup: entry_id 유니크 인덱스로 보장

## 아키텍처 개요
- 클라이언트는 로컬 DB에 append-only 저장
- peer는 tracker에 등록하고, tracker에서 peer 목록을 받아 여러 peer와 연결한다
- direct 연결이 가능하면 direct를 시도하고, 불가하면 relay 경유로 통신한다 (PoC는 relay 우선)
- 저장소는 entry_id 유니크 인덱스로 dedup
- peer 간 동기화는 “append + dedup + cursor”로 점진적으로 수렴한다

## 데이터 모델 (공통)
필드 (최소):
- entry_id: UUID (client-generated)
- device_id: string
- user_id: string
- ts: RFC3339 timestamp (client time)
- cmd: string
- cwd: string
- exit_code: int
- duration_ms: int
- shell: string
- hostname: string
- version: string

## Transport / 프로토콜 (초안)
### P2P (PoC 기본)
- libp2p 기반의 peer-to-peer 통신
- k8s 서버는 tracker(디스커버리) + relay(중계) 역할을 수행
- PoC는 “relay 우선”으로 단순화하되, relay 경유 연결이 수립된 경우에는 dcutr(DCUtR)로 direct 업그레이드(hole punching)를 시도한다(실패해도 relay로 계속 진행).
- 동기화 메시지(개념):
  - `SyncPull { cursor, limit } -> SyncBatch { entries, next_cursor }`
  - `EntriesPush { entries } -> PushAck { inserted, ignored }` (옵션)
- 개발 순서(권장): 먼저 HTTP(디버그)로 end-to-end 동작을 검증한 뒤, transport만 P2P로 치환한다

### HTTP (옵션/디버그)
P2P 개발/디버깅이 어려운 환경을 대비하여, HTTP transport를 보조 수단으로 둘 수 있다.
- POST /api/v1/entries
  - body: `[Entry]` 또는 `{ "entries": [Entry] }`
  - 동작: entry_id 기준 upsert/ignore (idempotent)
- GET /api/v1/entries
  - query: cursor=<cursor>, limit=<n> (기본: cursor=0, limit=1000)
  - 동작: cursor 이후 배치 반환 (cursor는 피어 기준 ingest_seq)
  - response: `{ "entries": [Entry], "next_cursor": <cursor|null> }`
- GET /api/v1/ping

## 클라이언트 동기화
- 기본은 pull 기반으로 단순화한다.
  - 다운로드(pull): `cursor` 기반으로 “피어 기준 ingest_seq” 이후 배치를 반복 요청
  - 업로드(push): PoC에서는 선택 사항(없어도 pull로 수렴 가능)
- 병합: entry_id 기준 dedup
- UI 정렬: `ts + device_id + entry_id`

## 로컬 저장
- 로컬 DB 파일 (예: ~/.rustory/history.db)
- entries 테이블은 “피어 기준 ingest_seq(단조 증가)” 컬럼을 가진다
- 인덱스: entry_id (unique), ts, device_id, ingest_seq
- peer_state 테이블에 peer별 last_cursor를 저장한다

## fzf UI (ctrl+r)
- ctrl+r에서 fzf UI 호출
- 로컬 DB에서 최근 N개 로드 (기본 100k)
- 선택된 커맨드를 프롬프트에 삽입
- 대용량 대비: prefix/최근 N 제한 유지

## bash/zsh 훅
- precmd/PROMPT_COMMAND로 마지막 커맨드 캡처
- 비동기 업로드(네트워크 실패 시 큐 유지)

## 비기능 요구사항
- 오프라인 동작
- idempotent 동기화
- 작은 서버 부담

## 구현 원칙
- 통신/스토리지/CLI 등은 가능한 한 라이브러리로 해결
- 동기화 프로토콜(append+dedup)과 데이터 모델은 직접 구현
- Transport 레이어를 추상화하여 향후 P2P 확장 가능하게 설계

## 모듈 구조 (초안)
- core: 데이터 모델, entry_id 생성, 정렬/병합, dedup
- storage: SQLite 접근 (배치 insert, 인덱스 관리)
- transport: 인터페이스 + 구현체
  - p2p: tracker(디스커버리) + relay(중계) + peer sync (PoC/MVP 기본)
  - http: 옵션/디버그용 transport (필요 시)
- cli: `rr` 커맨드/서브커맨드
- hook: bash/zsh 훅 템플릿

---

## 결정: 동기화 커서 = ingest_seq (피어 기준)
`since=<timestamp>`는 클라이언트 시계 불일치로 누락이 생길 수 있으므로, PoC부터 피어 기준 커서를 사용한다.
- 각 peer는 entries 테이블에 `ingest_seq`(정수, 단조 증가)를 부여한다
- 클라이언트는 peer별로 `last_cursor`를 저장한다
- pull 시나리오:
  - 요청: `cursor=<last_cursor>&limit=<n>`
  - 응답: `ingest_seq > cursor`인 엔트리들을 오름차순으로 반환 + `next_cursor`
- 장점:
  - 클라이언트 시계가 틀려도 누락이 크게 줄어듦
  - idempotent(중복 요청/재시도)해도 entry_id dedup로 안전

## 결정: P2P PoC = tracker(디스커버리) + relay(중계)
사용자가 운영하는 k8s 서버를 매개로 “트래커처럼” 피어들을 연결하고, 직접 연결이 어려우면 중계한다.
- tracker/relay 다운 시:
  - 신규 발견/중계가 중단되어 sync가 지연될 수 있음
  - 로컬 DB가 append-only이므로 데이터 유실은 없음
- peerbook 캐시:
  - 마지막으로 본 peer 주소를 로컬에 캐시하여 tracker 없이도 direct dial을 시도할 수 있다

## 결정: 보안/멤버십 (PoC)
- 전송 보안: libp2p의 암호화 채널(예: Noise)을 전제로 한다
- 멤버십 제어: 공유 시크릿 기반의 private network(예: swarm_key/PSK)를 권장한다
- E2EE(저장 암호화), 계정/로그인 시스템은 MVP에서 제외한다

## 결정: 설정/배포 (PoC)
- 설정 파일: `~/.config/rustory/config.toml`
- 주요 항목(초안):
  - `db_path`
  - `device_id`
  - `user_id`
  - `trackers = ["<multiaddr-or-host:port>", ...]`
  - `swarm_key_path` (private network 사용 시)
  - `search_limit_default`

## 결정: entry_id 생성
- `entry_id`는 클라이언트에서 UUIDv4로 생성한다
- 목적: 전역 유니크 키로 dedup/idempotency를 보장
