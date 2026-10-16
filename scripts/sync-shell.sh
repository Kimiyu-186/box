#!/usr/bin/env bash
# Regenerate crates/shell from one commit of the `shell/patches` branch.
# Usage: scripts/sync-shell.sh <commit>
#
# The commit is upstream Strands Shell plus the patch stack Box carries. The paths that
# crates/shell/UPSTREAM.md excludes are dropped, the nested `[workspace]` table is removed
# because crates/shell is a member of this workspace, and the revision in UPSTREAM.md is set
# to the commit. Every other file is copied as the commit holds it.

set -euo pipefail
cd "$(dirname "$0")/.."

if [ "$#" -ne 1 ]; then
  echo "usage: scripts/sync-shell.sh <commit>" >&2
  exit 2
fi

revision=$(git rev-parse --verify --quiet "$1^{commit}") || {
  echo "sync-shell: $1 does not name a commit" >&2
  exit 1
}
if [ -n "$(git status --porcelain --ignored -- crates/shell)" ]; then
  echo "sync-shell: crates/shell has uncommitted, untracked, or ignored files" >&2
  exit 1
fi
# A shell/patches commit carries the Box provenance notices, and a bare upstream or Box
# commit does not.
manifest=$(git show "$revision:Cargo.toml" 2>/dev/null || true)
library=$(git show "$revision:src/lib.rs" 2>/dev/null || true)
if ! grep -q '^name = "strands-shell"$' <<<"$manifest" ||
  ! grep -q '^// Modified by Amazon' <<<"${library%%$'\n'*}"; then
  echo "sync-shell: $1 is not a shell/patches commit with provenance notices" >&2
  exit 1
fi

# Upstream paths that Box does not vendor: Git and CI metadata, the lockfile, language
# packaging, and release tooling.
excluded=(
  .cargo
  .github
  Cargo.lock
  index.d.ts
  index.js
  package.json
  pyproject.toml
  python
  scripts
  tests/python
  tsconfig.json
  xtask
)

staging=$(mktemp -d "${TMPDIR:-/tmp}/sync-shell.XXXXXX")
trap 'rm -rf "$staging"' EXIT

git archive "$revision" | tar -x -C "$staging"
for path in "${excluded[@]}"; do
  rm -rf "${staging:?}/$path"
done

# Drop every `[workspace]` table: a member of this workspace cannot declare its own.
awk '
  /^\[/ { skip = ($0 ~ /^\[workspace[].]/) }
  !skip { print }
' "$staging/Cargo.toml" > "$staging/Cargo.toml.new"
mv "$staging/Cargo.toml.new" "$staging/Cargo.toml"
if grep -q '^\[workspace[].]' "$staging/Cargo.toml"; then
  echo "sync-shell: the [workspace] table was not removed" >&2
  exit 1
fi

cp crates/shell/UPSTREAM.md "$staging/UPSTREAM.md"
sed -i.bak -E "s/^- revision: \`[0-9a-f]+\`/- revision: \`$revision\`/" "$staging/UPSTREAM.md"
rm -f "$staging/UPSTREAM.md.bak"
if ! grep -q "^- revision: \`$revision\`" "$staging/UPSTREAM.md"; then
  echo "sync-shell: UPSTREAM.md has no '- revision:' line to update" >&2
  exit 1
fi

trap 'rm -rf "$staging"; echo "sync-shell: failed; restore with: git checkout -- crates/shell && git clean -fdx crates/shell" >&2' ERR
find crates/shell -mindepth 1 -maxdepth 1 -exec rm -rf {} +
cp -R "$staging/." crates/shell/
trap - ERR

echo "crates/shell regenerated from $revision"
