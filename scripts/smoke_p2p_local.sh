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

cleanup() {
  set +e
  if [[ -n "${P2P_A_PID:-}" ]]; then kill "$P2P_A_PID" 2>/dev/null; fi
  if [[ -n "${P2P_B_PID:-}" ]]; then kill "$P2P_B_PID" 2>/dev/null; fi
  if [[ -n "${RELAY_PID:-}" ]]; then kill "$RELAY_PID" 2>/dev/null; fi
  if [[ -n "${TRACKER_PID:-}" ]]; then kill "$TRACKER_PID" 2>/dev/null; fi
  if [[ -n "${TMPDIR:-}" ]]; then rm -rf "$TMPDIR" 2>/dev/null; fi
}
trap cleanup EXIT

echo "[1/5] build rr"
cargo build --bin rr >/dev/null

TMPDIR="$(mktemp -d "${TMPDIR:-/tmp}/rustory-smoke.XXXXXX")"
SWARM_KEY="$TMPDIR/swarm.key"

TRACKER_PORT="$(pick_port)"
RELAY_PORT="$(pick_port)"
TRACKER_URL="http://127.0.0.1:${TRACKER_PORT}"

echo "[2/5] start tracker/relay"
target/debug/rr tracker-serve --bind "127.0.0.1:${TRACKER_PORT}" --ttl-sec 60 >"$TMPDIR/tracker.log" 2>&1 &
TRACKER_PID=$!

target/debug/rr relay-serve \
  --listen "/ip4/127.0.0.1/tcp/${RELAY_PORT}" \
  --swarm-key "$SWARM_KEY" \
  --identity-key "$TMPDIR/relay.key" >"$TMPDIR/relay.log" 2>&1 &
RELAY_PID=$!

TRACKER_READY=0
for _ in $(seq 1 200); do
  if curl -fsS "${TRACKER_URL}/api/v1/ping" >/dev/null 2>&1; then
    TRACKER_READY=1
    break
  fi
  sleep 0.05
done
if [[ "$TRACKER_READY" != "1" ]]; then
  echo "error: tracker did not start"
  tail -n 50 "$TMPDIR/tracker.log" || true
  exit 1
fi

echo "[3/5] wait relay addr"
RELAY_ADDR=""
for _ in $(seq 1 200); do
  if [[ -s "$TMPDIR/relay.log" ]]; then
    RELAY_ADDR="$(sed -n 's/^relay listen: //p' "$TMPDIR/relay.log" | head -n 1 | tr -d '\r')"
    if [[ -n "$RELAY_ADDR" ]]; then
      break
    fi
  fi
  sleep 0.05
done
if [[ -z "$RELAY_ADDR" ]]; then
  echo "error: relay addr not found"
  tail -n 50 "$TMPDIR/relay.log" || true
  exit 1
fi

echo "[4/5] start p2p peers (serve)"
RUSTORY_USER_ID=smoke RUSTORY_DEVICE_ID=a target/debug/rr --db-path "$TMPDIR/a.db" p2p-serve \
  --swarm-key "$SWARM_KEY" \
  --identity-key "$TMPDIR/p2p-a.key" \
  --trackers "$TRACKER_URL" \
  --relay "$RELAY_ADDR" >"$TMPDIR/p2p-a.log" 2>&1 &
P2P_A_PID=$!

RUSTORY_USER_ID=smoke RUSTORY_DEVICE_ID=b target/debug/rr --db-path "$TMPDIR/b.db" p2p-serve \
  --swarm-key "$SWARM_KEY" \
  --identity-key "$TMPDIR/p2p-b.key" \
  --trackers "$TRACKER_URL" \
  --relay "$RELAY_ADDR" >"$TMPDIR/p2p-b.log" 2>&1 &
P2P_B_PID=$!

echo "[5/5] wait tracker has peers and run p2p-sync"
READY=0
for _ in $(seq 1 200); do
  if curl -fsS "${TRACKER_URL}/api/v1/peers?user_id=smoke" 2>/dev/null | python3 -c 'import json,sys
try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(1)
sys.exit(0 if len(data.get("peers", [])) >= 2 else 1)'
  then
    READY=1
    break
  fi
  sleep 0.05
done
if [[ "$READY" != "1" ]]; then
  echo "error: tracker did not receive peer registrations"
  echo "--- tracker peers (no filter) ---"
  curl -fsS "${TRACKER_URL}/api/v1/peers" 2>/dev/null || true
  echo
  echo "--- tracker peers (user_id=smoke) ---"
  curl -fsS "${TRACKER_URL}/api/v1/peers?user_id=smoke" 2>/dev/null || true
  echo
  echo "--- tracker log ---"
  tail -n 50 "$TMPDIR/tracker.log" || true
  echo "--- relay log ---"
  tail -n 50 "$TMPDIR/relay.log" || true
  echo "--- p2p-a log ---"
  tail -n 50 "$TMPDIR/p2p-a.log" || true
  echo "--- p2p-b log ---"
  tail -n 50 "$TMPDIR/p2p-b.log" || true
  exit 1
fi

RUSTORY_USER_ID=smoke RUSTORY_DEVICE_ID=c target/debug/rr --db-path "$TMPDIR/c.db" p2p-sync \
  --swarm-key "$SWARM_KEY" \
  --trackers "$TRACKER_URL" \
  --relay "$RELAY_ADDR" \
  --limit 10

echo "ok"
