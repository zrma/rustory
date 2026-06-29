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
- `skipped`: 빈 명령, `record_ignore_regex`에 걸린 row 수
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

## Shell hook handoff

Hishtory에서 Rustory로 실제 전환할 때는 import 성공만으로 끝내지 않는다.
기존 Hishtory hook이 남아 있으면 같은 명령을 두 시스템이 동시에 기록하거나, `Ctrl-R` 검색 키가 로딩 순서에 따라 다시 Hishtory로 넘어갈 수 있다.

전환 순서:

1. Hishtory DB 파일과 `~/.hishtory/` 디렉터리는 fallback source로 보존한다.
2. `~/.zshrc`, `~/.zprofile`, `~/.zshenv`, `~/.zlogin`, `~/.bashrc`, `~/.bash_profile`, `~/.bash_login`, `~/.profile`에서 Hishtory `source`/PATH block을 삭제한다.
3. `/etc/profile`, `/etc/bash.bashrc`, `/etc/profile.d/*.sh`, `/etc/zsh/*` 같은 system profile에 Hishtory hook이 없는지 확인한다.
4. bash만 쓰는 Linux host에 Hishtory 설치 과정에서 생긴 `~/.zshrc`가 있고 내용이 Hishtory block뿐이면, 백업을 남긴 뒤 파일 자체를 삭제해도 된다.
5. Rustory hook block은 shell별 시작 파일에 남긴다. 예: `source <(rr hook --shell zsh)` 또는 `source <(rr hook --shell bash)`.
6. 새 shell을 열거나 `source ~/.zshrc` / `source ~/.bashrc` 후 hook 상태를 확인한다.
7. 마지막으로 `rr p2p-sync --push`와 `rr sync-status --json --with-tracker`를 실행해 `pending_push=0` 수렴을 확인한다.

확인 예:

```sh
rr doctor --json
rr sync-status --json --with-tracker
```

zsh:

```sh
zsh -ic 'bindkey "^R"; print -l -- "preexec_functions:" ${preexec_functions[@]-}; print -l -- "precmd_functions:" ${precmd_functions[@]-}'
```

기대 상태:
- `^R`이 Rustory widget에 바인딩된다.
- `preexec_functions` / `precmd_functions`에 `__rustory_*` hook이 있고 `_hishtory_*` hook은 없다.

bash:

```sh
bash -ic 'command -v rr; bind -X | grep rustory || true; rr doctor --json'
```

기대 상태:
- `rr`가 user-local install 경로에서 발견된다.
- `rr doctor`의 `hook.installed=true`가 나오고 `db status`에 검색 후보 entry가 보인다.
- 새 prompt마다 `[1]+ Done ... rr record ...` 같은 background job completion message가 나오지 않는다.

잔여 Hishtory hook 검색:

```sh
grep -RIn "hishtory" ~/.zshrc ~/.zprofile ~/.zshenv ~/.zlogin ~/.bashrc ~/.bash_profile ~/.bash_login ~/.profile /etc/profile /etc/profile.d /etc/bash.bashrc /etc/zsh 2>/dev/null
```

installer 기반 전환에서는 아래 옵션이 같은 정책을 자동화한다.

```sh
curl -fsSL https://raw.githubusercontent.com/zrma/rustory/main/install/rustory.py | \
  python3 - --token "$RUSTORY_TRACKER_TOKEN" --tracker "<tracker-url>" \
    --install-hook --import-hishtory
```

이 경로는 Hishtory DB/디렉터리는 삭제하지 않고, import 성공 후 user startup files의 Hishtory hook 라인만 제거한다.
Hishtory hook을 임시 유지해야 하는 디버깅 상황에서는 `--keep-hishtory-hooks`를 추가한다.

## 안정화 후 Hishtory 찌꺼기 정리

초기 전환과 삭제는 분리한다.
`rr import --shell hishtory`와 installer의 `--import-hishtory` 경로는 Hishtory DB/디렉터리를 fallback source로 남긴다.
몇 주 동안 Rustory 검색, 기록, tracker/relay sync가 안정적으로 동작한 뒤에만 명시적으로 정리한다.

먼저 삭제 계획만 확인한다.

```sh
rr cleanup-hishtory
```

기본 dry-run은 파일을 지우지 않고 다음 대상을 보여준다.
- `~/.hishtory`
- `~/.config/hishtory`
- `~/.local/bin/hishtory`
- user startup files(`~/.zshrc`, `~/.zprofile`, `~/.zshenv`, `~/.zlogin`, `~/.bashrc`, `~/.bash_profile`, `~/.bash_login`, `~/.profile`) 안의 Hishtory hook 라인

실제 삭제는 archive 경로를 명시한 경우를 기본으로 한다.

```sh
rr cleanup-hishtory --apply --archive-dir ~/SynologyDrive/rustory/hishtory-backups
```

`--archive-dir`를 쓰면 삭제 전에 영향을 받는 Hishtory 디렉터리와 startup file을 `hishtory-backup-<unix>` 디렉터리 아래에 복사한다.
startup file은 Hishtory 관련 라인만 제거하고, 제거 후 공백만 남는 파일은 삭제한다.
따라서 bash만 쓰는 Linux host에 Hishtory 때문에 생긴 `~/.zshrc` 같은 파일은 backup 후 사라진다.

외부 백업을 이미 확보했고 로컬 archive를 만들지 않으려면 아래처럼 명시한다.

```sh
rr cleanup-hishtory --apply --no-archive
```

이 명령은 system profile(`/etc/profile`, `/etc/profile.d`, `/etc/zsh` 등)이나 package manager 설치 상태는 건드리지 않는다.
system-wide Hishtory hook이 남아 있으면 관리자 권한으로 별도 점검한다.

## Multi-machine soak runbook

Hishtory 대체 readiness는 loopback이나 direct-only 성공으로 판정하지 않는다.
최소 soak는 서로 다른 NAT/WiFi/router 뒤에 있는 실제 peer 2대 이상을 tracker + relay로 수렴시키는 것이다.
relay circuit 관측 기준은 `docs/p2p.md`와 Docker relay-only acceptance 문서가 소유하며, 이 문서는 실사용 전환 판정 기준만 둔다.

### 준비 조건

- repo gate: `scripts/check.sh --fast --acceptance`가 green이어야 한다.
- control plane: tracker와 relay를 먼저 띄우고, relay identity key와 swarm key를 영속화한다.
- peer config: 모든 Rustory peer는 같은 `user_id`와 같은 swarm key fingerprint를 사용하고, `device_id`는 머신마다 고유해야 한다.
- import safety: 각 머신에서 temp DB smoke를 먼저 실행하고 `inserted/skipped/ignored`를 확인한 뒤 실제 DB import를 진행한다.
- privacy safety: `record_ignore_regex` 또는 `RUSTORY_RECORD_IGNORE_REGEX`를 import 전에 설정한다. 이미 민감 명령을 import했다면 `rr delete --cmd-regex ... --dry-run`으로 각 peer의 local DB에서 삭제 대상을 먼저 확인한다.

### 2대 기준 절차

1. Peer A에서 `rr init`, `rr doctor --json`, `rr swarm-key`, temp DB smoke, real import를 수행한다.
2. Peer A에서 foreground로 `rr p2p-serve`와 `rr p2p-sync --watch --push`를 각각 띄우고, tracker reachable 상태를 확인한다.
3. Peer B에서 같은 절차를 수행한다. Peer B는 다른 NAT/WiFi/router 뒤에 둔다.
4. 양쪽에서 `rr record --cmd "rustory-soak-<device>-<timestamp>" --print-id`로 canary entry를 1개 이상 만든다.
5. 양쪽 `p2p-sync` 로그에서 pull/push summary가 발생하고, 상대 canary가 `inserted` 또는 중복 수렴 시 `ignored`로 처리되는지 확인한다.
6. relay 로그에서 reservation/circuit이 실제로 발생했는지 확인한다. direct 업그레이드 로그가 있어도 최초 relay circuit 없이 성공한 실행은 cross-NAT readiness 증거로 쓰지 않는다.
7. `rr sync-status --json --with-tracker` 산출물을 양쪽에서 저장하고, tracker reachable과 peer별 pull/push cursor가 전진했는지 확인한다.

### 합격 기준

- 최소 2개 실제 머신에서 `p2p-serve`와 `p2p-sync --watch --push`가 동시에 실행된다.
- 각 peer는 tracker에 등록되고 relay reservation/circuit 경로를 실제로 사용한다.
- 각 peer의 canary entry가 반대편에 수렴한다.
- Hishtory import를 같은 머신에서 다시 실행했을 때 기존 row가 대량 재삽입되지 않고 `ignored`로 수렴한다.
- 24시간 이상 foreground 또는 daemon 로그에 반복적인 tracker unreachable, relay reservation 실패, request timeout 폭증이 없다.
- `rr doctor`와 `rr sync-status --with-tracker`가 전환 전/후 모두 재현 가능한 증거를 남긴다.

### 중단 조건

- tracker/relay가 떠 있는데도 peer가 tracker에 등록되지 않는다.
- relay circuit 없이 direct 후보만으로 수렴한다.
- 같은 Hishtory source를 재import했을 때 예상보다 큰 `inserted`가 반복된다.
- 한 peer의 push summary가 계속 실패하거나 `sync-status` cursor가 전진하지 않는다.
- config, swarm key, device id, tracker token 중 하나라도 머신 간 기준과 다르다.

## 운영 전환 순서

1. tracker/relay를 먼저 띄우고 identity key와 swarm key를 고정한다.
2. 한 머신에서 `rr init`, temp DB smoke, real import, `p2p-serve`, `p2p-sync --push`를 수행한다.
3. 두 번째 머신에서 같은 절차를 수행하고 tracker 등록, `sync-status`, `ignored` 카운트를 확인한다.
4. 나머지 머신을 같은 방식으로 추가한다.
5. 각 머신의 shell hook을 Rustory로 전환한다.
6. 충분한 soak 기간 동안 Hishtory는 제거하지 말고 read-only fallback source로 남긴다.
7. Rustory `doctor`, `sync-status`, P2P 로그가 안정적이면 Hishtory hook/daemon을 비활성화한다. 구체 절차는 `Shell hook handoff`를 따른다.
8. 몇 주간 fallback이 필요 없었다는 것이 확인되면 `rr cleanup-hishtory` dry-run과 archive-backed apply로 Hishtory 찌꺼기를 정리한다.

## 운영 체크리스트

- `rr import --help`, `rr p2p-serve --help`, `rr p2p-sync --help`, `rr sync-status --help`로 현재 CLI surface를 확인했다.
- tracker URL, relay multiaddr, tracker token, swarm key fingerprint, user/device id를 각 머신별로 기록했다.
- `docs/daemon.md`의 preflight를 통과한 뒤 launchd/systemd로 전환했다.
- Hishtory hook/daemon 비활성화 전까지 Rustory와 Hishtory를 dual-run 했다.
- 전환 후에도 Hishtory DB 파일은 read-only fallback source로 보존했다.
- 운영 로그에는 최소한 `rr doctor --json`, `rr sync-status --json --with-tracker`, peer별 p2p summary, relay reservation/circuit 증거가 남아 있다.

## 알려진 경계

- import는 삭제 이력이나 tombstone을 옮기지 않는다.
- Hishtory DB가 실행 중인 Hishtory 프로세스에 의해 갱신 중이면 import는 읽는 시점의 SQLite snapshot 기준으로 수행된다.
- 서로 다른 `user_id`로 import하면 Hishtory `entry_id`가 같아도 Rustory entry id가 달라진다.
- `rr delete`는 sync tombstone이 아니라 local-only 삭제다. 이미 다른 peer로 전파된 민감 row는 각 peer에서 같은 삭제를 수행해야 한다.
- SQLite 파일/WAL에 남은 삭제 흔적까지 줄여야 하면 `rr delete ... --yes --vacuum`을 사용한다.

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
