#!/bin/bash
# Run the git pack demo in a real box.
#
#   demo.sh forbid   # the policy forbids Pack::git::Action::"push": status runs, push is refused
#   demo.sh allow    # the same box without the forbid: the push reaches the bare remote
#   demo.sh nopack   # the forbid stays but box.toml installs no pack: the box refuses to start
#   demo.sh unmodeled  # also forbid Pack::git::Action::"unmodeled": `git log` is refused
#   demo.sh baseline # no pack and no pack rule: every command decides as it does without packs
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

# macOS: the /usr/bin/git shim runs the developer tools' git. Elsewhere, any readable directory.
DEVELOPER_DIR=$(xcode-select -p 2>/dev/null || echo "$DEMO/config")
render() { sed -e "s#<DEMO>#$DEMO#g" -e "s#<DEVELOPER_DIR>#$DEVELOPER_DIR#g" "$HERE/$1"; }
render box.toml > "$DEMO/config/box.toml"
render policy.dw > "$DEMO/config/policy.dw"
cp "$HERE/git.pack.toml" "$DEMO/config/git.pack.toml"
case "$MODE" in
  forbid) ;;
  allow) sed -i.bak '/@id("no_push")/,$d' "$DEMO/config/policy.dw" ;;
  nopack) sed -i.bak '/^packs = /d' "$DEMO/config/box.toml" ;;
  unmodeled) printf '\n@id("no_unmodeled_git")\nforbid (principal, action == Pack::git::Action::"unmodeled", resource);\n' >> "$DEMO/config/policy.dw" ;;
  baseline)
    sed -i.bak '/^packs = /d' "$DEMO/config/box.toml"
    sed -i.bak '/@id("no_push")/,$d' "$DEMO/config/policy.dw" ;;
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
echo "--- policy decision records naming git (action, category, verdict, rule, args):"
python3 - "$DEMO/state/private/telemetry/records.jsonl" <<'PY' || echo "(could not read records)"
import json, sys
for line in open(sys.argv[1]):
    for batch in json.loads(line).get("resourceLogs", []):
        for scope in batch["scopeLogs"]:
            for record in scope["logRecords"]:
                a = {x["key"]: x["value"] for x in record["attributes"]}
                get = lambda k: a.get(k, {}).get("stringValue", "")
                args = [v.get("stringValue") for v in a.get("process.command_args", {}).get("arrayValue", {}).get("values", [])]
                if "git" not in " ".join(args or [""]):
                    continue
                print(" ", get("strands.box.policy.action"), "|", get("strands.box.policy.category"), "|",
                      get("strands.box.policy.verdict"), "|", get("strands.box.policy.rule"), "|", " ".join(args))
PY
echo "--- remote main after the run: $(git --git-dir="$DEMO/work/remote.git" rev-parse --verify -q main || echo '<none>')"
