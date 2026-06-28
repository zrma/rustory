#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
# shellcheck source=scripts/lib/rust-toolchain.sh
source "$ROOT_DIR/scripts/lib/rust-toolchain.sh"

rustory_require_cargo

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

ACC_DIR="${RUSTORY_ACCEPTANCE_DIR:-$ROOT_DIR/target/acceptance/docker-two-peer-relay}"
PROJECT="${RUSTORY_ACCEPTANCE_PROJECT:-rustory-two-peer-relay}"
IMAGE="${RUSTORY_ACCEPTANCE_IMAGE:-rustory-two-peer-relay:local}"
KEEP="${RUSTORY_ACCEPTANCE_KEEP:-0}"

TRACKER_PORT="${RUSTORY_ACCEPTANCE_TRACKER_PORT:-$(pick_port)}"
TRACKER_URL="http://127.0.0.1:${TRACKER_PORT}"
USER_ID="${RUSTORY_ACCEPTANCE_USER_ID:-acceptance}"
TOKEN="${RUSTORY_ACCEPTANCE_TRACKER_TOKEN:-acceptance-token}"

NET_A="${PROJECT}-a-net"
NET_B="${PROJECT}-b-net"
TRACKER="${PROJECT}-tracker"
RELAY="${PROJECT}-relay"
PEER_A="${PROJECT}-peer-a"
PEER_B="${PROJECT}-peer-b"

cleanup_names() {
  set +e
  docker rm -f "$PEER_A" "$PEER_B" "$RELAY" "$TRACKER" >/dev/null 2>&1 || true
  docker network rm "$NET_A" "$NET_B" >/dev/null 2>&1 || true
}

cleanup() {
  if [[ "$KEEP" == "1" ]]; then
    return 0
  fi
  cleanup_names
}
trap cleanup EXIT

fetch_peer_json() {
  curl -fsS -H "Authorization: Bearer ${TOKEN}" \
    "${TRACKER_URL}/api/v1/peers?user_id=${ENC_USER_ID}"
}

peer_id_for_device() {
  local device="$1"
  python3 - "$ACC_DIR/peers.json" "$device" <<'PY'
import json
import sys

path, device = sys.argv[1], sys.argv[2]
with open(path, "r", encoding="utf-8") as f:
    data = json.load(f)

for peer in data.get("peers", []):
    meta = peer.get("meta") or {}
    if meta.get("device_id") == device:
        print(peer["peer_id"])
        raise SystemExit(0)

raise SystemExit(1)
PY
}

wait_log_contains() {
  local container="$1"
  local pattern="$2"
  local label="$3"

  for _ in $(seq 1 300); do
    if docker logs "$container" 2>&1 | grep -Fq "$pattern"; then
      return 0
    fi
    sleep 0.1
  done

  echo "error: timed out waiting for ${label}: ${pattern}" >&2
  docker logs "$container" 2>&1 | tail -n 160 >&2 || true
  exit 1
}

count_relay_circuits() {
  docker logs "$RELAY" 2>&1 | awk '/relay: circuit accepted:/ {n++} END {print n + 0}'
}

start_peer() {
  local container="$1"
  local device="$2"
  local network="$3"

  docker run -d \
    --name "$container" \
    --network "$network" \
    --network-alias "$device" \
    -e "DEVICE=$device" \
    -e "DB=/tmp/${device}.db" \
    -e "RELAY_PEER_ID=$RELAY_PEER_ID" \
    -e "RUSTORY_USER_ID=$USER_ID" \
    -e "RUSTORY_DEVICE_ID=$device" \
    -e "RUSTORY_TRACKER_TOKEN=$TOKEN" \
    -v "$ACC_DIR:/data" \
    "$IMAGE" \
    sh -lc '
      set -eu

      run_and_record() {
        cmd="$1"
        status=0
        sh -lc "$cmd" >/tmp/last-command.out 2>/tmp/last-command.err || status=$?
        rr --db-path "$DB" record \
          --cmd "$cmd" \
          --cwd "/tmp" \
          --exit-code "$status" \
          --shell "bash" \
          --hostname "$DEVICE" \
          --print-id >/dev/null
      }

      run_and_record "echo ${DEVICE}-one"
      run_and_record "pwd"
      run_and_record "false"

      relay_ip="$(getent hosts relay | awk '"'"'{print $1; exit}'"'"')"
      test -n "$relay_ip"

      rr --db-path "$DB" p2p-serve \
        --listen /ip4/0.0.0.0/tcp/0 \
        --swarm-key /data/swarm.key \
        --identity-key "/data/${DEVICE}.identity.key" \
        --trackers http://tracker:8850 \
        --relay "/ip4/${relay_ip}/tcp/4001/p2p/${RELAY_PEER_ID}"
    ' >/dev/null
}

run_sync() {
  local container="$1"
  local device="$2"

  docker exec \
    -e "RUSTORY_USER_ID=$USER_ID" \
    -e "RUSTORY_DEVICE_ID=$device" \
    -e "RUSTORY_TRACKER_TOKEN=$TOKEN" \
    -e "RUSTORY_SWARM_KEY_PATH=/data/swarm.key" \
    "$container" \
    sh -lc '
      set -eu
      relay_ip="$(getent hosts relay | awk '"'"'{print $1; exit}'"'"')"
      test -n "$relay_ip"
      rr --db-path "$DB" p2p-sync \
        --trackers http://tracker:8850 \
        --relay "/ip4/${relay_ip}/tcp/4001/p2p/${RELAY_PEER_ID}" \
        --push \
        --limit 1000
    '
}

echo "[1/11] prepare acceptance dir: $ACC_DIR"
cleanup_names
rm -rf "$ACC_DIR"
mkdir -p "$ACC_DIR"

echo "[2/11] build acceptance image: $IMAGE"
docker build -t "$IMAGE" -f contrib/docker/acceptance/Dockerfile . >/dev/null

echo "[3/11] create isolated peer networks"
docker network create "$NET_A" >/dev/null
docker network create "$NET_B" >/dev/null

echo "[4/11] start tracker on both networks"
docker run -d \
  --name "$TRACKER" \
  --network "$NET_A" \
  --network-alias tracker \
  -p "127.0.0.1:${TRACKER_PORT}:8850" \
  "$IMAGE" \
  rr tracker-serve --bind 0.0.0.0:8850 --ttl-sec 120 --token "$TOKEN" >/dev/null
docker network connect --alias tracker "$NET_B" "$TRACKER" >/dev/null

ENC_USER_ID="$(python3 - <<'PY' "$USER_ID"
import sys
import urllib.parse
print(urllib.parse.quote(sys.argv[1]))
PY
)"

TRACKER_READY=0
for _ in $(seq 1 300); do
  if curl -fsS -H "Authorization: Bearer ${TOKEN}" "${TRACKER_URL}/api/v1/ping" >/dev/null 2>&1; then
    TRACKER_READY=1
    break
  fi
  sleep 0.1
done
if [[ "$TRACKER_READY" != "1" ]]; then
  echo "error: tracker did not start" >&2
  docker logs "$TRACKER" 2>&1 | tail -n 120 >&2 || true
  exit 1
fi

echo "[5/11] start relay on both networks"
docker run -d \
  --name "$RELAY" \
  --network "$NET_A" \
  --network-alias relay \
  -v "$ACC_DIR:/data" \
  "$IMAGE" \
  rr relay-serve \
    --listen /ip4/0.0.0.0/tcp/4001 \
    --swarm-key /data/swarm.key \
    --identity-key /data/relay.key >/dev/null
docker network connect --alias relay "$NET_B" "$RELAY" >/dev/null

RELAY_PEER_ID=""
for _ in $(seq 1 300); do
  line="$(docker logs "$RELAY" 2>&1 | grep 'relay listen:' | head -n 1 || true)"
  if [[ -n "$line" ]]; then
    RELAY_PEER_ID="$(echo "$line" | sed -n 's#.*relay listen: .*/p2p/##p' | tr -d '\r')"
    if [[ -n "$RELAY_PEER_ID" ]]; then
      break
    fi
  fi
  sleep 0.1
done
if [[ -z "$RELAY_PEER_ID" ]]; then
  echo "error: relay peer id not found" >&2
  docker logs "$RELAY" 2>&1 | tail -n 160 >&2 || true
  exit 1
fi
echo "relay peer id: $RELAY_PEER_ID"

echo "[6/11] start peer-a and peer-b on separate networks"
start_peer "$PEER_A" "peer-a" "$NET_A"
start_peer "$PEER_B" "peer-b" "$NET_B"

echo "[7/11] verify peers are not directly discoverable by Docker DNS"
if docker exec "$PEER_A" getent hosts peer-b >/dev/null 2>&1; then
  echo "error: peer-a can resolve peer-b directly; network isolation is broken" >&2
  exit 1
fi
if docker exec "$PEER_B" getent hosts peer-a >/dev/null 2>&1; then
  echo "error: peer-b can resolve peer-a directly; network isolation is broken" >&2
  exit 1
fi

echo "[8/11] wait tracker registrations and relay reservations"
TRACKER_READY=0
for _ in $(seq 1 300); do
  if fetch_peer_json >"$ACC_DIR/peers.json" 2>/dev/null \
    && python3 - "$ACC_DIR/peers.json" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as f:
    data = json.load(f)

devices = {
    (peer.get("meta") or {}).get("device_id")
    for peer in data.get("peers", [])
}
raise SystemExit(0 if {"peer-a", "peer-b"} <= devices else 1)
PY
  then
    TRACKER_READY=1
    break
  fi
  sleep 0.1
done
if [[ "$TRACKER_READY" != "1" ]]; then
  echo "error: tracker did not receive both peer registrations" >&2
  docker logs "$PEER_A" 2>&1 | tail -n 120 >&2 || true
  docker logs "$PEER_B" 2>&1 | tail -n 120 >&2 || true
  exit 1
fi

PEER_A_ID="$(peer_id_for_device peer-a)"
PEER_B_ID="$(peer_id_for_device peer-b)"
wait_log_contains "$RELAY" "relay: reservation accepted: ${PEER_A_ID}" "peer-a reservation"
wait_log_contains "$RELAY" "relay: reservation accepted: ${PEER_B_ID}" "peer-b reservation"

echo "[9/11] run bidirectional p2p-sync through tracker + relay"
CIRCUITS_BEFORE="$(count_relay_circuits)"
for _ in $(seq 1 3); do
  run_sync "$PEER_A" "peer-a"
  run_sync "$PEER_B" "peer-b"
done
CIRCUITS_AFTER="$(count_relay_circuits)"

if (( CIRCUITS_AFTER <= CIRCUITS_BEFORE )); then
  echo "error: relay circuit count did not increase during sync" >&2
  echo "before=${CIRCUITS_BEFORE} after=${CIRCUITS_AFTER}" >&2
  docker logs "$RELAY" 2>&1 | tail -n 200 >&2 || true
  exit 1
fi
echo "relay_circuits_before=${CIRCUITS_BEFORE} relay_circuits_after=${CIRCUITS_AFTER}"

echo "[10/11] snapshot peer databases"
docker stop "$PEER_A" "$PEER_B" >/dev/null
docker cp "${PEER_A}:/tmp/peer-a.db" "$ACC_DIR/peer-a.db" >/dev/null
docker cp "${PEER_B}:/tmp/peer-b.db" "$ACC_DIR/peer-b.db" >/dev/null

echo "[11/11] verify both peers converged"
python3 - <<'PY' "$ACC_DIR/peer-a.db" "$ACC_DIR/peer-b.db"
import sqlite3
import sys

expected = {
    ("peer-a", "echo peer-a-one"),
    ("peer-a", "pwd"),
    ("peer-a", "false"),
    ("peer-b", "echo peer-b-one"),
    ("peer-b", "pwd"),
    ("peer-b", "false"),
}

for db_path in sys.argv[1:]:
    conn = sqlite3.connect(db_path)
    try:
        rows = set(
            conn.execute(
                "SELECT hostname, cmd FROM entries WHERE hostname IN ('peer-a', 'peer-b')"
            ).fetchall()
        )
        counts = dict(
            conn.execute(
                "SELECT hostname, COUNT(*) FROM entries WHERE hostname IN ('peer-a', 'peer-b') GROUP BY hostname"
            ).fetchall()
        )
    finally:
        conn.close()

    if rows != expected:
        missing = sorted(expected - rows)
        extra = sorted(rows - expected)
        raise SystemExit(f"{db_path}: unexpected rows missing={missing} extra={extra}")
    if counts != {"peer-a": 3, "peer-b": 3}:
        raise SystemExit(f"{db_path}: unexpected per-host counts {counts}")

    print(f"{db_path}: ok")
PY

echo "ok"
