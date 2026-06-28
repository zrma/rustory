# Hishtory Migration Runbook

이 문서는 Hishtory가 이미 설치된 머신에서 Rustory로 로컬 히스토리를 seed하고, P2P sync로 점진 이관하는 절차를 정리한다.
현재 CLI 옵션과 default는 `rr import --help`, `rr init --help`, `rr p2p-sync --help`, `rr sync-status --help`, `src/cli.rs`, `src/history_import.rs`를 직접 확인한다.

## 범위

- Rustory는 Hishtory public sync 장애를 복구하지 않는다.
- `rr import --shell hishtory`는 로컬 Hishtory SQLite DB를 read-only로 읽어 Rustory 로컬 DB에 추가한다.
- 기본 Hishtory DB 경로는 `~/.hishtory/.hishtory.db`이다. 다른 경로는 `--path`로 지정한다.
- import는 additive/idempotent다. 같은 source를 다시 import하면 이미 들어간 row는 `ignored`로 집계된다.

## 이관 전 확인

각 머신에서 먼저 Rustory 설정과 P2P 키를 고정한다.

```sh
rr init \
  --user-id "<same-user-id>" \
  --device-id "<unique-device-id>" \
  --trackers "http://<tracker-host>:8850" \
  --relay "/ip4/<relay-ip>/tcp/4001/p2p/<relay_peer_id>" \
  --tracker-token "secret"

rr doctor
rr swarm-key
```

주의할 값:
- `user_id`: 같은 Rustory 사용자 클러스터에 넣을 머신들은 같은 값을 쓴다.
- `device_id`: 머신마다 고유해야 한다.
- `swarm.key`: 같은 private swarm 안의 머신들은 같은 fingerprint여야 한다.
- `record_ignore_regex`: token, password, secret 같은 민감 명령 제외 규칙을 import 전에 설정한다.

예:

```sh
export RUSTORY_RECORD_IGNORE_REGEX='(?i)(password|token|secret|authorization:|bearer )'
```

## 안전 스모크

실제 Rustory DB에 쓰기 전에 임시 DB로 작은 범위를 먼저 가져온다.

```sh
rr --db-path /tmp/rustory-hishtory-smoke.db import --shell hishtory --limit 1000
rr --db-path /tmp/rustory-hishtory-smoke.db doctor
```

출력에서 `inserted`, `skipped`, `ignored`를 확인한다.
- `inserted`: 새 Rustory entry로 들어간 row 수
- `skipped`: 빈 명령, `rr` 자체 명령, `record_ignore_regex`에 걸린 row 수
- `ignored`: deterministic entry id 기준 중복으로 이미 있던 row 수

## 실제 import

Hishtory 전체 로컬 DB를 현재 Rustory DB에 가져온다.

```sh
rr import --shell hishtory
```

필요하면 명시 경로와 limit를 쓴다.

```sh
rr import --shell hishtory --path ~/.hishtory/.hishtory.db
rr import --shell hishtory --limit 100000
```

Rustory가 보존하는 값:
- command
- working directory
- exit code
- start time
- duration
- source hostname

Rustory가 migration 목적에 맞게 재해석하는 값:
- Rustory `device_id`는 현재 머신의 Rustory device id로 저장한다. 그래야 `rr p2p-sync --push`가 import된 entry를 peer로 보낼 수 있다.
- Hishtory `entry_id`가 있으면 Rustory deterministic entry id의 source key로 사용한다. 같은 Hishtory row를 여러 머신에서 import해도 P2P 수신 측에서 중복을 `ignored`로 처리할 수 있게 하기 위해서다.
- Hishtory `entry_id`가 없는 오래된 row는 Hishtory의 원본 composite fields(`local_username`, `hostname`, `command`, `current_working_directory`, `home_directory`, `exit_code`, `start_time`, `end_time`, `device_id`)를 fallback source key로 쓴다. SQLite `rowid`나 현재 Rustory device id는 fallback key에 넣지 않는다.

## P2P 클러스터로 전파

import 후 각 머신에서 `p2p-serve`와 `p2p-sync --watch --push`를 함께 실행한다.
`p2p-serve`는 이 머신을 tracker에 등록하고 inbound pull/push를 받는 서버 역할이고,
`p2p-sync`는 주기적으로 다른 peer에서 pull하고 현재 디바이스 entry를 push하는 클라이언트 루프다.

```sh
rr p2p-serve
```

```sh
rr p2p-sync --watch --interval-sec 60 --start-jitter-sec 10 --push
```

상태 확인:

```sh
rr doctor
rr sync-status --with-tracker
rr sync-status --json --with-tracker
```

처음 이관할 때는 한 머신을 먼저 import + push하고, 두 번째 머신을 import + push한 뒤 `ignored`가 증가하는지 확인한다.
이 값은 중복 source가 같은 entry id로 수렴하고 있다는 신호다.

## 운영 전환 순서

1. tracker/relay를 먼저 띄우고 identity key와 swarm key를 고정한다.
2. 한 머신에서 `rr init`, temp DB smoke, real import, `p2p-serve`, `p2p-sync --push`를 수행한다.
3. 두 번째 머신에서 같은 절차를 수행하고 tracker 등록, `sync-status`, `ignored` 카운트를 확인한다.
4. 나머지 머신을 같은 방식으로 추가한다.
5. 각 머신의 shell hook을 Rustory로 전환한다.
6. 충분한 soak 기간 동안 Hishtory는 제거하지 말고 read-only fallback source로 남긴다.
7. Rustory `doctor`, `sync-status`, P2P 로그가 안정적이면 Hishtory hook/daemon을 비활성화한다.

## 알려진 경계

- import는 삭제 이력이나 tombstone을 옮기지 않는다.
- Hishtory DB가 실행 중인 Hishtory 프로세스에 의해 갱신 중이면 import는 읽는 시점의 SQLite snapshot 기준으로 수행된다.
- 서로 다른 `user_id`로 import하면 Hishtory `entry_id`가 같아도 Rustory entry id가 달라진다.
- 민감 명령을 이미 import한 뒤에는 현재 Rustory에 별도 bulk redact/delete 명령이 없다. 실사용 import 전 `record_ignore_regex`와 temp DB smoke를 먼저 확인한다.

## 검증 경로

개발 검증:

```sh
cargo test history_import --workspace
cargo test import_ --workspace
scripts/check.sh --fast
```

P2P 경계 검증:

```sh
scripts/smoke_p2p_local.sh
scripts/check.sh --fast --acceptance
```
