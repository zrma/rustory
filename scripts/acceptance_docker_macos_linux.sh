#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

pick_port() {
  python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
}

need_cmd() {
  local name="$1"
  if ! command -v "$name" >/dev/null 2>&1; then
    echo "error: missing command: $name" >&2
    exit 127
  fi
}

need_cmd docker
need_cmd python3
need_cmd curl

if ! docker compose version >/dev/null 2>&1; then
  echo "error: docker compose v2 is required (docker compose ...)" >&2
  exit 127
fi

ACC_DIR="${RUSTORY_ACCEPTANCE_DIR:-$ROOT_DIR/target/acceptance/docker-macos-linux}"
COMPOSE_FILE="$ROOT_DIR/contrib/docker/acceptance/compose.yml"
PROJECT="rustory-acceptance"
KEEP="${RUSTORY_ACCEPTANCE_KEEP:-0}"

TRACKER_PORT="${RUSTORY_ACCEPTANCE_TRACKER_PORT:-$(pick_port)}"
RELAY_PORT="${RUSTORY_ACCEPTANCE_RELAY_PORT:-$(pick_port)}"
TRACKER_URL="http://127.0.0.1:${TRACKER_PORT}"

USER_ID="${RUSTORY_ACCEPTANCE_USER_ID:-acceptance}"
TOKEN="${RUSTORY_ACCEPTANCE_TRACKER_TOKEN:-acceptance-token}"

cleanup() {
  set +e
  if [[ "$KEEP" == "1" ]]; then
    return 0
  fi
  RUSTORY_ACCEPTANCE_DIR="$ACC_DIR" \
    docker compose -f "$COMPOSE_FILE" -p "$PROJECT" down -v >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "[1/8] prepare acceptance dir: $ACC_DIR"
rm -rf "$ACC_DIR"
mkdir -p "$ACC_DIR"

echo "[2/8] start tracker/relay (docker)"
RUSTORY_ACCEPTANCE_DIR="$ACC_DIR" \
RUSTORY_ACCEPTANCE_TRACKER_PORT="$TRACKER_PORT" \
RUSTORY_ACCEPTANCE_RELAY_PORT="$RELAY_PORT" \
RUSTORY_ACCEPTANCE_TRACKER_TOKEN="$TOKEN" \
docker compose -f "$COMPOSE_FILE" -p "$PROJECT" up -d --build tracker relay >/dev/null

echo "[3/8] wait tracker ready"
TRACKER_READY=0
for _ in $(seq 1 200); do
  if [[ -n "$TOKEN" ]]; then
    if curl -fsS -H "Authorization: Bearer ${TOKEN}" "${TRACKER_URL}/api/v1/ping" >/dev/null 2>&1; then
      TRACKER_READY=1
      break
    fi
  else
    if curl -fsS "${TRACKER_URL}/api/v1/ping" >/dev/null 2>&1; then
      TRACKER_READY=1
      break
    fi
  fi
  sleep 0.05
done
if [[ "$TRACKER_READY" != "1" ]]; then
  echo "error: tracker did not start" >&2
  docker compose -f "$COMPOSE_FILE" -p "$PROJECT" logs --no-color tracker | tail -n 80 >&2 || true
  exit 1
fi

echo "[4/8] get relay peer id"
RELAY_PEER_ID=""
for _ in $(seq 1 200); do
  line="$(docker compose -f "$COMPOSE_FILE" -p "$PROJECT" logs --no-color relay 2>/dev/null | grep 'relay listen:' | head -n 1 || true)"
  if [[ -n "$line" ]]; then
    RELAY_PEER_ID="$(echo "$line" | sed -n 's#.*relay listen: .*/p2p/##p' | tr -d '\r')"
    if [[ -n "$RELAY_PEER_ID" ]]; then
      break
    fi
  fi
  sleep 0.05
done
if [[ -z "$RELAY_PEER_ID" ]]; then
  echo "error: relay peer id not found" >&2
  docker compose -f "$COMPOSE_FILE" -p "$PROJECT" logs --no-color relay | tail -n 120 >&2 || true
  exit 1
fi

RELAY_ADDR="/ip4/127.0.0.1/tcp/${RELAY_PORT}/p2p/${RELAY_PEER_ID}"
echo "relay addr (host): $RELAY_ADDR"

echo "[5/8] start linux peer (docker)"
RELAY_PEER_ID="$RELAY_PEER_ID" \
RUSTORY_ACCEPTANCE_DIR="$ACC_DIR" \
RUSTORY_ACCEPTANCE_TRACKER_PORT="$TRACKER_PORT" \
RUSTORY_ACCEPTANCE_RELAY_PORT="$RELAY_PORT" \
RUSTORY_ACCEPTANCE_USER_ID="$USER_ID" \
RUSTORY_ACCEPTANCE_TRACKER_TOKEN="$TOKEN" \
docker compose -f "$COMPOSE_FILE" -p "$PROJECT" up -d linux-peer >/dev/null

echo "[6/8] wait tracker has peers (linux peer registered)"
ENC_USER_ID="$(python3 - <<'PY' "$USER_ID"
import sys
import urllib.parse
print(urllib.parse.quote(sys.argv[1]))
PY
)"

READY=0
for _ in $(seq 1 200); do
  if [[ -n "$TOKEN" ]]; then
    if curl -fsS -H "Authorization: Bearer ${TOKEN}" "${TRACKER_URL}/api/v1/peers?user_id=${ENC_USER_ID}" 2>/dev/null \
      | python3 -c 'import json,sys
try:
  data = json.load(sys.stdin)
except Exception:
  sys.exit(1)
sys.exit(0 if len(data.get("peers", [])) >= 1 else 1)'; then
      READY=1
      break
    fi
  else
    if curl -fsS "${TRACKER_URL}/api/v1/peers?user_id=${ENC_USER_ID}" 2>/dev/null \
      | python3 -c 'import json,sys
try:
  data = json.load(sys.stdin)
except Exception:
  sys.exit(1)
sys.exit(0 if len(data.get("peers", [])) >= 1 else 1)'; then
      READY=1
      break
    fi
  fi
  sleep 0.05
done

if [[ "$READY" != "1" ]]; then
  echo "error: linux peer did not register to tracker in time" >&2
  docker compose -f "$COMPOSE_FILE" -p "$PROJECT" logs --no-color linux-peer | tail -n 120 >&2 || true
  exit 1
fi

echo "[7/8] wait relay reservation accepted (linux peer is dialable via relay)"
LINUX_PEER_ID=""
if [[ -n "$TOKEN" ]]; then
  LINUX_PEER_ID="$(curl -fsS -H "Authorization: Bearer ${TOKEN}" "${TRACKER_URL}/api/v1/peers?user_id=${ENC_USER_ID}" 2>/dev/null \
    | python3 -c 'import json,sys
data = json.load(sys.stdin)
peers = data.get("peers", [])
print(peers[0]["peer_id"] if peers else "")' || true)"
else
  LINUX_PEER_ID="$(curl -fsS "${TRACKER_URL}/api/v1/peers?user_id=${ENC_USER_ID}" 2>/dev/null \
    | python3 -c 'import json,sys
data = json.load(sys.stdin)
peers = data.get("peers", [])
print(peers[0]["peer_id"] if peers else "")' || true)"
fi
if [[ -z "$LINUX_PEER_ID" ]]; then
  echo "error: failed to resolve linux peer id from tracker" >&2
  exit 1
fi

RESERVED=0
for _ in $(seq 1 200); do
  if docker compose -f "$COMPOSE_FILE" -p "$PROJECT" logs --no-color relay 2>/dev/null \
    | grep -q "relay: reservation accepted: ${LINUX_PEER_ID}"; then
    RESERVED=1
    break
  fi
  sleep 0.05
done
if [[ "$RESERVED" != "1" ]]; then
  echo "error: relay reservation was not accepted in time: peer_id=${LINUX_PEER_ID}" >&2
  docker compose -f "$COMPOSE_FILE" -p "$PROJECT" logs --no-color relay | tail -n 120 >&2 || true
  exit 1
fi

echo "[8/8] run p2p-sync on macOS host"
cargo build --bin rr >/dev/null

MAC_DB="$ACC_DIR/mac.db"
MAC_ENTRY_ID="$(RUSTORY_USER_ID="$USER_ID" \
  RUSTORY_DEVICE_ID="mac" \
  target/debug/rr --db-path "$MAC_DB" record \
    --cmd "echo acceptance-from-mac" \
    --cwd "/tmp" \
    --exit-code 0 \
    --shell "zsh" \
    --hostname "mac" \
    --print-id | tr -d '\r')"
if [[ -z "$MAC_ENTRY_ID" ]]; then
  echo "error: failed to record entry on mac" >&2
  exit 1
fi

RUSTORY_USER_ID="$USER_ID" \
RUSTORY_DEVICE_ID="mac" \
RUSTORY_SWARM_KEY_PATH="$ACC_DIR/swarm.key" \
RUSTORY_TRACKER_TOKEN="$TOKEN" \
target/debug/rr --db-path "$MAC_DB" p2p-sync \
  --trackers "$TRACKER_URL" \
  --relay "$RELAY_ADDR" \
  --push \
  --limit 1000

echo "[verify] mac db pulled linux seed entry"
python3 - <<'PY' "$MAC_DB"
import sqlite3
import sys

db = sys.argv[1]
conn = sqlite3.connect(db)
try:
    n = conn.execute(
        "SELECT COUNT(*) FROM entries WHERE cmd LIKE '%acceptance-from-linux%'"
    ).fetchone()[0]
finally:
    conn.close()

if n <= 0:
    sys.stderr.write("error: expected linux seed entry in mac db\n")
    sys.exit(1)
print(f"linux_seed_entries={n}")
PY

echo "[export] copy linux peer db snapshot"
LINUX_CID="$(docker compose -f "$COMPOSE_FILE" -p "$PROJECT" ps -q linux-peer | tr -d '\r')"
if [[ -z "$LINUX_CID" ]]; then
  echo "error: linux-peer container id not found" >&2
  exit 1
fi

LINUX_DB="$ACC_DIR/linux.db"

# Docker Desktop(macOS) + bind-mount 환경에서는 SQLite WAL 동기화/가시성이 깨질 수 있어
# linux peer DB는 컨테이너 내부 FS(/tmp)에 두고, 여기서 snapshot을 꺼내 검증한다.
#
# KEEP=1(디버깅)에서는 컨테이너를 멈추지 않고(db/wal/shm best-effort copy),
# 기본 모드에서는 stop 후 snapshot을 복사해 일관성을 높인다.
if [[ "$KEEP" != "1" ]]; then
  docker compose -f "$COMPOSE_FILE" -p "$PROJECT" stop linux-peer >/dev/null 2>&1 || true
fi

docker cp "${LINUX_CID}:/tmp/linux.db" "$LINUX_DB" >/dev/null
docker cp "${LINUX_CID}:/tmp/linux.db-wal" "${LINUX_DB}-wal" >/dev/null 2>&1 || true
docker cp "${LINUX_CID}:/tmp/linux.db-shm" "${LINUX_DB}-shm" >/dev/null 2>&1 || true

echo "[verify] linux db received mac pushed entry"
python3 - <<'PY' "$MAC_ENTRY_ID" "$LINUX_DB"
import sqlite3
import sys

entry_id = sys.argv[1]
db = sys.argv[2]
conn = sqlite3.connect(db)
try:
    n = conn.execute("SELECT COUNT(*) FROM entries WHERE entry_id = ?", (entry_id,)).fetchone()[0]
finally:
    conn.close()

if n <= 0:
    sys.stderr.write(f"error: expected mac pushed entry in linux db: entry_id={entry_id}\n")
    sys.exit(1)
print("ok")
PY

echo "[verify] relay fallback was used (circuit accepted)"
if ! docker compose -f "$COMPOSE_FILE" -p "$PROJECT" logs --no-color relay | grep -q "relay: circuit accepted:"; then
  echo "error: relay circuit accepted log not found (direct dial may have succeeded unexpectedly)" >&2
  docker compose -f "$COMPOSE_FILE" -p "$PROJECT" logs --no-color relay | tail -n 120 >&2 || true
  exit 1
fi

echo "ok"
