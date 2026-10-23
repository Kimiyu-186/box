# Policy pack demos

Each demo runs a real box on macOS with one or more of the packs in `examples/packs/`, and prints
the allowed and refused commands plus the policy decision records. Build the box first:

```sh
cargo build -p strands-box --all-features --release
```

Each script uses `target/release/strands-box` beside this repository (set `BOX` to use another
binary) and recreates its scratch directory under `/tmp` (set `DEMO` to move it).

## git

```sh
bash examples/packs-demo/git/demo.sh <mode>
```

The modes are `force`, `upstream`, `forbid`, `allow`, `nopack`, `unmodeled`, `unmodeled-all`,
`symlink`, and `baseline`.

A git that the pack binds is decided only by its `Tool::git` action, never by `shell:spawn`, and a
pack action needs a permit like any other action. So most modes permit `Tool::git::Action::"*"`
and add one forbid. They show that one rule on the `force_push` effect catches every way to spell a
force push, that a rule can read a parsed field such as `remote`, that a pack action can be refused
outright, that a permit on `"*"` with no forbid lets the push land, and that a policy naming a pack
the box does not install will not load.

`unmodeled` permits only `status`, `log`, and `push`. Then `git log -1` and the config alias
`git ci` decide as `unmodeled`, and no permit names that action, so both are refused with no
forbid rule. `unmodeled-all` runs the same commands under a permit on `"*"`, which includes
`unmodeled`, and all of them run. `symlink` permits only `status`, so the git file run under a name
no `[tool]` declares (a planted `bin/g -> /usr/bin/git`) decides as `unmodeled` and is refused.
`baseline` installs no pack, so git decides as `shell:spawn`.

## cargo

```sh
bash examples/packs-demo/cargo/demo.sh
```

A permit on `Tool::cargo::Action::"*"` lets cargo run, and one rule on the `update_lockfile` effect refuses `cargo build`, `cargo test`, `cargo t`, and
`cargo update`, and lets `cargo build --locked` run. `Cargo.lock` is unchanged at the end. `cargo`
and `rustc` are the rustup proxies in `~/.cargo/bin`, two names for one file. The pack binds the
file only under the name `cargo`, so `rustc --version` runs with no cargo action, decided by its
`shell:spawn` permit.

## aws

```sh
bash examples/packs-demo/aws/demo.sh
```

A permit on `Tool::aws::Action::"*"` lets aws run, and one rule on the `delete_data` effect refuses `aws s3 rm`, `aws s3 sync --delete`, and
`aws s3api delete-bucket`, and lets `aws sts get-caller-identity` and `aws s3 rm --dryrun` start.
The credentials are dummies and the endpoint is a closed local port, so nothing reaches AWS.

## temporal (git and cargo in one box)

```sh
bash examples/packs-demo/temporal/demo.sh            # two runs on one box directory
bash examples/packs-demo/temporal/demo.sh unscoped   # the effect rule over every action
```

Permits on `Tool::git::Action::"*"` and `Tool::cargo::Action::"*"` let both programs run.
`git push` is allowed only after a passing `cargo test` in the last hour, and a rule over
`Tool::cargo::Action::"*"` (every cargo action) refuses any cargo command that can rewrite
`Cargo.lock`. A push before a test and after a failing test is refused. `cargo` is the rustup proxy,
so a passing `cargo +stable test --locked` runs, and the push after it lands. A second `strands-box run` pushes again with no test, because history is
durable across runs.

`unscoped` writes the same effect rule over `action`, with no scope. The box refuses to start,
because a git action's effects are `Tool::git::Effect`, a different type from
`Tool::cargo::Effect`, so the rule does not type-check against both packs.

## Parse corpus

```sh
cargo build -p strands-box-policy --all-features --release --examples
python3 examples/packs/corpus/run.py target/release/examples/parse_pack examples/packs examples/packs/corpus
```

This runs every command line in `examples/packs/corpus/*.corpus.md` through the parser and checks
the action and the effects it reports. To see one parse, run
`target/release/examples/parse_pack examples/packs/cargo.pack.toml -- cargo +stable test`.
