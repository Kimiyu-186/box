#!/bin/bash
# Run the cargo pack demo in a real box, with the full pack in examples/packs/cargo.pack.toml.
# One forbid on Pack::cargo::Effect::"update_lockfile" refuses `cargo build`, `cargo test`,
# `cargo t`, and `cargo update`, and lets `cargo build --locked` run.
#
# BOX names the strands-box binary (default: target/release/strands-box beside this repo).
# DEMO names the scratch directory (default: /tmp/packs-demo-cargo). Both are recreated per run.
# FULL=1 shows the startup disclosure of filesystem grants.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd -P)
REPO_ROOT=$(cd "$HERE/../../.." && pwd -P)
BOX=${BOX:-$REPO_ROOT/target/release/strands-box}
DEMO=${DEMO:-/tmp/packs-demo-cargo}
case "$DEMO" in
  /tmp/*|/private/tmp/*) ;;
  *) echo "DEMO must be under /tmp" >&2; exit 1 ;;
esac

rm -rf "$DEMO"
mkdir -p "$DEMO/config" "$DEMO/home" "$DEMO/work/hello/src" "$DEMO/cargo-home" "$DEMO/tmp"
DEMO=$(cd "$DEMO" && pwd -P)
cd "$DEMO" || exit 1
# The toolchain outside this repository's rust-toolchain.toml.
TOOLCHAIN=$(cd "$(dirname "$(rustup which cargo)")/.." && pwd -P)

# A library crate with no dependencies, so nothing needs the network and nothing links.
cat > "$DEMO/work/hello/Cargo.toml" <<'TOML'
[package]
name = "hello"
version = "0.1.0"
edition = "2021"
TOML
echo 'pub fn hello() -> &'"'"'static str { "hello" }' > "$DEMO/work/hello/src/lib.rs"
(cd "$DEMO/work/hello" && CARGO_HOME="$DEMO/cargo-home" "$TOOLCHAIN/bin/cargo" generate-lockfile --offline -q)

SCRIPT='run "cargo build"
run "cargo test"
run "cargo t"
run "cargo update"
run "cargo build --locked"
run "ls target/debug"'
python3 - "$HERE" "$DEMO" "$TOOLCHAIN" "$SCRIPT" <<'PY'
import sys
here, demo, toolchain, script = sys.argv[1:]
for name in ("box.toml", "policy.dw"):
    text = open(f"{here}/{name}").read()
    text = text.replace("<DEMO>", demo).replace("<TOOLCHAIN>", toolchain).replace("<AGENT_SCRIPT>", script)
    open(f"{demo}/config/{name}", "w").write(text)
PY
cp "$REPO_ROOT/examples/packs/cargo.pack.toml" "$DEMO/config/cargo.pack.toml"

echo "=== cargo pack demo (toolchain $TOOLCHAIN)"
echo "--- Cargo.lock before: $(shasum "$DEMO/work/hello/Cargo.lock" | cut -c1-12)"
echo "--- strands-box run"
set +e
PATH="$TOOLCHAIN/bin:$PATH" "$BOX" run --config "$DEMO/config/box.toml" 2>&1 | if [ -n "${FULL:-}" ]; then cat; else
  grep -Ev '^strands-box: \[|^  (read|write|metadata|exec|enter|list|deny|read_file|write_file|\(no) '; fi
status=${PIPESTATUS[0]}
set -e
echo "--- strands-box exit=$status"
python3 "$REPO_ROOT/examples/packs-demo/records.py" "$DEMO/state/private/telemetry/records.jsonl" cargo
echo "--- Cargo.lock after:  $(shasum "$DEMO/work/hello/Cargo.lock" | cut -c1-12)"
