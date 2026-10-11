# git pack corpus

Expected parses for `git.pack.toml` under the shell grammar in `crates/policy/src/pack/grammar.rs`.
`run.py` checks the action and the effects of every row.
A row shows only fields that differ from their defaults: Bool `false`, String absent, Set empty.
`fx` is `context.input.effects`. The `subcommand` note on `unmodeled` rows is documentation only (EXT-11 is not implemented).
Rows marked "verified" were checked against git 2.50 on this host.

| # | Command line | Action | Fields | fx |
|---|---|---|---|---|
| 1 | `git push -f origin main` | push | remote=`origin`, refspec={`main`}, force | network, force_push |
| 2 | `git push -uf origin main` | push | set_upstream, force, remote, refspec={`main`} | network, force_push |
| 3 | `git push origin main --force` | push | option after operands is still an option | network, force_push |
| 4 | `git push --force-with-lease origin main` | push | force_with_lease=`""`, remote=`origin`, refspec={`main`} | network, force_push |
| 5 | `git push --force-with-lease=main:abc123 origin main` | push | force_with_lease=`main:abc123` | network, force_push |
| 6 | `git push --force-with-lease origin` | push | force_with_lease=`""`, remote=`origin`. An optional value never takes the next word | network, force_push |
| 7 | `git push origin +main` | push | refspec={`+main`} | network, force_push |
| 8 | `git push origin '+refs/heads/*:refs/heads/*'` | push | refspec={`+refs/heads/*:refs/heads/*`} | network, force_push |
| 9 | `git -C app push -f origin main` | push | directory=`app`, force | network, force_push |
| 10 | `git push -n -f origin main` | push | dry_run, force | network |
| 11 | `git push -fn origin main` | push | same as #10: removes win, order does not matter | network |
| 12 | `git push origin :old-topic` | push | refspec={`:old-topic`} | network, delete_remote_ref |
| 13 | `git push -d origin old-topic` | push | delete | network, delete_remote_ref |
| 14 | `git push --mirror backup` | push | mirror, remote=`backup` | network, force_push, delete_remote_ref |
| 15 | `git push --prune origin 'refs/heads/*:refs/heads/*'` | push | prune | network, delete_remote_ref |
| 16 | `git push --repo=upstream` | push | repo=`upstream`; remote absent. A rule on `remote` alone misses this | network |
| 17 | `git push -o ci.skip origin` | push | push_option={`ci.skip`}, remote | network |
| 18 | `git push --receive-pack=/tmp/x ../local.git main` | push | receive_pack=`/tmp/x` | network, run_program |
| 19 | `git push origin -- -f` | push | after `--` every word is an operand: refspec={`-f`}; force=false | network |
| 20 | `git push --dele origin main` | unmodeled | subcommand=`push`. git takes `--dele` as `--delete` (verified); the parser does no prefix matching | — |
| 21 | `git push --no-force origin main` | unmodeled | negation not declared | — |
| 22 | `git push --dry-run=yes` | unmodeled | Bool given a value (git: "takes no value", verified) | — |
| 23 | `git -c core.sshCommand=/tmp/x push origin main` | push | config={`core.sshCommand=/tmp/x`} | network, override_config |
| 24 | `git --exec-path=/tmp/evil push` | unmodeled | `--exec-path` not declared on purpose | — |
| 25 | `git -C a -C b status` | unmodeled | non-repeated value option given twice (git would cd into `a/b`) | — |
| 26 | `git -Capp status` | status | directory=`app`. git refuses `-Capp` (verified); over-acceptance is harmless | — |
| 27 | `git -C push status` | status | directory=`push`; the value is taken before the command word | — |
| 28 | `git --git-dir ../o/.git --work-tree ../o log` | log | git_dir, work_tree (separate-word form, verified) | — |
| 29 | `git status -C .` | unmodeled | global option after the subcommand (git: "unknown switch", verified) | — |
| 30 | `git commit -am "fix parser"` | commit | all, message={`fix parser`} (bundle: value option ends the cluster, takes next word, verified) | — |
| 31 | `git commit -m"msg"` / `git commit -mmsg` | commit | message={`msg`} | — |
| 32 | `git commit -m=foo` | unmodeled | short value option with attached `=`; git reads `=foo` (verified), clap reads `foo` | — |
| 33 | `git commit -m a -m b` | commit | message={`a`,`b`} (repeated) | — |
| 34 | `git commit -m` | unmodeled | value missing | — |
| 35 | `git commit --amend --no-edit` | commit | amend, no_edit | rewrite_history |
| 36 | `git commit -n -m wip` | commit | no_verify (for commit, `-n` is `--no-verify`) | skip_hooks |
| 37 | `git commit --amen --no-edit` | unmodeled | prefix of `--amend`; git accepts it (verified) | — |
| 38 | `git commit --amend --dry-run` | commit | amend, dry_run | — |
| 39 | `git reset --hard HEAD~1` | reset | hard, commit=`HEAD~1` | discard_changes |
| 40 | `git reset HEAD -- src/lib.rs` | reset | commit=`HEAD`, pathspec={`src/lib.rs`} | — |
| 41 | `git reset -- HEAD` | reset | pathspec={`HEAD`}; commit absent (`--` splits the groups) | — |
| 42 | `git rebase -i --autosquash main` | rebase | interactive, autosquash, upstream=`main` | rewrite_history |
| 43 | `git rebase -x "cargo test" origin/main` | rebase | exec={`cargo test`}, upstream | rewrite_history, run_program |
| 44 | `git rebase --abort` | rebase | abort | — |
| 45 | `git clean -fdx` | clean | force, directories, ignored | discard_changes |
| 46 | `git clean -nfd` | clean | dry_run, force, directories | — |
| 47 | `git clean -i` | clean | interactive | discard_changes |
| 48 | `git checkout -- src/main.rs` | checkout | pathspec={`src/main.rs`} | discard_changes |
| 49 | `git checkout .` | checkout | target=`.` | discard_changes |
| 50 | `git checkout -b topic origin/main` | checkout | create=`topic`, target=`origin/main` | — |
| 51 | `git checkout src/main.rs` | checkout | target=`src/main.rs`. **Known gap**: git restores the file; the parser cannot tell a path from a branch | — |
| 52 | `git checkout -` | checkout | target=`-` (a lone `-` is an operand) | — |
| 53 | `git switch -c feature` | switch | create=`feature` | — |
| 54 | `git switch -f main` | switch | discard_changes, target=`main` | discard_changes |
| 55 | `git status -sb` | status | short, branch | — |
| 56 | `git status -uno` | status | untracked_files=`no` (optional value, attached) | — |
| 57 | `git status -u no` | status | untracked_files=`""`, pathspec={`no`} | — |
| 58 | `git log -5 --oneline` | unmodeled | `-<n>` is not expressible (no `-<n>` form) | — |
| 59 | `git log -n5 --oneline main` | log | max_count=`5` (a String: `type = "long"` is not implemented), oneline, revision={`main`} | — |
| 60 | `git log --max-count=many` | log | max_count=`many` (no `type = "long"`, so the value is not checked) | — |
| 61 | `git log -p -- src/` | log | patch, path={`src/`} | — |
| 62 | `git log --output=/tmp/x` | log | output=`/tmp/x` | write_file |
| 63 | `git diff --no-index /etc/passwd /dev/null` | diff | no_index, commit={`/etc/passwd`,`/dev/null`} | read_outside_repo |
| 64 | `git diff --cached -- src` | diff | cached, path={`src`} | — |
| 65 | `git diff --ext-diff HEAD~1` | diff | ext_diff, commit={`HEAD~1`} | run_program |
| 66 | `git add -A` | add | all | — |
| 67 | `git add -- -weird-name` | add | pathspec={`-weird-name`} | — |
| 68 | `git fetch -n origin` | fetch | no_tags (for fetch, `-n` is not dry-run) | network |
| 69 | `git fetch --upload-pack=/tmp/x origin` | fetch | upload_pack | network, run_program |
| 70 | `git pull --rebase` | pull | rebase=`""` | network, rewrite_history |
| 71 | `git pull -r origin main` | pull | rebase=`""`, remote, refspec | network, rewrite_history |
| 72 | `git pull --rebase=false origin main` | pull | rebase=`false` | network |
| 73 | `git --no-pager -c color.ui=always log` | log | no_pager, config={`color.ui=always`} | override_config |
| 74 | `git frobnicate` | unmodeled | subcommand=`frobnicate` | — |
| 75 | `git ci -m x` (config alias) | unmodeled | subcommand=`ci` | — |
| 76 | `git` / `git --version` | unmodeled | no command word | — |
| 77 | `git push --help` | unmodeled | `--help` not declared | — |
