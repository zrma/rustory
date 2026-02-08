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

wait_p2p_peer_id() {
  local log_path="$1"
  local peer_id=""

  for _ in $(seq 1 200); do
    if [[ -s "$log_path" ]]; then
      peer_id="$(sed -n 's#^p2p listen: .*/p2p/##p' "$log_path" | tail -n 1 | tr -d '\r')"
      if [[ -n "$peer_id" ]]; then
        echo "$peer_id"
        return 0
      fi
    fi
    sleep 0.05
  done

  return 1
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
P2P_A_LOG="$TMPDIR/p2p-a.log"
P2P_B_LOG="$TMPDIR/p2p-b.log"

RUSTORY_USER_ID=smoke RUSTORY_DEVICE_ID=a target/debug/rr --db-path "$TMPDIR/a.db" p2p-serve \
  --swarm-key "$SWARM_KEY" \
  --identity-key "$TMPDIR/p2p-a.key" \
  --trackers "$TRACKER_URL" \
  --relay "$RELAY_ADDR" >"$P2P_A_LOG" 2>&1 &
P2P_A_PID=$!

RUSTORY_USER_ID=smoke RUSTORY_DEVICE_ID=b target/debug/rr --db-path "$TMPDIR/b.db" p2p-serve \
  --swarm-key "$SWARM_KEY" \
  --identity-key "$TMPDIR/p2p-b.key" \
  --trackers "$TRACKER_URL" \
  --relay "$RELAY_ADDR" >"$P2P_B_LOG" 2>&1 &
P2P_B_PID=$!

echo "[4/5] verify PeerId persists across restart (identity key persistence)"
PEER_A_ID_1="$(wait_p2p_peer_id "$P2P_A_LOG" || true)"
PEER_B_ID_1="$(wait_p2p_peer_id "$P2P_B_LOG" || true)"
if [[ -z "$PEER_A_ID_1" || -z "$PEER_B_ID_1" ]]; then
  echo "error: failed to parse peer id from p2p listen logs"
  echo "--- p2p-a log ---"
  tail -n 80 "$P2P_A_LOG" || true
  echo "--- p2p-b log ---"
  tail -n 80 "$P2P_B_LOG" || true
  exit 1
fi

kill "$P2P_A_PID" 2>/dev/null || true
kill "$P2P_B_PID" 2>/dev/null || true
wait "$P2P_A_PID" 2>/dev/null || true
wait "$P2P_B_PID" 2>/dev/null || true

P2P_A_LOG2="$TMPDIR/p2p-a.restart.log"
P2P_B_LOG2="$TMPDIR/p2p-b.restart.log"

RUSTORY_USER_ID=smoke RUSTORY_DEVICE_ID=a target/debug/rr --db-path "$TMPDIR/a.db" p2p-serve \
  --swarm-key "$SWARM_KEY" \
  --identity-key "$TMPDIR/p2p-a.key" \
  --trackers "$TRACKER_URL" \
  --relay "$RELAY_ADDR" >"$P2P_A_LOG2" 2>&1 &
P2P_A_PID=$!

RUSTORY_USER_ID=smoke RUSTORY_DEVICE_ID=b target/debug/rr --db-path "$TMPDIR/b.db" p2p-serve \
  --swarm-key "$SWARM_KEY" \
  --identity-key "$TMPDIR/p2p-b.key" \
  --trackers "$TRACKER_URL" \
  --relay "$RELAY_ADDR" >"$P2P_B_LOG2" 2>&1 &
P2P_B_PID=$!

PEER_A_ID_2="$(wait_p2p_peer_id "$P2P_A_LOG2" || true)"
PEER_B_ID_2="$(wait_p2p_peer_id "$P2P_B_LOG2" || true)"
if [[ "$PEER_A_ID_1" != "$PEER_A_ID_2" ]]; then
  echo "error: peer a id changed after restart: $PEER_A_ID_1 -> $PEER_A_ID_2"
  echo "--- p2p-a initial log ---"
  tail -n 80 "$P2P_A_LOG" || true
  echo "--- p2p-a restart log ---"
  tail -n 80 "$P2P_A_LOG2" || true
  exit 1
fi
if [[ "$PEER_B_ID_1" != "$PEER_B_ID_2" ]]; then
  echo "error: peer b id changed after restart: $PEER_B_ID_1 -> $PEER_B_ID_2"
  echo "--- p2p-b initial log ---"
  tail -n 80 "$P2P_B_LOG" || true
  echo "--- p2p-b restart log ---"
  tail -n 80 "$P2P_B_LOG2" || true
  exit 1
fi

echo "[5/5] wait tracker has peers"
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
  tail -n 50 "$P2P_A_LOG2" || true
  echo "--- p2p-b log ---"
  tail -n 50 "$P2P_B_LOG2" || true
  exit 1
fi

echo "[5/5] record seed entries on peers (a/b)"
A_ENTRY_ID="$(RUSTORY_USER_ID=smoke RUSTORY_DEVICE_ID=a target/debug/rr --db-path "$TMPDIR/a.db" record \
  --cmd "echo smoke-a" \
  --cwd "/tmp" \
  --exit-code 0 \
  --shell zsh \
  --print-id | tr -d '\r')"
if [[ -z "$A_ENTRY_ID" ]]; then
  echo "error: failed to record entry on peer a"
  exit 1
fi

B_ENTRY_ID="$(RUSTORY_USER_ID=smoke RUSTORY_DEVICE_ID=b target/debug/rr --db-path "$TMPDIR/b.db" record \
  --cmd "echo smoke-b" \
  --cwd "/tmp" \
  --exit-code 0 \
  --shell zsh \
  --print-id | tr -d '\r')"
if [[ -z "$B_ENTRY_ID" ]]; then
  echo "error: failed to record entry on peer b"
  exit 1
fi

echo "[5/5] record one entry on client (c)"
ENTRY_ID="$(RUSTORY_USER_ID=smoke RUSTORY_DEVICE_ID=c target/debug/rr --db-path "$TMPDIR/c.db" record \
  --cmd "echo smoke-push" \
  --cwd "/tmp" \
  --exit-code 0 \
  --shell zsh \
  --print-id | tr -d '\r')"
if [[ -z "$ENTRY_ID" ]]; then
  echo "error: failed to record entry on client"
  exit 1
fi

echo "[5/5] run p2p-sync twice with --push (should not gossip a<->b)"
for _ in 1 2; do
  RUSTORY_USER_ID=smoke RUSTORY_DEVICE_ID=c target/debug/rr --db-path "$TMPDIR/c.db" p2p-sync \
    --swarm-key "$SWARM_KEY" \
    --trackers "$TRACKER_URL" \
    --relay "$RELAY_ADDR" \
    --push \
    --limit 10
done

echo "[5/5] verify entry was pushed to at least one peer"
python3 - <<'PY' "$ENTRY_ID" "$TMPDIR/a.db" "$TMPDIR/b.db"
import sqlite3
import sys

entry_id = sys.argv[1]
dbs = sys.argv[2:]

found = 0
for db in dbs:
    conn = sqlite3.connect(db)
    try:
        cur = conn.execute("SELECT COUNT(*) FROM entries WHERE entry_id = ?", (entry_id,))
        n = cur.fetchone()[0]
    finally:
        conn.close()
    if n:
        found += 1

if found == 0:
    sys.stderr.write(f"entry_id not found on any peer: {entry_id}\n")
    sys.exit(1)
PY

echo "[5/5] verify no gossip between a and b (push is local-only)"
python3 - <<'PY' "$A_ENTRY_ID" "$B_ENTRY_ID" "$TMPDIR/a.db" "$TMPDIR/b.db"
import sqlite3
import sys

a_entry_id, b_entry_id, a_db, b_db = sys.argv[1:5]

def has_entry(db_path: str, entry_id: str) -> bool:
    conn = sqlite3.connect(db_path)
    try:
        cur = conn.execute("SELECT COUNT(*) FROM entries WHERE entry_id = ?", (entry_id,))
        return cur.fetchone()[0] > 0
    finally:
        conn.close()

if has_entry(b_db, a_entry_id):
    sys.stderr.write(f"unexpected gossip: a entry found in b db: entry_id={a_entry_id}\n")
    sys.exit(1)
if has_entry(a_db, b_entry_id):
    sys.stderr.write(f"unexpected gossip: b entry found in a db: entry_id={b_entry_id}\n")
    sys.exit(1)
PY

echo "ok"
