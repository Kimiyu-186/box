#!/usr/bin/env bash
# PROTOTYPE(content-inspection): end-to-end check of outbound content inspection.
#
#   ./run-e2e.sh socket [regex|presidio|decider|fake]   mode A: Box connects to a running inspector
#   ./run-e2e.sh stdio  [regex|presidio|decider|fake]   mode B: Box launches the inspector in a leaf
#
# Cases: (a) clean POST -> 200, (b) POST with an SSN -> 403, (c) SSN POST under a policy with no
# inspection rule -> 200, and the inspector is not called.
set -uo pipefail

MODE="${1:-socket}"
BACKEND="${2:-regex}"
TIMEOUT_MS="${TIMEOUT_MS:-$([ "$BACKEND" = decider ] && echo 120000 || echo 2000)}"
HERE="$(cd "$(dirname "$0")" && pwd -P)"
REPO="$(cd "$HERE/../../.." && pwd -P)"
BOX="$REPO/target/debug/strands-box"
INSPECTOR_DIR="$REPO/.agents/pocs/content-inspector"
# PROTOTYPE(content-inspection): canonical, because macOS reaches /var through a /private symlink.
WORK="$(cd "$(mktemp -d /var/tmp/ci-e2e.XXXXXX)" && pwd -P)"
SOCK="$WORK/inspector.sock"
URL="https://httpbin.org/post"
export EXPERIMENTAL_LINUX_SUPPORT=TRUE

if [ "$BACKEND" = fake ]; then
  # A copy of /tmp/fake_inspector.py: the Linux leaf mounts a fresh /tmp.
  PY=/usr/bin/python3
  LEAF_PY=/usr/bin/python3
  LEAF_PY_FLAGS=(-I)
  SCRIPT="$HERE/fake_inspector.py"
  INSPECTOR_ARGS=()
  PY_READS=(/usr/lib64/python3.9 /usr/lib/python3.9)
  LEAF_ENV=""
else
  PY="$INSPECTOR_DIR/.venv/bin/python"
  SCRIPT="$INSPECTOR_DIR/inspector.py"
  INSPECTOR_ARGS=(--backend "$BACKEND")
  # DEVICE=mps on an Apple Silicon Mac, DEVICE=cuda on an NVIDIA host.
  [ "$BACKEND" = decider ] && INSPECTOR_ARGS+=(--device "${DEVICE:-cpu}")
  # The venv python links to the system (mise) interpreter, so the leaf reads both trees.
  PY_REAL="$(readlink -f "$PY")"
  PY_PREFIX="$(cd "$(dirname "$PY_REAL")/.." && pwd -P)"
  PY_READS=("$PY_PREFIX")  # the script directory below also covers .venv
  # The venv's python links through /home/fdingler, a symlink the Linux leaf view does not carry,
  # so the leaf runs the real interpreter and adds the venv's site-packages itself (Box reserves
  # every PYTHON* variable, so PYTHONPATH is not an option).
  LEAF_PY="$PY_REAL"
  SITE="$(ls -d "$INSPECTOR_DIR"/.venv/lib/python3*/site-packages)"
  LEAF_PY_FLAGS=(-I -c "import sys, runpy, site; site.addsitedir('$SITE'); sys.argv = sys.argv[1:]; runpy.run_path(sys.argv[0], run_name='__main__')")
  LEAF_ENV=""
  [ "$BACKEND" = decider ] && PY_READS+=("$(readlink -f "$HOME/.cache/huggingface")")
  # PROTOTYPE(content-inspection): on macOS the leaf starts in the agent workspace, where getcwd()
  # needs a read (torch calls os.path.abspath at import), and Homebrew python loads its dylibs
  # (openssl, xz, zstd, mpdecimal) through /opt/homebrew/opt symlinks outside the interpreter prefix.
  if [ "$(uname)" = Darwin ]; then
    case "$PY_PREFIX" in /opt/homebrew/*) PY_READS=(/opt/homebrew/Cellar /opt/homebrew/opt "${PY_READS[@]:1}") ;; esac
    PY_READS+=("$WORK/ws")
    # The Linux leaf gets a fresh /tmp; a Seatbelt leaf needs its own writable TMPDIR (torch cache).
    LEAF_TMP="$WORK/leaf-tmp"
    LEAF_ENV=", TMPDIR = \"$LEAF_TMP\""
    PY_READS+=("$LEAF_TMP")
  fi
fi

(cd "$REPO" && cargo build -q -p strands-box -p strands-box-containment --all-features >/dev/null 2>&1) || { echo "cargo build failed"; exit 1; }

toml_list() { local out="" x; for x in "$@"; do out+="\"$x\", "; done; echo "[${out%, }]"; }

write_config() { # $1 = policy file, $2 = config path, $3 = state dir
  {
    echo "name = \"inspection-e2e\""
    echo "box_dir = \"$3\""
    echo "policy = \"$1\""
    echo
    echo "[agent]"
    echo "command = [\"/usr/bin/curl\"]"
    echo "workspace = \"$WORK/ws\""
    echo "env = { PATH = \"/usr/bin:/bin\", HOME = \"$WORK/home\" }"
    # PROTOTYPE(content-inspection): macOS curl (LibreSSL) opens openssl.cnf at startup.
    [ "$(uname)" = Darwin ] && { echo "[agent.filesystem]"; echo "read_file = [\"/private/etc/ssl/openssl.cnf\"]"; }
    echo
    echo "[inspection.content]"
    echo "provider = \"local\""
    echo "timeout_ms = $TIMEOUT_MS"
    if [ "$MODE" = socket ]; then
      echo "socket = \"$SOCK\""
    else
      echo "command = $(toml_list "$LEAF_PY" "${LEAF_PY_FLAGS[@]}" "$SCRIPT" --stdio "${INSPECTOR_ARGS[@]}")"
      echo "env = { PATH = \"/usr/bin:/bin\", HF_HUB_OFFLINE = \"1\", HOME = \"$(readlink -f "$HOME")\"$LEAF_ENV }"
      # PROTOTYPE(content-inspection): MPS needs the leaf's GPU grant; GPU=0 leaves it out.
      [ "${DEVICE:-cpu}" = mps ] && [ "${GPU:-1}" = 1 ] && echo "gpu = true"
      echo "[inspection.content.filesystem]"
      echo "read = $(toml_list "${PY_READS[@]}" "$(dirname "$SCRIPT")")"
      [ -n "${LEAF_TMP:-}" ] && echo "write = $(toml_list "$LEAF_TMP")"
    fi
  } > "$2"
}

INSPECTOR_PID=""
INSPECTOR_LOG="$WORK/inspector.log"
if [ "$MODE" = socket ]; then
  "$PY" -I "$SCRIPT" --socket "$SOCK" "${INSPECTOR_ARGS[@]}" 2> "$INSPECTOR_LOG" &
  INSPECTOR_PID=$!
  for _ in $(seq 1 3000); do [ -S "$SOCK" ] && break; sleep 0.1; done
  [ -S "$SOCK" ] || { echo "inspector socket never appeared"; cat "$INSPECTOR_LOG"; exit 1; }
fi
trap '[ -n "$INSPECTOR_PID" ] && kill "$INSPECTOR_PID" 2>/dev/null' EXIT

mkdir -p "$WORK/home" "$WORK/ws" ${LEAF_TMP:+"$LEAF_TMP"}
run_case() { # $1 = name, $2 = policy, $3 = body, $4 = expected status
  local name="$1" state="$WORK/state-$1" config="$WORK/box-$1.toml" out
  write_config "$HERE/$2" "$config" "$state"
  cp "$config" "$HERE/box-$MODE-$name.toml"
  out="$("$BOX" run --config "$config" -- -sS -o /dev/null -w 'status=%{http_code}\n' \
        -X POST -H 'content-type: application/json' -H 'authorization: Bearer hunter2' \
        --data "$3" "$URL" 2> "$WORK/$name.stderr")"
  local status; status="$(sed -n 's/^status=//p' <<<"$out")"
  echo "--- case $name: expect $4, got ${status:-none}"
  grep -E '^inspection:|inspector|http:request gate' "$WORK/$name.stderr" | sed 's/^/    /'
  [ "$status" = "$4" ] && echo "    PASS" || { echo "    FAIL"; sed 's/^/    | /' "$WORK/$name.stderr" | tail -25; }
}

run_case a-clean policy.dw '{"msg":"hello"}' 200
run_case b-pii policy.dw '{"ssn":"123-45-6789"}' 403
calls_before="$(grep -c . "$INSPECTOR_LOG" 2>/dev/null || true)"
run_case c-no-rule policy-noinspect.dw '{"ssn":"123-45-6789"}' 200
if grep -q '^inspection:' "$WORK/c-no-rule.stderr"; then
  echo "    FAIL: the inspector was called under a policy with no inspection rule"
else
  echo "    PASS: no inspection line under the no-rule policy"
fi
[ "$MODE" = socket ] && echo "--- inspector log lines before/after case c: $calls_before/$(grep -c . "$INSPECTOR_LOG" || true)"
echo "work dir: $WORK"
