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

## 배포 모드 (MVP)
- Mesh 모드(유일): 모든 노드가 서버+클라이언트 역할
  - 로컬 DB 사용
  - entry_id 유니크 인덱스로 dedup

## 아키텍처 개요
- 클라이언트는 로컬 DB에 append-only 저장
- 주기적으로 피어에 배치 업로드
- 저장소는 entry_id 유니크 인덱스로 dedup
- 클라이언트는 API로 recent entries를 다운로드하여 로컬 DB에 병합

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

## 피어 API (초안)
- POST /api/v1/entries
  - body: [Entry]
  - 동작: entry_id 기준 upsert/ignore
- GET /api/v1/entries
  - query: since=<timestamp>, limit=<n>, device_id(optional)
  - 동작: since 이후 최신순 반환
- GET /api/v1/ping

## 클라이언트 동기화
- 업로드: 로컬 큐에서 배치로 POST
- 다운로드: GET /api/v1/entries?since=<last_sync_ts>
- 병합: entry_id 기준 dedup
- 정렬: ts + device_id + entry_id

## 로컬 저장
- 로컬 DB 파일 (예: ~/.rustory/history.db)
- 인덱스: entry_id (unique), ts, device_id

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
  - http: 피어 API 기반 동기화 (MVP)
  - p2p: 후속(libp2p/webrtc 등)용 스텁
- cli: `rr` 커맨드/서브커맨드
- hook: bash/zsh 훅 템플릿
