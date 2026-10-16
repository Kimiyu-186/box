#!/bin/bash
# Run the aws pack demo in a real box, with the full pack in examples/packs/aws.pack.toml.
# One forbid on Tool::aws::Effect::"delete_data" refuses `aws s3 rm`, `aws s3 sync --delete`, and
# `aws s3api delete-bucket` (exit 126), and lets `aws sts get-caller-identity` and
# `aws s3 rm --dryrun` start. Credentials are dummies and the endpoint is http://127.0.0.1:9, so
# an allowed command fails at the network: that is expected, and no command reaches AWS.
#
# BOX names the strands-box binary (default: target/release/strands-box beside this repo).
# DEMO names the scratch directory (default: /tmp/packs-demo-aws). Both are recreated per run.
# FULL=1 shows the startup disclosure of filesystem grants.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd -P)
REPO_ROOT=$(cd "$HERE/../../.." && pwd -P)
BOX=${BOX:-$REPO_ROOT/target/release/strands-box}
DEMO=${DEMO:-/tmp/packs-demo-aws}
case "$DEMO" in
  /tmp/*|/private/tmp/*) ;;
  *) echo "DEMO must be under /tmp" >&2; exit 1 ;;
esac

rm -rf "$DEMO"
mkdir -p "$DEMO/config" "$DEMO/home" "$DEMO/work" "$DEMO/tmp"
DEMO=$(cd "$DEMO" && pwd -P)
: > "$DEMO/home/aws-config"
: > "$DEMO/home/aws-credentials"
echo "site" > "$DEMO/work/index.html"
# The official AWS CLI v2 bundle, whose /usr/local/bin/aws links into /usr/local/aws-cli.
AWS_BIN=${AWS_BIN:-/usr/local/bin/aws}
[ -x "$AWS_BIN" ] || AWS_BIN=$(command -v aws)
AWS_HOME=$(dirname "$(python3 -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$AWS_BIN")")

SCRIPT='run "aws s3 rm s3://b/k"
run "aws s3 sync . s3://b --delete"
run "aws s3api delete-bucket --bucket b"
run "aws sts get-caller-identity"
run "aws s3 rm s3://b/k --dryrun"'
python3 - "$HERE" "$DEMO" "$AWS_HOME" "$SCRIPT" <<'PY'
import sys
here, demo, aws_home, script = sys.argv[1:]
for name in ("box.toml", "policy.dw"):
    text = open(f"{here}/{name}").read()
    text = text.replace("<DEMO>", demo).replace("<AWS_HOME>", aws_home).replace("<AGENT_SCRIPT>", script)
    open(f"{demo}/config/{name}", "w").write(text)
PY
cp "$REPO_ROOT/examples/packs/aws.pack.toml" "$DEMO/config/aws.pack.toml"

echo "=== aws pack demo ($AWS_HOME/aws, endpoint http://127.0.0.1:9, dummy credentials)"
echo "--- strands-box run"
set +e
PATH="$AWS_HOME:$PATH" "$BOX" run --config "$DEMO/config/box.toml" 2>&1 | if [ -n "${FULL:-}" ]; then cat; else
  grep -Ev '^strands-box: \[|^  (read|write|metadata|exec|enter|list|deny|read_file|write_file|\(no) '; fi
status=${PIPESTATUS[0]}
set -e
echo "--- strands-box exit=$status"
python3 "$REPO_ROOT/examples/packs-demo/records.py" "$DEMO/state/private/telemetry/records.jsonl" aws
