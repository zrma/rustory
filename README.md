# Rustory

분산/동기화 기반 셸 히스토리 관리 도구 (MVP 설계 단계)

## 목표 (MVP)
- append-only 기반 히스토리 수집
- entry_id 기반 dedup 동기화
- fzf 기반 로컬 검색 UI
- bash/zsh 후킹

## 현재 상태
- 설계 문서: `docs/mvp.md`
- PoC 구현 진행 중:
  - SQLite 스토리지(ingest_seq cursor, dedup)
  - pull 기반 sync 코어 루프
  - HTTP(디버그) `serve`/`sync` (pull + optional push: `rr sync --push`, push는 로컬 device_id 엔트리만 전송)
  - P2P `p2p-serve`/`p2p-sync`
    - 단계 1: 수동 multiaddr
    - 단계 2: tracker/relay(디스커버리 + 중계) + PSK(pnet) + direct-first + relay fallback + hole punching(DCUtR) (문서: `docs/p2p.md`)
  - tracker/relay 서버
    - `tracker-serve` (HTTP)
    - `relay-serve` (libp2p circuit relay v2)
  - 로컬 기록용 `record` + fzf 기반 `search`
  - bash/zsh hook 스크립트 생성(`hook`)

## 다음 단계
- push 기반 동기화(옵션: `p2p-sync --push`, 로컬 device_id 엔트리만 전송) 고도화(압축/배치 튜닝 등)
- 주기 동기화(옵션: `p2p-sync --watch`)를 데몬/스케줄러로 정식화(launchd/systemd 등)
- 설정/키 관리 UX 개선(PSK fingerprint 출력, 키 배포 문서화 등)
- 로컬/CI 수용 테스트(스모크) 정비

## 개발
- 테스트: `cargo test`
- 로컬 실행: `cargo run --bin rr -- --help`
- 로컬 P2P 스모크: `scripts/smoke_p2p_local.sh`
- 백그라운드 실행(launchd/systemd): `docs/daemon.md`

## 사용(로컬 PoC)
### zsh
```sh
source <(rr hook --shell zsh)
```

### bash
```sh
source <(rr hook --shell bash)
```

추가 옵션/환경 변수는 `docs/hook.md` 참고.

### 수동 기록/검색
```sh
rr record --cmd "echo hello" --cwd "$PWD" --exit-code 0 --shell zsh
rr search --limit 100000
```

### DB 경로 오버라이드(옵션)
```sh
rr --db-path "/tmp/rustory.db" record --cmd "echo hello" --cwd "$PWD" --shell zsh
rr --db-path "/tmp/rustory.db" search --limit 10
```

### P2P 동기화(단계 1: 수동 multiaddr)
```sh
# peer A (서버 역할)
rr --db-path "/tmp/rustory-a.db" p2p-serve --listen /ip4/0.0.0.0/tcp/8845

# peer B (클라이언트 역할): peer A가 출력한 /p2p/<peer_id> 포함 주소를 그대로 넣는다.
rr --db-path "/tmp/rustory-b.db" p2p-sync --peers "/ip4/127.0.0.1/tcp/8845/p2p/<peer_id>" --limit 1000
```

자세한 단계 2(tracker/relay + PSK) 사용법은 `docs/p2p.md` 참고.
