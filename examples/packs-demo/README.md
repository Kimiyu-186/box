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

The modes are `force`, `upstream`, `forbid`, `allow`, `nopack`, `unmodeled`, and `baseline`. They
show that one rule on the `force_push` effect catches every way to spell a force push, that a rule
can read a parsed field such as `remote`, that a pack action can be refused outright, that a box
with the pack but no pack rule behaves as before, that a policy naming a pack the box does not
install will not load, and that a command the pack does not model decides as `unmodeled`.

## cargo

```sh
bash examples/packs-demo/cargo/demo.sh
```

One rule on the `update_lockfile` effect refuses `cargo build`, `cargo test`, `cargo t`, and
`cargo update`, and lets `cargo build --locked` run. `Cargo.lock` is unchanged at the end. The box
runs the toolchain's own `cargo`, not the rustup proxy.

## aws

```sh
bash examples/packs-demo/aws/demo.sh
```

One rule on the `delete_data` effect refuses `aws s3 rm`, `aws s3 sync --delete`, and
`aws s3api delete-bucket`, and lets `aws sts get-caller-identity` and `aws s3 rm --dryrun` start.
The credentials are dummies and the endpoint is a closed local port, so nothing reaches AWS.

## temporal (git and cargo in one box)

```sh
bash examples/packs-demo/temporal/demo.sh            # two runs on one box directory
bash examples/packs-demo/temporal/demo.sh unscoped   # the effect rule over every action
```

`git push` is allowed only after a passing `cargo test` in the last hour, and a rule over
`Pack::cargo::Action::"*"` (every cargo action) refuses any cargo command that can rewrite
`Cargo.lock`. A push before a test, after a failing test, and after `cargo +stable test` (which
the toolchain cargo cannot run, so it exits non-zero) is refused. A push after a passing
`cargo t --locked` lands. A second `strands-box run` pushes again with no test, because history is
durable across runs.

`unscoped` writes the same effect rule over `action`, with no scope. The box refuses to start,
because a git action's effects are `Pack::git::Effect`, a different type from
`Pack::cargo::Effect`, so the rule does not type-check against both packs.

## Parse corpus

```sh
cargo build -p strands-box-policy --all-features --release --examples
python3 examples/packs/corpus/run.py target/release/examples/parse_pack examples/packs examples/packs/corpus
```

This runs every command line in `examples/packs/corpus/*.corpus.md` through the parser and checks
the action and the effects it reports. To see one parse, run
`target/release/examples/parse_pack examples/packs/cargo.pack.toml -- cargo +stable test`.
