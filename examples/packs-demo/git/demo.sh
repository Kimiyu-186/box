#!/bin/bash
# Run the git pack demo in a real box.
#
#   demo.sh forbid   # the policy forbids Pack::git::Action::"push": status runs, push is refused
#   demo.sh allow    # the same box without the forbid: the push reaches the bare remote
#   demo.sh nopack   # the forbid stays but box.toml installs no pack: the box refuses to start
#
# BOX names the strands-box binary (default: target/release/strands-box beside this repo).
# DEMO names the scratch directory (default: /tmp/packs-demo-git). Both are recreated per run.
set -eu
MODE=${1:-forbid}
HERE=$(cd "$(dirname "$0")" && pwd -P)
REPO_ROOT=$(cd "$HERE/../../.." && pwd -P)
BOX=${BOX:-$REPO_ROOT/target/release/strands-box}
DEMO=${DEMO:-/tmp/packs-demo-git}
case "$DEMO" in
  /tmp/*|/private/tmp/*) ;;
  *) echo "DEMO must be under /tmp" >&2; exit 1 ;;
esac

rm -rf "$DEMO"
mkdir -p "$DEMO/config" "$DEMO/home" "$DEMO/work"
DEMO=$(cd "$DEMO" && pwd -P)

# A scratch repository with one commit, and a bare remote a push would really update.
git init -q --bare "$DEMO/work/remote.git"
git init -q -b main "$DEMO/work/repo"
cd "$DEMO/work/repo" || exit 1
git -c user.name=demo -c user.email=demo@example.com commit -q --allow-empty -m "first commit"
git remote add origin "$DEMO/work/remote.git"
cd "$DEMO" || exit 1

render() { sed "s#<DEMO>#$DEMO#g" "$HERE/$1"; }
render box.toml > "$DEMO/config/box.toml"
render policy.dw > "$DEMO/config/policy.dw"
cp "$HERE/git.pack.toml" "$DEMO/config/git.pack.toml"
case "$MODE" in
  forbid) ;;
  allow) sed -i.bak '/@id("no_push")/,$d' "$DEMO/config/policy.dw" ;;
  nopack) sed -i.bak '/^packs = /d' "$DEMO/config/box.toml" ;;
  *) echo "unknown mode $MODE" >&2; exit 1 ;;
esac
rm -f "$DEMO/config/"*.bak

echo "=== mode: $MODE"
echo "--- policy.dw forbids:"; grep -n "forbid" "$DEMO/config/policy.dw" || echo "(no forbid)"
echo "--- box.toml packs:"; grep -n "^packs" "$DEMO/config/box.toml" || echo "(no packs key)"
echo "--- strands-box run"
set +e
"$BOX" run --config "$DEMO/config/box.toml"
status=$?
set -e
echo "--- strands-box exit=$status"
echo "--- remote main after the run: $(git --git-dir="$DEMO/work/remote.git" rev-parse --verify -q main || echo '<none>')"
