# cargo pack corpus

Expected parses for `cargo.pack.toml` under the shell grammar in `crates/policy/src/pack/grammar.rs`.
`run.py` checks the action and the effects of every row.
A row shows only fields that differ from their defaults. `fx` is `context.input.effects`.
Shorthand: **B** = {update_lockfile, network, run_build_scripts}; **T** = B + execute_built_code.
Rows marked "verified" were checked against cargo 1.98 on this host.

| # | Command line | Action | Fields | fx |
|---|---|---|---|---|
| 1 | `cargo test` | test | — | T |
| 2 | `cargo t` | test | alias (EXT-1) | T |
| 3 | `cargo +stable test` | test | toolchain=`stable` (EXT-8) | T |
| 4 | `cargo +nightly -Zunstable-options t --locked` | test | toolchain=`nightly`, unstable={`unstable-options`}, locked | T − update_lockfile |
| 5 | `cargo --locked build` | build | locked (global before the command) | network, run_build_scripts |
| 6 | `cargo build --locked` | build | locked (global after the command, `anywhere`) | network, run_build_scripts |
| 7 | `cargo test --frozen` | test | frozen | run_build_scripts, execute_built_code |
| 8 | `cargo build --offline` | build | offline. `--offline` alone can still rewrite Cargo.lock | update_lockfile, run_build_scripts |
| 9 | `cargo b -r -p strands-box` | build | release, package={`strands-box`} | B |
| 10 | `cargo build -rp strands-box` | build | bundle: `-r` Bool, `-p` takes next word | B |
| 11 | `cargo build -p=strands-box` | unmodeled | short value option with `=` (clap reads `strands-box`, verified; git would read `=strands-box`) | — |
| 12 | `cargo build --manifest-path=crates/x/Cargo.toml` | build | manifest_path | B |
| 13 | `cargo --manifest-path x/Cargo.toml build` | unmodeled | not a global option; cargo refuses it too (verified) | — |
| 14 | `cargo test -p box --all-features -- --nocapture` | test | package={`box`}, all_features, test_args={`--nocapture`} | T |
| 15 | `cargo test parser -- --test-threads=1` | test | testname=`parser`, test_args={`--test-threads=1`} | T |
| 16 | `cargo test foo bar` | unmodeled | too many operands: `[-- <test_args>...]` is reached only after `--` (cargo: "unexpected argument", verified) | — |
| 17 | `cargo test --no-run` | test | no_run | B |
| 18 | `cargo test -F a,b -F c` | test | features={`a,b`,`c`}; values are not split | T |
| 19 | `cargo test --relea` | unmodeled | no prefix matching (cargo refuses it too, verified) | — |
| 20 | `cargo run --release -- --port 8080` | run | release, program_args={`--port`,`8080`} | T |
| 21 | `cargo run app --release` | run | program_args={`app`,`--release`}; release=false (EXT-9, verified) | T |
| 22 | `cargo run -- -- y` | run | program_args={`--`,`y`} (first `--` is the separator, verified) | T |
| 23 | `cargo r --bin server` | run | bin=`server` | T |
| 24 | `cargo update` | update | — | update_lockfile, network |
| 25 | `cargo update -p serde --precise 1.0.200` | update | package={`serde`}, precise | update_lockfile, network |
| 26 | `cargo update serde tokio` | update | spec={`serde`,`tokio`} | update_lockfile, network |
| 27 | `cargo update -n` | update | dry_run | network |
| 28 | `cargo update --locked` | update | locked (cargo then refuses to write) | network |
| 29 | `cargo add serde -F derive` | add | dependency={`serde`}, features={`derive`} | modify_manifest, update_lockfile, network |
| 30 | `cargo add --locked tokio` | add | locked | modify_manifest, network |
| 31 | `cargo add --dry-run anyhow` | add | dry_run | network |
| 32 | `cargo add` | unmodeled | required operand `<dependency>...` missing | — |
| 33 | `cargo add evil --git https://example.com/evil.git` | add | git=`https://…` | modify_manifest, update_lockfile, network |
| 34 | `cargo rm serde` | remove | alias `rm`, dependency={`serde`} | modify_manifest, update_lockfile |
| 35 | `cargo publish` | publish | — | publish_crate, network, run_build_scripts |
| 36 | `cargo publish -n` | publish | dry_run (`-n` verified) | network, run_build_scripts |
| 37 | `cargo publish --dry-run --no-verify` | publish | dry_run, no_verify | network |
| 38 | `cargo install ripgrep --locked` | install | crate={`ripgrep`}, locked | install_binary, network, run_build_scripts |
| 39 | `cargo install --list` | install | list | — |
| 40 | `cargo clippy --all-targets -- -D warnings` | clippy | all_targets, clippy_args={`-D`,`warnings`} | B |
| 41 | `cargo clippy --fix --allow-dirty` | clippy | fix, allow_dirty | B + modify_sources |
| 42 | `cargo fmt` | fmt | — | modify_sources |
| 43 | `cargo fmt --check` | fmt | check | — |
| 44 | `cargo fmt -- --check` | fmt | rustfmt_args={`--check`} (operand `when` rule) | — |
| 45 | `cargo -vv test` | test | verbose (bundled repeat of a Bool) | T |
| 46 | `cargo --config 'build.rustc-wrapper="/tmp/x"' build` | build | config={`build.rustc-wrapper="/tmp/x"`} | B + override_config |
| 47 | `cargo build --config net.offline=true` | build | config={…}. The parser does not read config values: update_lockfile stays | B + override_config |
| 48 | `cargo -v +stable test` | unmodeled | `+<toolchain>` is honoured only as the first argument (rustup) | — |
| 49 | `cargo -C sub build` | build | directory=`sub` (cargo refuses without nightly `-Z unstable-options`, verified; harmless) | B |
| 50 | `cargo build -C sub` | unmodeled | `-C` is before-only (cargo: "unexpected argument", verified) | — |
| 51 | `cargo tset` | unmodeled | subcommand=`tset` | — |
| 52 | `cargo nextest run` | unmodeled | external subcommand `cargo-nextest` | — |
| 53 | `cargo build -- x` | unmodeled | build has no operands | — |
| 54 | `cargo t` with `.cargo/config.toml` holding `[alias] t = "publish"` | test | **Wrong, L1.** A config-file alias overrides the built-in alias `t` with no warning (verified with `t = "check"`). Only `cargo test` spelled in full is safe | T |
