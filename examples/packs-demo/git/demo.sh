#!/bin/bash
# Run the git pack demo in a real box, with the full pack in examples/packs/git.pack.toml.
#
# A git the pack binds is decided only by its Tool::git action, so every pack mode needs a permit on
# Tool::git actions. Each mode below except unmodeled and symlink permits Tool::git::Action::"*".
#
#   demo.sh force         # no_force_push: every force spelling is refused; -n -f and a plain push run
#   demo.sh upstream      # a forbid on remote == "upstream": pushes to upstream are refused
#   demo.sh forbid        # forbid Tool::git::Action::"push": status runs, push is refused
#   demo.sh allow         # permit "*" and no forbid: the push reaches the bare remote
#   demo.sh nopack        # the push forbid stays but box.toml installs no pack: the box refuses to start
#   demo.sh unmodeled     # permit only status, log, and push: `git log -1` (the -<n> form) and the
#                         # config alias `git ci` decide as unmodeled, and no permit names them, so
#                         # both are refused with no forbid rule
#   demo.sh unmodeled-all # the same commands under a permit on "*", which includes unmodeled: all run
#   demo.sh symlink       # the box shell refuses `ln -s /usr/bin/git`, because the target is outside
#                         # its binds. A link a leaf could plant (demo.sh plants bin/g) is the git file
#                         # under a name no [tool] declares, so it decides as unmodeled, and only status
#                         # is permitted, so it is refused
#   demo.sh baseline      # no pack and no pack rule: git decides as shell:spawn, as it does without packs
#
# BOX names the strands-box binary (default: target/release/strands-box beside this repo).
# DEMO names the scratch directory (default: /tmp/packs-demo-git). Both are recreated per run.
set -eu
MODE=${1:-force}
HERE=$(cd "$(dirname "$0")" && pwd -P)
REPO_ROOT=$(cd "$HERE/../../.." && pwd -P)
BOX=${BOX:-$REPO_ROOT/target/release/strands-box}
DEMO=${DEMO:-/tmp/packs-demo-git}
case "$DEMO" in
  /tmp/*|/private/tmp/*) ;;
  *) echo "DEMO must be under /tmp" >&2; exit 1 ;;
esac

rm -rf "$DEMO"
mkdir -p "$DEMO/config" "$DEMO/home" "$DEMO/work" "$DEMO/tmp"
DEMO=$(cd "$DEMO" && pwd -P)

# A scratch repository whose main is pushed to origin, plus one unpushed commit, so a plain
# `git push` has work to do. `upstream` is a second bare remote.
G="git -c user.name=demo -c user.email=demo@example.com"
git init -q --bare "$DEMO/work/remote.git"
git init -q --bare "$DEMO/work/upstream.git"
git init -q -b main "$DEMO/work/repo"
cd "$DEMO/work/repo" || exit 1
$G commit -q --allow-empty -m "first commit"
git remote add origin "$DEMO/work/remote.git"
git remote add upstream "$DEMO/work/upstream.git"
git push -q -u origin main
$G commit -q --allow-empty -m "second commit"
# A config alias the pack cannot see through, so `git ci` decides as unmodeled.
git config alias.ci "commit --allow-empty -m alias"
git config user.name demo && git config user.email demo@example.com
# A link a tool leaf with a write grant on the workspace could make; the box shell cannot.
[ "$MODE" = symlink ] && mkdir bin && ln -s /usr/bin/git bin/g
cd "$DEMO" || exit 1

ALL='@id("all_git")
permit (principal, action in Tool::git::Action::"*", resource);

'
case "$MODE" in
  force)
    RULES=$ALL'@id("no_force_push")
@description("Force pushes are not allowed.")
forbid (principal, action == Tool::git::Action::"push", resource)
when { context.input.effects.contains(Tool::git::Effect::"force_push") };'
    SCRIPT='run "git push -f origin main"
run "git push --force origin main"
run "git push --force-with-lease origin main"
run "git push -uf origin main"
run "git push origin +main"
run "cd .. && git -C repo push -f origin main"
run "git push -n -f origin main"
run "git push"' ;;
  upstream)
    RULES=$ALL'@id("no_upstream")
@description("Nothing is pushed to upstream.")
forbid (principal, action == Tool::git::Action::"push", resource)
when { context.input has remote && context.input.remote == "upstream" };'
    SCRIPT='run "git push upstream main"
run "cd .. && git -C repo push upstream"
run "git push origin main"' ;;
  forbid|nopack)
    RULES=$ALL'@id("no_push")
@description("Pushes are not allowed in this box.")
forbid (principal, action == Tool::git::Action::"push", resource);'
    SCRIPT='run "git status --short --branch"
run "git push origin main"' ;;
  unmodeled|unmodeled-all)
    RULES='@id("some_git")
permit (principal, action in [Tool::git::Action::"status", Tool::git::Action::"log", Tool::git::Action::"push"], resource);'
    [ "$MODE" = unmodeled-all ] && RULES=$ALL
    SCRIPT='run "git status --short --branch"
run "git log -n 1 --oneline"
run "git log -1 --oneline"
run "git ci"' ;;
  symlink)
    RULES='@id("git_status")
permit (principal, action == Tool::git::Action::"status", resource);

@id("link_in_workspace")
permit (principal, action in [Box::Action::"fs:read", Box::Action::"fs:write"], resource)
when { context.input.path like "*packs-demo-git/work/repo/*g" || context.input.path == "/usr/bin/git" };'
    SCRIPT='run "ln -s /usr/bin/git g"
run "git status --short --branch"
run "./bin/g status --short --branch"
run "$PWD/bin/g status --short --branch"' ;;
  allow)
    RULES=$ALL
    SCRIPT='run "git status --short --branch"
run "git push origin main"' ;;
  baseline)
    RULES=''
    SCRIPT='run "git status --short --branch"
run "git push origin main"' ;;
  *) echo "unknown mode $MODE" >&2; exit 1 ;;
esac

# macOS: the /usr/bin/git shim runs the developer tools' git. Elsewhere, any readable directory.
DEVELOPER_DIR=$(xcode-select -p 2>/dev/null || echo "$DEMO/config")
python3 - "$HERE" "$DEMO" "$DEVELOPER_DIR" "$SCRIPT" "$RULES" <<'PY'
import sys
here, demo, developer, script, rules = sys.argv[1:]
def render(name):
    text = open(f"{here}/{name}").read()
    return text.replace("<DEMO>", demo).replace("<DEVELOPER_DIR>", developer).replace("<AGENT_SCRIPT>", script)
open(f"{demo}/config/box.toml", "w").write(render("box.toml"))
open(f"{demo}/config/policy.dw", "w").write(render("policy.dw") + rules + "\n")
PY
cp "$REPO_ROOT/examples/packs/git.pack.toml" "$DEMO/config/git.pack.toml"
case "$MODE" in
  nopack|baseline) sed -i.bak '/^tool_packs = /d' "$DEMO/config/box.toml"; rm -f "$DEMO/config/"*.bak ;;
esac

echo "=== mode: $MODE"
echo "--- policy.dw mode rules:"; sed -n '/MODE RULES/,$p' "$DEMO/config/policy.dw" | tail -n +2 | sed 's/^/    /'
echo "--- box.toml tool_packs:"; grep -n "^tool_packs" "$DEMO/config/box.toml" || echo "(no tool_packs key)"
echo "--- strands-box run"
set +e
# The startup disclosure of filesystem grants is long; FULL=1 shows it.
"$BOX" run --config "$DEMO/config/box.toml" 2>&1 | if [ -n "${FULL:-}" ]; then cat; else
  grep -Ev '^strands-box: \[|^  (read|write|metadata|exec|enter|list|deny|read_file|write_file|\(no) '; fi
status=${PIPESTATUS[0]}
set -e
echo "--- strands-box exit=$status"
python3 "$REPO_ROOT/examples/packs-demo/records.py" "$DEMO/state/private/telemetry/records.jsonl" git
echo "--- origin main: $(git --git-dir="$DEMO/work/remote.git" log --oneline -1 main 2>/dev/null || echo '<none>')"
echo "--- upstream main: $(git --git-dir="$DEMO/work/upstream.git" log --oneline -1 main 2>/dev/null || echo '<none>')"
