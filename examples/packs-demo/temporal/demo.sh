#!/bin/bash
# Run the temporal pack demo with two packs in one box, the full examples/packs/git.pack.toml and
# examples/packs/cargo.pack.toml. `git push` is allowed only after a passing `cargo test` in the
# last hour, and one rule over every cargo action refuses each one that can rewrite Cargo.lock.
#
# Run 1 (one box run, so one history):
#   1. git push before any test                 -> refused by push_after_tests
#   2. cargo build, cargo test (no --locked)    -> refused by lockfile_is_frozen; git status runs
#   3. cargo test --locked that fails, git push -> the push is still refused
#   4. cargo +stable test --locked that passes,  -> allowed, and the push lands in the bare remote;
#      then git push                               cargo is the rustup proxy, so +stable runs
#   5. cargo t --locked that passes, git push   -> allowed; nothing new to push
# Run 2 (a new `strands-box run` on the same box_dir): a new commit, and git push with no test in
#   this run -> allowed, because the passing test from run 1 is in the durable history.
#
#   demo.sh unscoped  # the lockfile rule over `action` (every action): the box refuses to start,
#                     # because Tool::git::Effect and Tool::cargo::Effect are not compatible types
#
# BOX names the strands-box binary (default: target/release/strands-box beside this repo).
# DEMO names the scratch directory (default: /tmp/packs-demo-temporal). It is recreated.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd -P)
REPO_ROOT=$(cd "$HERE/../../.." && pwd -P)
BOX=${BOX:-$REPO_ROOT/target/release/strands-box}
DEMO=${DEMO:-/tmp/packs-demo-temporal}
case "$DEMO" in
  /tmp/*|/private/tmp/*) ;;
  *) echo "DEMO must be under /tmp" >&2; exit 1 ;;
esac
MODE=${1:-run}
# The toolchain outside this repository's rust-toolchain.toml.
TOOLCHAIN=$(cd "$(dirname "$(cd /tmp && rustup which cargo)")/.." && pwd -P)
CARGO_BIN=$(cd "${CARGO_HOME:-$HOME/.cargo}/bin" && pwd -P)
RUSTUP_HOME=$(cd "${RUSTUP_HOME:-$HOME/.rustup}" && pwd -P)
# A toolchain name for `cargo +<name>`: stable when it is installed, else the default.
PLUS=$(rustup toolchain list | awk '/^stable/ {print "stable"; exit}')
PLUS=${PLUS:-$(rustup toolchain list | awk '/default/ {print $1; exit}')}

rm -rf "$DEMO"
mkdir -p "$DEMO/config" "$DEMO/home" "$DEMO/work" "$DEMO/cargo-home" "$DEMO/target" "$DEMO/tmp"
DEMO=$(cd "$DEMO" && pwd -P)

# A tiny crate with no dependencies, in a repository with a bare remote a push really updates.
# Its one test passes when verdict.txt says "pass", so the agent decides which way it goes.
git init -q --bare "$DEMO/work/remote.git"
git init -q -b main "$DEMO/work/repo"
cd "$DEMO/work/repo" || exit 1
mkdir src
cat > Cargo.toml <<'TOML'
[package]
name = "demo"
version = "0.1.0"
edition = "2021"
TOML
cat > src/lib.rs <<'RS'
#[test]
fn verdict_is_pass() {
    let verdict = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/verdict.txt"))
        .unwrap_or_default();
    assert_eq!(verdict.trim(), "pass", "verdict.txt says {:?}", verdict.trim());
}
RS
printf 'target/\nverdict.txt\nCargo.lock\n' > .gitignore
: > verdict.txt
CARGO_HOME="$DEMO/cargo-home" "$TOOLCHAIN/bin/cargo" generate-lockfile --offline -q
git add . && git -c user.name=demo -c user.email=demo@example.com commit -q -m "first commit"
git remote add origin "$DEMO/work/remote.git"
cd "$DEMO" || exit 1

DEVELOPER_DIR=$(xcode-select -p 2>/dev/null || echo "$DEMO/config")
SDKROOT=$(xcrun --sdk macosx --show-sdk-path 2>/dev/null || echo "$DEMO/config")
render() {
  sed -e "s#<DEMO>#$DEMO#g" -e "s#<DEVELOPER_DIR>#$DEVELOPER_DIR#g" -e "s#<SDKROOT>#$SDKROOT#g" \
      -e "s#<CARGO_BIN>#$CARGO_BIN#g" -e "s#<RUSTUP_HOME>#$RUSTUP_HOME#g" -e "s#<AGENT_SCRIPT>#$2#g" "$HERE/$1"
}
cp "$HERE/policy.dw" "$DEMO/config/"
cp "$REPO_ROOT/examples/packs/git.pack.toml" "$REPO_ROOT/examples/packs/cargo.pack.toml" "$DEMO/config/"
if [ "$MODE" = unscoped ]; then
  sed -i.bak 's#^forbid (principal, action in Tool::cargo::Action::"\*", resource)#forbid (principal, action, resource)#; s#^when { context.input.effects#when { context.input has effects \&\& context.input.effects#' "$DEMO/config/policy.dw"
  grep -A1 'action, resource)' "$DEMO/config/policy.dw"
fi

cat > "$DEMO/config/run1.sh" <<'SH'
echo "### 1. git push before any test"
zsh -c 'git push -q origin main; echo "git push exit=$?"'
echo "### 2. cargo without --locked can rewrite Cargo.lock; git is not affected"
zsh -c 'cargo build -q; echo "cargo build exit=$?"'
zsh -c 'cargo test -q; echo "cargo test exit=$?"'
zsh -c 'git status --short; echo "git status exit=$?"'
echo "### 3. a failing cargo test --locked, then git push"
echo fail > verdict.txt
zsh -c 'cargo test -q --locked; echo "cargo test --locked exit=$?"'
zsh -c 'git push -q origin main; echo "git push exit=$?"'
echo "### 4. a passing cargo +PLUS test --locked through the rustup proxy, then git push"
echo pass > verdict.txt
zsh -c 'cargo +PLUS test -q --locked; echo "cargo +PLUS test --locked exit=$?"'
zsh -c 'git push -q origin main; echo "git push exit=$?"'
echo "### 5. a passing cargo t --locked, then git push"
zsh -c 'cargo t -q --locked; echo "cargo t --locked exit=$?"'
zsh -c 'git push -q origin main; echo "git push exit=$?"'
SH
sed -i.bak "s#+PLUS#+$PLUS#g" "$DEMO/config/run1.sh" && rm -f "$DEMO/config/run1.sh.bak"
cat > "$DEMO/config/run2.sh" <<'SH'
echo "### 6. a new box run on the same box_dir: git push with no test in this run"
zsh -c 'git push -q origin main; echo "git push exit=$?"'
SH

remote_main() { git --git-dir="$DEMO/work/remote.git" rev-parse --short --verify -q main || echo '<none>'; }
run_box() {
  render box.toml "$DEMO/config/$1.sh" > "$DEMO/config/box.toml"
  echo "=== strands-box run ($1)"
  set +e
  PATH="$CARGO_BIN:$PATH" "$BOX" run --config "$DEMO/config/box.toml" 2>&1 | if [ -n "${FULL:-}" ]; then cat; else
    grep -Ev '^strands-box: \[|^  (read|write|metadata|exec|enter|list|deny|read_file|write_file|\(no) '; fi
  echo "=== strands-box exit=${PIPESTATUS[0]}"
  set -e
}

echo "--- local main: $(git -C "$DEMO/work/repo" rev-parse --short main), remote main: $(remote_main)"
run_box run1
[ "$MODE" = unscoped ] && exit 0
echo "--- remote main after run 1: $(remote_main)"
git -C "$DEMO/work/repo" -c user.name=demo -c user.email=demo@example.com commit -q --allow-empty -m "second commit"
echo "--- local main before run 2: $(git -C "$DEMO/work/repo" rev-parse --short main)"
run_box run2
echo "--- remote main after run 2: $(remote_main)"

echo "--- decision records for git and cargo (category action, verdict, rule, args):"
python3 - "$DEMO/state/private/telemetry/records.jsonl" <<'PY' || echo "(could not read records)"
import json, sys
for line in open(sys.argv[1]):
    for batch in json.loads(line).get("resourceLogs", []):
        for scope in batch["scopeLogs"]:
            for record in scope["logRecords"]:
                a = {x["key"]: x["value"] for x in record["attributes"]}
                get = lambda k: a.get(k, {}).get("stringValue", "")
                args = [v.get("stringValue") for v in a.get("process.command_args", {}).get("arrayValue", {}).get("values", [])]
                action = get("strands.box.policy.action")
                category = get("strands.box.policy.category")
                if not (category.startswith("Tool::") or "spawn" in action):
                    continue
                print(" ", category, action, "|", get("strands.box.policy.verdict"), "|",
                      get("strands.box.policy.rule"), "|", " ".join(args))
PY
